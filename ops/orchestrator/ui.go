// Interactive terminal dashboard for the supervisor (надзиратель).
//
// Built on tview: a profile list with live state, a details pane fed by the
// server's control port, a live server-output log, and a command line that
// speaks both the supervisor's vocabulary (start/stop/restart/kill) and the
// server's control commands (players, say, ban, end_match, ...).
//
// Everything here is presentation only. Process management lives in
// supervisor.go, the control protocol in client.go; the entry point wires
// them together through the callbacks at the bottom of this file.
package supervisor

import (
	"fmt"
	"io"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/gdamore/tcell/v2"
	"github.com/rivo/tview"
)

// ProfileRow is one line of the profile list and the details pane.
type ProfileRow struct {
	Name        string
	State       string
	PID         int
	Addr        string
	Uptime      string
	MemMB       float64
	CPU         float64
	Tags        []string
	BinPath     string
	Config      string
	WorkDir     string
	ControlPort uint16
}

// UI is the full-screen terminal dashboard for the supervisor.
type UI struct {
	app   *tview.Application
	pages *tview.Pages

	header      *tview.TextView
	profileList *tview.List
	detailsView *tview.TextView
	logView     *tview.TextView
	cmdInput    *tview.InputField
	statusBar   *tview.TextView

	// Create-profile form.
	createForm    *tview.Form
	formName      *tview.InputField
	formBin       *tview.InputField
	formConfig    *tview.InputField
	formWorkDir   *tview.InputField
	formPort      *tview.InputField
	formAutoStart *tview.Checkbox

	confirmModal *tview.Modal
	helpModal    *tview.Modal

	defaultBin    string
	defaultConfig string

	mu           sync.RWMutex
	profiles     []ProfileRow
	statuses     map[string]*StatusInfo
	selected     string
	logs         []string
	maxLogs      int
	history      []string
	historyPos   int
	statusMsg    string
	statusKind   int // 0 = info, 1 = ok, 2 = error
	started      bool
	updatingList bool
	running      bool

	onProfileSelect func(name string)
	onAction        func(action, name string) (string, error)
	onCommand       func(line string) (string, error)
	onCreateProfile func(p Profile) error
	onDeleteProfile func(name string) error
	onRefresh       func()
}

// NewUI creates a new dashboard.
func NewUI() *UI {
	return &UI{
		statuses:   make(map[string]*StatusInfo),
		maxLogs:    2000,
		running:    true,
		statusKind: 0,
	}
}

// SetDefaults sets the values pre-filled in the "new profile" form.
func (ui *UI) SetDefaults(binPath, configPath string) {
	ui.defaultBin = binPath
	ui.defaultConfig = configPath
}

// SelectedProfile returns the profile currently highlighted in the list.
func (ui *UI) SelectedProfile() string {
	ui.mu.RLock()
	defer ui.mu.RUnlock()
	return ui.selected
}

// Run builds the dashboard and runs the tview event loop until it exits.
func (ui *UI) Run() error {
	app := tview.NewApplication()
	ui.mu.Lock()
	ui.app = app
	ui.mu.Unlock()

	ui.buildUI()

	app.SetRoot(ui.pages, true)
	app.SetFocus(ui.profileList)
	app.EnableMouse(true)

	ui.mu.Lock()
	ui.started = true
	ui.mu.Unlock()

	go ui.refreshLoop()

	err := app.Run()

	ui.mu.Lock()
	ui.started = false
	ui.running = false
	ui.mu.Unlock()
	return err
}

// Stop stops the event loop.
func (ui *UI) Stop() {
	ui.mu.Lock()
	ui.running = false
	app := ui.app
	ui.mu.Unlock()
	if app != nil {
		app.Stop()
	}
}

// LogWriter returns an io.Writer that appends server output to the log pane.
func (ui *UI) LogWriter() io.Writer {
	return uiLogWriter{ui: ui}
}

type uiLogWriter struct{ ui *UI }

func (w uiLogWriter) Write(p []byte) (int, error) {
	text := strings.TrimRight(string(p), "\r\n")
	for _, line := range strings.Split(text, "\n") {
		if strings.TrimSpace(line) != "" {
			w.ui.AddLog(line)
		}
	}
	return len(p), nil
}

// ---- construction ----------------------------------------------------------

func (ui *UI) buildUI() {
	ui.pages = tview.NewPages()

	ui.header = tview.NewTextView().SetDynamicColors(true)
	ui.header.SetTextColor(tcell.ColorWhite)

	// Profile list with action buttons underneath.
	ui.profileList = tview.NewList()
	ui.profileList.ShowSecondaryText(false)
	ui.profileList.SetUseStyleTags(true, false)
	ui.profileList.SetHighlightFullLine(true)
	ui.profileList.SetSelectedBackgroundColor(tcell.ColorDarkCyan)
	ui.profileList.SetSelectedTextColor(tcell.ColorWhite)
	ui.profileList.SetBorder(true)
	ui.profileList.SetBorderColor(tcell.ColorTeal)
	ui.profileList.SetTitle(" Profiles ")
	ui.profileList.SetChangedFunc(func(index int, mainText, secondaryText string, shortcut rune) {
		ui.mu.Lock()
		if ui.updatingList || index < 0 || index >= len(ui.profiles) {
			ui.mu.Unlock()
			return
		}
		name := ui.profiles[index].Name
		ui.selected = name
		ui.mu.Unlock()
		ui.renderDetails()
		if ui.onProfileSelect != nil {
			ui.onProfileSelect(name)
		}
	})

	buttons1 := tview.NewFlex().
		AddItem(ui.mkButton("Start", func() { ui.runAction("start") }), 0, 1, false).
		AddItem(ui.mkButton("Stop", func() { ui.runAction("stop") }), 0, 1, false).
		AddItem(ui.mkButton("Restart", func() { ui.runAction("restart") }), 0, 1, false)

	buttons2 := tview.NewFlex().
		AddItem(ui.mkButton("Kill", func() { ui.runAction("kill") }), 0, 1, false).
		AddItem(ui.mkButton("Delete", func() { ui.confirmDelete() }), 0, 1, false).
		AddItem(ui.mkButton("New", func() { ui.showCreatePage() }), 0, 1, false)

	leftPanel := tview.NewFlex().
		SetDirection(tview.FlexRow).
		AddItem(ui.profileList, 0, 1, true).
		AddItem(buttons1, 1, 0, false).
		AddItem(buttons2, 1, 0, false)

	ui.detailsView = tview.NewTextView().SetDynamicColors(true).SetWordWrap(true)
	ui.detailsView.SetBorder(true)
	ui.detailsView.SetBorderColor(tcell.ColorTeal)
	ui.detailsView.SetTitle(" Profile ")
	ui.detailsView.SetText("[gray]Select a profile.[white]")

	ui.logView = tview.NewTextView().SetDynamicColors(true).SetScrollable(true)
	ui.logView.SetBorder(true)
	ui.logView.SetBorderColor(tcell.ColorTeal)
	ui.logView.SetTitle(" Live logs ")

	ui.cmdInput = tview.NewInputField().SetLabel("Command: ")
	ui.cmdInput.SetLabelColor(tcell.ColorYellow)
	ui.cmdInput.SetFieldBackgroundColor(tcell.ColorBlack)
	ui.cmdInput.SetFieldTextColor(tcell.ColorWhite)
	ui.cmdInput.SetPlaceholder("say hello | players | ban 7 spam | @beta status | start beta | help")
	ui.cmdInput.SetDoneFunc(func(key tcell.Key) {
		if key != tcell.KeyEnter {
			return
		}
		line := ui.cmdInput.GetText()
		ui.cmdInput.SetText("")
		ui.app.SetFocus(ui.profileList)
		ui.submitCommand(line)
	})
	ui.cmdInput.SetInputCapture(func(event *tcell.EventKey) *tcell.EventKey {
		switch event.Key() {
		case tcell.KeyUp:
			ui.historyPrev()
			return nil
		case tcell.KeyDown:
			ui.historyNext()
			return nil
		case tcell.KeyEsc:
			ui.app.SetFocus(ui.profileList)
			return nil
		}
		return event
	})

	ui.statusBar = tview.NewTextView().SetDynamicColors(true)
	ui.statusBar.SetText("[gray]Enter runs a command. F6 focuses the command line. F1 shows help.[white]")

	grid := tview.NewGrid().
		SetRows(2, 0, 1, 1).
		SetColumns(36, 0, 0).
		AddItem(ui.header, 0, 0, 1, 3, 0, 0, false).
		AddItem(leftPanel, 1, 0, 1, 1, 0, 0, true).
		AddItem(ui.detailsView, 1, 1, 1, 1, 0, 0, false).
		AddItem(ui.logView, 1, 2, 1, 1, 0, 0, false).
		AddItem(ui.cmdInput, 2, 0, 1, 3, 0, 0, false).
		AddItem(ui.statusBar, 3, 0, 1, 3, 0, 0, false)

	ui.pages.AddPage("main", grid, true, true)
	ui.pages.AddPage("create", ui.buildCreatePage(), true, false)
	ui.pages.AddPage("help", ui.buildHelpModal(), true, false)
	ui.pages.AddPage("confirm", ui.buildConfirmModal(), true, false)

	ui.app.SetInputCapture(ui.globalKeys)
}

func (ui *UI) mkButton(label string, fn func()) *tview.Button {
	btn := tview.NewButton(label)
	btn.SetSelectedFunc(func() {
		go fn()
	})
	return btn
}

func (ui *UI) buildCreatePage() tview.Primitive {
	ui.formName = tview.NewInputField().SetLabel("Name").SetFieldWidth(46)
	ui.formBin = tview.NewInputField().SetLabel("Server binary").SetFieldWidth(46)
	ui.formConfig = tview.NewInputField().SetLabel("Config file").SetFieldWidth(46)
	ui.formWorkDir = tview.NewInputField().SetLabel("Work dir").SetFieldWidth(46)
	ui.formPort = tview.NewInputField().SetLabel("Control port").SetFieldWidth(10).SetText("7778")
	ui.formAutoStart = tview.NewCheckbox().SetLabel("Auto-restart if the process crashes")

	ui.createForm = tview.NewForm().
		AddFormItem(ui.formName).
		AddFormItem(ui.formBin).
		AddFormItem(ui.formConfig).
		AddFormItem(ui.formWorkDir).
		AddFormItem(ui.formPort).
		AddFormItem(ui.formAutoStart).
		AddButton("Create", func() { ui.submitCreateProfile() }).
		AddButton("Cancel", func() { ui.hidePage("create") })
	ui.createForm.SetBorder(true)
	ui.createForm.SetTitle(" New server profile ")
	ui.createForm.SetBorderColor(tcell.ColorTeal)
	ui.createForm.SetCancelFunc(func() { ui.hidePage("create") })

	wrapper := tview.NewGrid().
		SetColumns(0, 62, 0).
		SetRows(0, 12, 0).
		AddItem(ui.createForm, 1, 1, 1, 1, 0, 0, true)
	return wrapper
}

func (ui *UI) buildHelpModal() *tview.Modal {
	ui.helpModal = tview.NewModal().
		SetText(helpText).
		AddButtons([]string{"OK"}).
		SetDoneFunc(func(buttonIndex int, buttonLabel string) {
			ui.hidePage("help")
		})
	return ui.helpModal
}

func (ui *UI) buildConfirmModal() *tview.Modal {
	ui.confirmModal = tview.NewModal().
		AddButtons([]string{"Delete", "Cancel"}).
		SetDoneFunc(func(buttonIndex int, buttonLabel string) {
			ui.hidePage("confirm")
			if buttonLabel != "Delete" {
				return
			}
			name := ui.SelectedProfile()
			if name == "" || ui.onDeleteProfile == nil {
				return
			}
			go func() {
				if err := ui.onDeleteProfile(name); err != nil {
					ui.SetError(fmt.Sprintf("delete %s: %v", name, err))
					return
				}
				ui.SetOK(fmt.Sprintf("deleted profile %q", name))
				ui.AddLog(fmt.Sprintf("[supervisor] deleted profile %q", name))
				ui.triggerRefresh()
			}()
		})
	return ui.confirmModal
}

const helpText = `RFS Server Supervisor — keys

  F1   this help              F6   focus command line
  F2   new profile            F7   restart profile
  F3   start profile          F8   kill profile
  F4   stop profile           F9   delete profile
  F5   refresh now            Ctrl+C quit the dashboard

Command line (Enter runs it)

  start|stop|restart|kill [name]   process actions
  delete <name>                    delete a stopped profile
  say-all <text>                   announcement to every running server
  status-all                       status of every running server
  @<profile> <command>             target a specific profile
  <command> [args]                 control command on the selected profile

Control commands: status perf net players config log watch audit say kick
                  stop reload ban unban banlist end_match start_match
                  restart_match

Examples: players · say hello · ban 7 cheating · kick 7 spam
          end_match · start_match · @beta status`

// ---- keys ------------------------------------------------------------------

func (ui *UI) globalKeys(event *tcell.EventKey) *tcell.EventKey {
	front, _ := ui.pages.GetFrontPage()
	if front != "main" {
		if event.Key() == tcell.KeyEsc {
			ui.hidePage(front)
			return nil
		}
		return event
	}

	switch event.Key() {
	case tcell.KeyF1:
		ui.showPage("help", ui.helpModal)
		return nil
	case tcell.KeyF2:
		ui.showCreatePage()
		return nil
	case tcell.KeyF3:
		ui.runAction("start")
		return nil
	case tcell.KeyF4:
		ui.runAction("stop")
		return nil
	case tcell.KeyF5:
		ui.triggerRefresh()
		return nil
	case tcell.KeyF6:
		ui.app.SetFocus(ui.cmdInput)
		return nil
	case tcell.KeyF7:
		ui.runAction("restart")
		return nil
	case tcell.KeyF8:
		ui.runAction("kill")
		return nil
	case tcell.KeyF9:
		ui.confirmDelete()
		return nil
	case tcell.KeyEsc:
		if ui.app.GetFocus() == ui.cmdInput {
			ui.app.SetFocus(ui.profileList)
			return nil
		}
	}
	return event
}

func (ui *UI) historyPrev() {
	ui.mu.Lock()
	defer ui.mu.Unlock()
	if len(ui.history) == 0 {
		return
	}
	if ui.historyPos > 0 {
		ui.historyPos--
	}
	ui.cmdInput.SetText(ui.history[ui.historyPos])
}

func (ui *UI) historyNext() {
	ui.mu.Lock()
	defer ui.mu.Unlock()
	if len(ui.history) == 0 {
		return
	}
	if ui.historyPos < len(ui.history)-1 {
		ui.historyPos++
		ui.cmdInput.SetText(ui.history[ui.historyPos])
		return
	}
	ui.historyPos = len(ui.history)
	ui.cmdInput.SetText("")
}

// ---- pages -----------------------------------------------------------------

func (ui *UI) showPage(name string, p tview.Primitive) {
	ui.pages.ShowPage(name)
	ui.app.SetFocus(p)
}

func (ui *UI) hidePage(name string) {
	ui.pages.HidePage(name)
	ui.app.SetFocus(ui.profileList)
}

func (ui *UI) showCreatePage() {
	ui.formName.SetText("")
	ui.formBin.SetText(ui.defaultBin)
	ui.formConfig.SetText(ui.defaultConfig)
	ui.formWorkDir.SetText("")
	ui.formPort.SetText("7778")
	ui.formAutoStart.SetChecked(false)
	ui.showPage("create", ui.createForm)
}

func (ui *UI) confirmDelete() {
	name := ui.SelectedProfile()
	if name == "" {
		ui.SetError("no profile selected")
		return
	}
	ui.confirmModal.SetText(fmt.Sprintf("Delete profile %q?\n\nThe profile file is removed. A running server keeps running.", name))
	ui.showPage("confirm", ui.confirmModal)
}

// ---- commands --------------------------------------------------------------

func (ui *UI) runAction(action string) {
	name := ui.SelectedProfile()
	if name == "" {
		ui.SetError("no profile selected")
		return
	}
	if ui.onAction == nil {
		return
	}
	ui.AddLog(fmt.Sprintf("[supervisor] %s %s", action, name))
	go func() {
		msg, err := ui.onAction(action, name)
		if err != nil {
			ui.SetError(fmt.Sprintf("%s %s: %v", action, name, err))
			return
		}
		if msg != "" {
			ui.SetOK(msg)
		}
		ui.triggerRefresh()
	}()
}

func (ui *UI) submitCommand(line string) {
	line = strings.TrimSpace(line)
	if line == "" || ui.onCommand == nil {
		return
	}

	ui.mu.Lock()
	ui.history = append(ui.history, line)
	ui.historyPos = len(ui.history)
	ui.mu.Unlock()

	ui.AddLog("[cyan]>[white] " + line)
	go func() {
		out, err := ui.onCommand(line)
		if err != nil {
			ui.SetError(err.Error())
			return
		}
		if strings.TrimSpace(out) != "" {
			ui.AddLog(out)
		}
		ui.SetOK("done: " + line)
		ui.triggerRefresh()
	}()
}

func (ui *UI) triggerRefresh() {
	if ui.onRefresh != nil {
		ui.onRefresh()
	}
}

func (ui *UI) submitCreateProfile() {
	name := strings.TrimSpace(ui.formName.GetText())
	if name == "" {
		ui.setStatusDirect("profile name is required", 2)
		return
	}
	if strings.ContainsAny(name, `/\:*?"<>| `) {
		ui.setStatusDirect("profile name must not contain spaces or path separators", 2)
		return
	}

	port := uint16(7778)
	if raw := strings.TrimSpace(ui.formPort.GetText()); raw != "" {
		n, err := strconv.Atoi(raw)
		if err != nil || n <= 0 || n > 65535 {
			ui.setStatusDirect("control port must be 1..65535", 2)
			return
		}
		port = uint16(n)
	}

	binPath := strings.TrimSpace(ui.formBin.GetText())
	if binPath == "" {
		ui.setStatusDirect("server binary path is required", 2)
		return
	}

	p := DefaultProfile(name, binPath)
	p.Config = strings.TrimSpace(ui.formConfig.GetText())
	p.WorkDir = strings.TrimSpace(ui.formWorkDir.GetText())
	p.ControlPort = port
	p.AutoRestart = ui.formAutoStart.IsChecked()

	ui.hidePage("create")
	ui.AddLog(fmt.Sprintf("[supervisor] creating profile %q", name))

	if ui.onCreateProfile == nil {
		return
	}
	go func() {
		if err := ui.onCreateProfile(p); err != nil {
			ui.SetError(fmt.Sprintf("create %s: %v", name, err))
			return
		}
		ui.SetOK(fmt.Sprintf("created profile %q", name))
		ui.triggerRefresh()
	}()
}

// ---- rendering -------------------------------------------------------------

func (ui *UI) refreshLoop() {
	ticker := time.NewTicker(2 * time.Second)
	defer ticker.Stop()
	for range ticker.C {
		ui.mu.RLock()
		running := ui.running
		ui.mu.RUnlock()
		if !running {
			return
		}
		ui.triggerRefresh()
	}
}

// UpdateProfiles replaces the dashboard data and repaints.
func (ui *UI) UpdateProfiles(profiles []ProfileRow, statuses map[string]*StatusInfo) {
	if statuses == nil {
		statuses = make(map[string]*StatusInfo)
	}
	ui.mu.Lock()
	ui.profiles = profiles
	ui.statuses = statuses
	if ui.selected == "" && len(profiles) > 0 {
		ui.selected = profiles[0].Name
	} else if len(profiles) > 0 {
		found := false
		for _, p := range profiles {
			if p.Name == ui.selected {
				found = true
				break
			}
		}
		if !found {
			ui.selected = profiles[0].Name
		}
	}
	ui.mu.Unlock()

	ui.queueDraw(ui.renderAll)
}

// UpdateDetails repaints the details pane for one profile (compatibility
// helper; the dashboard normally refreshes through UpdateProfiles).
func (ui *UI) UpdateDetails(info *StatusInfo) {
	if info == nil {
		return
	}
	ui.mu.Lock()
	ui.statuses[info.Name] = info
	ui.mu.Unlock()
	ui.queueDraw(ui.renderDetails)
}

func (ui *UI) renderAll() {
	ui.renderHeader()

	ui.mu.Lock()
	profiles := append([]ProfileRow(nil), ui.profiles...)
	selected := ui.selected
	ui.updatingList = true
	ui.mu.Unlock()

	ui.profileList.Clear()
	for _, p := range profiles {
		text := fmt.Sprintf("%-24s [%s]%s[white]",
			truncate(p.Name, 24),
			stateTag(p.State),
			p.State)
		ui.profileList.AddItem(text, "", 0, nil)
	}
	if len(profiles) == 0 {
		ui.profileList.AddItem("[gray]no profiles — press F2 to create one[white]", "", 0, nil)
	} else {
		index := 0
		for i, p := range profiles {
			if p.Name == selected {
				index = i
				break
			}
		}
		ui.profileList.SetCurrentItem(index)
	}

	ui.mu.Lock()
	ui.updatingList = false
	ui.mu.Unlock()

	ui.renderDetails()
}

func (ui *UI) renderHeader() {
	ui.mu.RLock()
	total := len(ui.profiles)
	running := 0
	for _, p := range ui.profiles {
		if p.State == "running" || p.State == "starting" {
			running++
		}
	}
	ui.mu.RUnlock()

	ui.header.SetText(fmt.Sprintf(
		"[::b]RFS SERVER SUPERVISOR[white]   [green]%d running[white] / %d profiles   [gray]%s[white]\n"+
			"[gray]F1 help  F2 new  F3 start  F4 stop  F6 command  F7 restart  F8 kill  F9 delete  F5 refresh  Ctrl+C quit[white]",
		running, total, time.Now().Format("2006-01-02 15:04:05")))
}

// renderDetails repaints the details pane from cached data. Call it on the
// UI goroutine only.
func (ui *UI) renderDetails() {
	ui.mu.RLock()
	selected := ui.selected
	var row ProfileRow
	found := false
	for _, p := range ui.profiles {
		if p.Name == selected {
			row = p
			found = true
			break
		}
	}
	info, hasInfo := ui.statuses[selected]
	ui.mu.RUnlock()

	if !found {
		ui.detailsView.SetText("[gray]No profile selected.\n\nPress F2 to create one.[white]")
		return
	}

	var b strings.Builder
	fmt.Fprintf(&b, "[::b]%s[white]\n", tview.Escape(row.Name))
	fmt.Fprintf(&b, "[%s]%s[white]\n\n", stateTag(row.State), row.State)

	if !hasInfo {
		fmt.Fprintf(&b, "[gray]Not running.[white]\n\n")
		writeProfileConfig(&b, row)
		fmt.Fprintf(&b, "\n[gray]F3 start · F6 command line[white]\n")
		ui.detailsView.SetText(b.String())
		return
	}

	fmt.Fprintf(&b, "[gray]PID[white]            %-10d  [gray]Control port[white]  %d\n", info.PID, info.ControlPort)
	fmt.Fprintf(&b, "[gray]Address[white]        %-10s  [gray]Uptime[white]        %s\n", orDash(info.Addr), info.Uptime.Round(time.Second))
	fmt.Fprintf(&b, "[gray]Memory[white]         %-10s  [gray]CPU[white]           %s\n",
		fmt.Sprintf("%.1f MB", info.MemMB), fmt.Sprintf("%.1f%%", info.CPUPercent))
	fmt.Fprintf(&b, "[gray]Restarts[white]       %-10d  [gray]Health[white]        %s\n",
		info.RestartCount, healthTag(info.HealthOK))
	fmt.Fprintf(&b, "[gray]Match[white]          %-10s  [gray]Tick[white]          %d\n", orDash(info.MatchState), info.Tick)
	fmt.Fprintf(&b, "[gray]Players[white]        %-10d  [gray]Ships[white]         %d\n", info.Players, info.Ships)
	fmt.Fprintf(&b, "[gray]Connections[white]    %-10d  [gray]Entities[white]      %d\n", info.Connections, info.Entities)
	fmt.Fprintf(&b, "[gray]Tick rate[white]      %.1f Hz\n", info.TickRate)
	fmt.Fprintf(&b, "\n")
	writeProfileConfig(&b, row)
	fmt.Fprintf(&b, "\n[gray]F6 command line · F4 stop · F7 restart[white]\n")

	ui.detailsView.SetText(b.String())
}

func writeProfileConfig(b *strings.Builder, row ProfileRow) {
	if row.BinPath != "" {
		fmt.Fprintf(b, "[gray]Binary[white]         %s\n", tview.Escape(row.BinPath))
	}
	if row.Config != "" {
		fmt.Fprintf(b, "[gray]Config[white]         %s\n", tview.Escape(row.Config))
	}
	if row.WorkDir != "" {
		fmt.Fprintf(b, "[gray]Work dir[white]       %s\n", tview.Escape(row.WorkDir))
	}
	if len(row.Tags) > 0 {
		fmt.Fprintf(b, "[gray]Tags[white]           %s\n", tview.Escape(strings.Join(row.Tags, ", ")))
	}
}

// AddLog appends a line to the live log pane.
func (ui *UI) AddLog(text string) {
	for _, line := range strings.Split(strings.TrimRight(text, "\r\n"), "\n") {
		ui.appendLog(line)
	}
	ui.queueDraw(ui.renderLog)
}

func (ui *UI) appendLog(line string) {
	entry := fmt.Sprintf("[gray]%s[white] %s", time.Now().Format("15:04:05"), tview.Escape(line))
	ui.mu.Lock()
	ui.logs = append(ui.logs, entry)
	if len(ui.logs) > ui.maxLogs {
		ui.logs = ui.logs[len(ui.logs)-ui.maxLogs:]
	}
	ui.mu.Unlock()
}

func (ui *UI) renderLog() {
	ui.mu.RLock()
	text := strings.Join(ui.logs, "\n")
	ui.mu.RUnlock()
	ui.logView.SetText(text)
	ui.logView.ScrollToEnd()
}

// SetStatus shows an informational status-bar message.
func (ui *UI) SetStatus(msg string) {
	ui.setStatus(msg, 0, true)
}

// SetOK shows a success status-bar message.
func (ui *UI) SetOK(msg string) {
	ui.setStatus(msg, 1, true)
}

// SetError shows an error status-bar message.
func (ui *UI) SetError(msg string) {
	ui.setStatus(msg, 2, true)
}

func (ui *UI) setStatus(msg string, kind int, queue bool) {
	ui.mu.Lock()
	ui.statusMsg = msg
	ui.statusKind = kind
	ui.mu.Unlock()
	if queue {
		ui.queueDraw(ui.renderStatus)
		return
	}
	ui.renderStatus()
}

// setStatusDirect updates the status bar from the UI goroutine.
func (ui *UI) setStatusDirect(msg string, kind int) {
	ui.setStatus(msg, kind, false)
}

func (ui *UI) renderStatus() {
	ui.mu.RLock()
	msg := ui.statusMsg
	kind := ui.statusKind
	ui.mu.RUnlock()

	if msg == "" {
		ui.statusBar.SetText("[gray]Enter runs a command. F6 focuses the command line. F1 shows help.[white]")
		return
	}

	color, mark := "white", "•"
	switch kind {
	case 1:
		color, mark = "green", "✓"
	case 2:
		color, mark = "red", "✗"
	}
	ui.statusBar.SetText(fmt.Sprintf("[%s]%s %s[white]", color, mark, tview.Escape(msg)))
}

func (ui *UI) queueDraw(f func()) {
	ui.mu.RLock()
	app := ui.app
	started := ui.started
	ui.mu.RUnlock()
	if app == nil || !started {
		return
	}
	app.QueueUpdateDraw(f)
}

// ---- callbacks -------------------------------------------------------------

// SetProfileSelectCallback is called when the highlighted profile changes.
func (ui *UI) SetProfileSelectCallback(fn func(name string)) {
	ui.onProfileSelect = fn
}

// SetActionCallback handles process actions (start/stop/restart/kill).
func (ui *UI) SetActionCallback(fn func(action, name string) (string, error)) {
	ui.onAction = fn
}

// SetCommandCallback handles a command line typed into the dashboard.
func (ui *UI) SetCommandCallback(fn func(line string) (string, error)) {
	ui.onCommand = fn
}

// SetCreateProfileCallback creates and persists a new profile.
func (ui *UI) SetCreateProfileCallback(fn func(p Profile) error) {
	ui.onCreateProfile = fn
}

// SetDeleteProfileCallback deletes a stopped profile.
func (ui *UI) SetDeleteProfileCallback(fn func(name string) error) {
	ui.onDeleteProfile = fn
}

// SetRefreshCallback is called by the dashboard every two seconds and on F5.
func (ui *UI) SetRefreshCallback(fn func()) {
	ui.onRefresh = fn
}

// ---- small helpers ---------------------------------------------------------

func stateTag(state string) string {
	switch state {
	case "running":
		return "green"
	case "starting", "stopping":
		return "yellow"
	case "error", "unhealthy":
		return "red"
	case "stopped":
		return "gray"
	default:
		return "white"
	}
}

func healthTag(ok bool) string {
	if ok {
		return "[green]OK[white]"
	}
	return "[red]FAIL[white]"
}

func orDash(s string) string {
	if s == "" {
		return "-"
	}
	return s
}

func truncate(s string, n int) string {
	r := []rune(s)
	if len(r) <= n {
		return s
	}
	if n <= 1 {
		return string(r[:n])
	}
	return string(r[:n-1]) + "…"
}
