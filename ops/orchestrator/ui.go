// Package supervisor provides terminal UI parts for the supervisor (надзиратель).
package supervisor

import (
	"fmt"
	"strings"
	"time"

	"github.com/rivo/tview"
	"github.com/gdamore/tcell/v2"
)

// ProfileRow is one line of the profile list.
type ProfileRow struct {
	Name   string
	State  string
	PID    int
	Addr   string
	Uptime string
	MemMB  float64
	CPU    float64
	Tags   []string
}

// UI is the full-screen terminal dashboard for the supervisor.
type UI struct {
	app      *tview.Application
	pages    *tview.Pages
	grid     *tview.Grid

	profileList *tview.List
	detailsView *tview.TextView
	logView     *tview.TextView
	cmdInput    *tview.InputField
	statusBar   *tview.TextView

	profiles  []ProfileRow
	statuses  map[string]*StatusInfo
	logs      []string

	onProfileSelect func(name string)
	onCommand       func(command string)
	onCreateProfile func(name, binPath string)
	onRefresh       func()

	running bool
}

// NewUI creates a new UI instance.
func NewUI() *UI {
	return &UI{
		profiles:  make([]ProfileRow, 0),
		statuses:  make(map[string]*StatusInfo),
		logs:      make([]string, 0),
		running:   true,
	}
}

// Run starts the UI event loop.
func (ui *UI) Run() error {
	app := tview.NewApplication()
	ui.app = app

	ui.buildUI()

	app.SetRoot(ui.pages, true).SetFocus(ui.profileList)

	go ui.refreshLoop()

	return app.Run()
}

// Stop stops the UI.
func (ui *UI) Stop() {
	ui.running = false
	if ui.app != nil {
		ui.app.Stop()
	}
}

func (ui *UI) buildUI() {
	ui.pages = tview.NewPages()

	// Left panel: profile list
	ui.profileList = tview.NewList().
		ShowSecondaryText(false)

	leftPanel := tview.NewFlex().
		SetDirection(tview.FlexRow).
		AddItem(ui.profileList, 0, 1, true).
		AddItem(ui.buildProfileControls(), 3, 0, false)

	// Center panel: profile details
	ui.detailsView = tview.NewTextView()
	ui.detailsView.SetWordWrap(true)
	ui.detailsView.SetBorder(true)
	ui.detailsView.SetTitle(" Profile Details ")

	centerPanel := tview.NewFlex().
		SetDirection(tview.FlexRow).
		AddItem(ui.detailsView, 0, 1, false)

	// Right panel: live logs
	ui.logView = tview.NewTextView()
	ui.logView.SetWordWrap(true)
	ui.logView.SetChangedFunc(func() {
		ui.app.Draw()
	})
	ui.logView.SetBorder(true)
	ui.logView.SetTitle(" Live Logs ")

	rightPanel := tview.NewFlex().
		SetDirection(tview.FlexRow).
		AddItem(ui.logView, 0, 1, false)

	// Bottom panel: command line
	ui.cmdInput = tview.NewInputField().
		SetLabel("Command: ")

	ui.cmdInput.SetDoneFunc(func(key tcell.Key) {
		if key == tcell.KeyEnter {
			cmd := ui.cmdInput.GetText()
			ui.cmdInput.SetText("")
			if ui.onCommand != nil {
				go ui.onCommand(cmd)
			}
		}
	})

	ui.statusBar = tview.NewTextView()
	ui.statusBar.SetDynamicColors(true)
	ui.statusBar.SetText("[white]Ready[white]")
	ui.statusBar.SetBorder(true)
	ui.statusBar.SetTitle(" Status ")

	bottomPanel := tview.NewFlex().
		SetDirection(tview.FlexRow).
		AddItem(ui.cmdInput, 1, 0, false).
		AddItem(ui.statusBar, 1, 0, false)

	ui.grid = tview.NewGrid().
		SetRows(0, 0).
		SetColumns(30, 0, 40).
		AddItem(leftPanel, 0, 0, 1, 1, 0, 0, false).
		AddItem(centerPanel, 0, 1, 1, 1, 0, 0, false).
		AddItem(rightPanel, 0, 2, 1, 1, 0, 0, false).
		AddItem(bottomPanel, 1, 0, 1, 3, 0, 0, false)

	ui.profileList.SetSelectedFunc(func(index int, text string, secondaryText string, rune rune) {
		if index >= 0 && index < len(ui.profiles) {
			name := ui.profiles[index].Name
			if ui.onProfileSelect != nil {
				ui.onProfileSelect(name)
			}
		}
	})

	ui.profileList.SetInputCapture(func(event *tcell.EventKey) *tcell.EventKey {
		switch event.Key() {
		case tcell.KeyF2:
			ui.showCreateProfileModal()
			return nil
		case tcell.KeyF3:
			if len(ui.profiles) > 0 {
				name := ui.profiles[ui.profileList.GetCurrentItem()].Name
				ui.executeProfileCmd(name, "status")
			}
			return nil
		case tcell.KeyF4:
			if len(ui.profiles) > 0 {
				name := ui.profiles[ui.profileList.GetCurrentItem()].Name
				ui.executeProfileCmd(name, "stop")
			}
			return nil
		case tcell.KeyF5:
			if ui.onRefresh != nil {
				go ui.onRefresh()
			}
			return nil
		}
		return event
	})

	ui.pages.AddPage("main", ui.grid, true, false)
	ui.pages.AddPage("create", ui.buildCreateProfileModal(), true, false)
	ui.pages.AddPage("help", ui.buildHelpModal(), true, false)

	ui.setStatus("Ready. Press F1 for help.")
}

func (ui *UI) buildProfileControls() *tview.Flex {
	btnCreate := tview.NewButton("Create (F2)").
		SetSelectedFunc(func() {
			ui.showCreateProfileModal()
		})
	btnStart := tview.NewButton("Start").
		SetSelectedFunc(func() {
			if len(ui.profiles) > 0 {
				name := ui.profiles[ui.profileList.GetCurrentItem()].Name
				ui.executeProfileCmd(name, "start")
			}
		})
	btnStop := tview.NewButton("Stop").
		SetSelectedFunc(func() {
			if len(ui.profiles) > 0 {
				name := ui.profiles[ui.profileList.GetCurrentItem()].Name
				ui.executeProfileCmd(name, "stop")
			}
		})
	btnRestart := tview.NewButton("Restart").
		SetSelectedFunc(func() {
			if len(ui.profiles) > 0 {
				name := ui.profiles[ui.profileList.GetCurrentItem()].Name
				ui.executeProfileCmd(name, "restart")
			}
		})
	btnRefresh := tview.NewButton("Refresh (F5)").
		SetSelectedFunc(func() {
			if ui.onRefresh != nil {
				go ui.onRefresh()
			}
		})

	flex := tview.NewFlex().
		SetDirection(tview.FlexRow).
		AddItem(btnCreate, 1, 0, false).
		AddItem(btnStart, 1, 0, false).
		AddItem(btnStop, 1, 0, false).
		AddItem(btnRestart, 1, 0, false).
		AddItem(btnRefresh, 1, 0, false)

	return flex
}

func (ui *UI) buildCreateProfileModal() *tview.Modal {
	nameInput := tview.NewInputField().SetLabel("Name: ").SetFieldWidth(30)
	binInput := tview.NewInputField().SetLabel("Binary: ").SetFieldWidth(30)
	configInput := tview.NewInputField().SetLabel("Config: ").SetFieldWidth(30)
	portInput := tview.NewInputField().SetLabel("Port: ").SetFieldWidth(30).SetText("7778")

	modal := tview.NewModal().
		SetText("Create Profile").
		AddButtons([]string{"Create", "Cancel"}).
		SetDoneFunc(func(buttonIndex int, buttonLabel string) {
			if buttonLabel == "Create" {
				name := nameInput.GetText()
				binPath := binInput.GetText()
				config := configInput.GetText()
				port := portInput.GetText()
				if name == "" || binPath == "" {
					ui.setStatus("Name and binary path are required")
					return
				}
				if ui.onCreateProfile != nil {
					go ui.onCreateProfile(name, binPath)
				}
			}
			ui.pages.SwitchToPage("main")
		})

	// Build form layout
	form := tview.NewFlex().
		SetDirection(tview.FlexRow).
		AddItem(nameInput, 1, 0, false).
		AddItem(binInput, 1, 0, false).
		AddItem(configInput, 1, 0, false).
		AddItem(portInput, 1, 0, false)

	// Wrap modal content
	content := tview.NewFlex().
		SetDirection(tview.FlexRow).
		AddItem(form, 0, 1, false)

	_ = content

	return modal
}

func (ui *UI) buildHelpModal() *tview.Modal {
	text := `
┌─────────────────────────────────────────────────────────┐
│ RFS Server Supervisor - Hotkeys                         │
├─────────────────────────────────────────────────────────┤
│ F1       Show this help                                 │
│ F2       Create new profile                             │
│ F3       Status selected profile                        │
│ F4       Stop selected profile                          │
│ F5       Refresh all profiles                           │
│ Enter    Execute command                                │
│ Ctrl+C   Quit dashboard                                 │
└─────────────────────────────────────────────────────────┘
`
	return tview.NewModal().
		SetText(text).
		AddButtons([]string{"OK"}).
		SetDoneFunc(func(buttonIndex int, buttonLabel string) {
			ui.pages.SwitchToPage("main")
		})
}

func (ui *UI) setStatus(msg string) {
	if ui.statusBar != nil {
		ui.statusBar.SetText(fmt.Sprintf("[white]%s[white]", msg))
	}
}

func (ui *UI) setStatusWithColor(msg string, color tcell.Color) {
	if ui.statusBar != nil {
		ui.statusBar.SetText(fmt.Sprintf("[%s]%s[white]", color, msg))
	}
}

func (ui *UI) showCreateProfileModal() {
	nameInput := tview.NewInputField().SetLabel("Name: ").SetFieldWidth(30)
	binInput := tview.NewInputField().SetLabel("Binary: ").SetFieldWidth(30)

	modal := tview.NewModal().
		SetText("Create Profile\n\nName: \nBinary: ").
		AddButtons([]string{"OK", "Cancel"}).
		SetDoneFunc(func(buttonIndex int, buttonLabel string) {
			ui.pages.SwitchToPage("main")
		})

	ui.pages.AddPage("create_modal", modal, true, true)
}

func (ui *UI) executeProfileCmd(name, command string) {
	if ui.onCommand != nil {
		go ui.onCommand(fmt.Sprintf("%s %s", name, command))
	}
}

func (ui *UI) updateProfiles(profiles []ProfileRow, statuses map[string]*StatusInfo) {
	ui.profiles = profiles
	ui.statuses = statuses

	ui.profileList.Clear()
	for _, p := range profiles {
		statusText := p.State
		switch p.State {
		case "running":
			statusText = "[green]" + p.State + "[white]"
		case "stopped":
			statusText = "[gray]" + p.State + "[white]"
		case "error", "unhealthy":
			statusText = "[red]" + p.State + "[white]"
		case "starting":
			statusText = "[yellow]" + p.State + "[white]"
		}
		ui.profileList.AddItem(p.Name, statusText, 0, nil)
	}

	if len(profiles) == 0 {
		ui.detailsView.SetText("[red]No profiles configured.[white]\nPress F2 to create one.")
		return
	}

	if ui.profileList.GetCurrentItem() >= len(profiles) {
		ui.profileList.SetCurrentItem(0)
	}

	if len(profiles) > 0 {
		name := profiles[ui.profileList.GetCurrentItem()].Name
		ui.updateDetails(name)
	}
}

func (ui *UI) updateDetails(name string) {
	ui.profileList.SetCurrentItem(ui.findProfileIndex(name))

	info, ok := ui.statuses[name]
	if !ok {
		ui.detailsView.SetText(fmt.Sprintf("[red]No status for %s[white]", name))
		return
	}

	stateColor := "[white]"
	switch info.State {
	case "running":
		stateColor = "[green]"
	case "stopped":
		stateColor = "[gray]"
	case "error", "unhealthy":
		stateColor = "[red]"
	case "starting":
		stateColor = "[yellow]"
	}

	text := fmt.Sprintf(
		`[white]Profile:[white] %s
[white]State:[white] %s%s[white]
[white]PID:[white] %d
[white]Address:[white] %s
[white]Control Port:[white] %d
[white]Uptime:[white] %s
[white]Memory:[white] %.1f MB
[white]CPU:[white] %.1f%%
[white]Restarts:[white] %d
[white]Health:[white] %s`,
		name,
		stateColor, info.State,
		info.PID,
		info.Addr,
		info.ControlPort,
		info.Uptime,
		info.MemMB,
		info.CPUPercent,
		info.RestartCount,
		map[bool]string{true: "[green]OK[white]", false: "[red]FAIL[white]"}[info.HealthOK],
	)
	ui.detailsView.SetText(text)
}

func (ui *UI) findProfileIndex(name string) int {
	for i, p := range ui.profiles {
		if p.Name == name {
			return i
		}
	}
	return 0
}

func (ui *UI) addLog(text string) {
	ui.logs = append(ui.logs, text)
	if len(ui.logs) > 1000 {
		ui.logs = ui.logs[len(ui.logs)-1000:]
	}
	if ui.logView != nil {
		ui.logView.SetText(strings.Join(ui.logs, "\n"))
		ui.logView.ScrollToEnd()
	}
}

func (ui *UI) refreshLoop() {
	for ui.running {
		if ui.onRefresh != nil {
			go ui.onRefresh()
		}
		time.Sleep(2 * time.Second)
	}
}

func (ui *UI) SetProfileSelectCallback(fn func(name string)) {
	ui.onProfileSelect = fn
}

func (ui *UI) SetCommandCallback(fn func(command string)) {
	ui.onCommand = fn
}

func (ui *UI) SetCreateProfileCallback(fn func(name, binPath string)) {
	ui.onCreateProfile = fn
}

func (ui *UI) SetRefreshCallback(fn func()) {
	ui.onRefresh = fn
}

func (ui *UI) AddLog(text string) {
	ui.addLog(text)
}

func (ui *UI) SetStatus(msg string) {
	ui.setStatus(msg)
}

func (ui *UI) SetStatusWithColor(msg string, color tcell.Color) {
	ui.setStatusWithColor(msg, color)
}

func (ui *UI) UpdateProfiles(profiles []ProfileRow, statuses map[string]*StatusInfo) {
	ui.updateProfiles(profiles, statuses)
}

func (ui *UI) UpdateDetails(info *StatusInfo) {
	ui.updateDetails(info.Name)
}