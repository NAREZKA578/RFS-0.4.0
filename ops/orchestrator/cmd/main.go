// Package main implements the RFS server supervisor (надзиратель).
//
// The supervisor is a standalone Go program (outside the Rust workspace)
// that launches, stops and kills server processes; reads their output;
// measures memory and CPU; checks port availability; stores profiles; and
// connects to the server's control port to forward commands and data.
//
// With no subcommand it opens the interactive terminal dashboard.
//
// See `.DOC/new/server-interface-plan.md`, sections 3, 5 and 8.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	supervisor "rfs-orchestrator"
)

// options carries the global flags into the dashboard and its callbacks.
type options struct {
	profileDir   string
	serverBin    string
	configFile   string
	verbose      bool
	autoRestart  bool
	maxRestarts  int
	restartDelay time.Duration
	ui           bool
}

func main() {
	opts := options{}
	flag.StringVar(&opts.profileDir, "profiles", "profiles", "directory for server profiles")
	flag.StringVar(&opts.serverBin, "server", "", "default path to the RFS server binary")
	flag.StringVar(&opts.configFile, "config", "", "default path to a server config file")
	flag.BoolVar(&opts.verbose, "v", false, "verbose logging")
	flag.BoolVar(&opts.autoRestart, "auto-restart", false, "auto-restart servers on unexpected exit")
	flag.DurationVar(&opts.restartDelay, "restart-delay", 5*time.Second, "delay between auto-restart attempts")
	flag.IntVar(&opts.maxRestarts, "max-restarts", 3, "maximum auto-restart attempts (0 = unlimited)")
	flag.BoolVar(&opts.ui, "ui", false, "launch the interactive terminal dashboard")
	flag.Parse()

	absProfileDir, err := filepath.Abs(opts.profileDir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: resolving --profiles path: %v\n", err)
		os.Exit(2)
	}
	if err := os.MkdirAll(absProfileDir, 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "error: creating profiles directory: %v\n", err)
		os.Exit(2)
	}
	opts.profileDir = absProfileDir

	if opts.serverBin == "" {
		opts.serverBin = detectServerBinary()
	}
	if opts.serverBin != "" {
		if abs, err := filepath.Abs(opts.serverBin); err == nil {
			opts.serverBin = abs
		}
	}

	store, err := supervisor.NewStore(absProfileDir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: creating profile store: %v\n", err)
		os.Exit(1)
	}

	sup, err := supervisor.New(opts.serverBin, absProfileDir, opts.verbose)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: creating supervisor: %v\n", err)
		os.Exit(1)
	}
	args := flag.Args()

	// No subcommand (or --ui) means the interactive dashboard. It is the
	// long-lived supervisor: the servers it starts are stopped when it exits.
	if opts.ui || len(args) == 0 {
		err := runDashboard(sup, store, opts)
		_ = sup.Close()
		if err != nil {
			fmt.Fprintf(os.Stderr, "error: ui: %v\n", err)
			os.Exit(1)
		}
		return
	}

	switch args[0] {
	case "start":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		info, err := startDetached(sup, store, name, opts)
		if err != nil {
			fatal(err)
		}
		fmt.Printf("started profile %q (pid %d)\n  log: %s\n", name, info.PID, info.LogFile)

	case "stop":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		if err := stopProfile(sup, store, name); err != nil {
			fatal(err)
		}
		fmt.Printf("stopped profile %q\n", name)

	case "stop-all":
		names, err := store.List()
		if err != nil {
			fatal(fmt.Errorf("listing profiles: %w", err))
		}
		for _, name := range names {
			if err := stopProfile(sup, store, name); err != nil {
				fmt.Fprintf(os.Stderr, "  %s: %v\n", name, err)
			}
		}
		fmt.Println("stopped all profiles")

	case "kill":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		if err := killProfile(sup, name); err != nil {
			fatal(err)
		}
		fmt.Printf("killed profile %q\n", name)

	case "kill-all":
		names, err := store.List()
		if err != nil {
			fatal(fmt.Errorf("listing profiles: %w", err))
		}
		for _, name := range names {
			if err := killProfile(sup, name); err != nil {
				fmt.Fprintf(os.Stderr, "  %s: %v\n", name, err)
			}
		}
		fmt.Println("killed all profiles")

	case "restart":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		_ = stopProfile(sup, store, name)
		info, err := startDetached(sup, store, name, opts)
		if err != nil {
			fatal(err)
		}
		fmt.Printf("restarted profile %q (pid %d)\n", name, info.PID)

	case "restart-all":
		profiles, err := store.List()
		if err != nil {
			fatal(fmt.Errorf("listing profiles: %w", err))
		}
		for _, name := range profiles {
			_ = stopProfile(sup, store, name)
			info, err := startDetached(sup, store, name, opts)
			if err != nil {
				fmt.Fprintf(os.Stderr, "error restarting %q: %v\n", name, err)
				continue
			}
			fmt.Printf("restarted profile %q (pid %d)\n", name, info.PID)
		}

	case "list":
		profiles, err := store.List()
		if err != nil {
			fatal(fmt.Errorf("listing profiles: %w", err))
		}
		if len(profiles) == 0 {
			fmt.Println("no profiles")
			return
		}
		sort.Strings(profiles)
		rows := make([]supervisor.ProfileRow, 0, len(profiles))
		for _, pname := range profiles {
			p, err := store.Load(pname)
			if err != nil {
				continue
			}
			info := sup.ProfileStatus(&p)
			rows = append(rows, supervisor.ProfileRow{
				Name:        pname,
				State:       info.State,
				PID:         info.PID,
				Addr:        orDash(info.Addr),
				Uptime:      uptimeOrDash(info.Uptime),
				MemMB:       info.MemMB,
				CPU:         info.CPUPercent,
				BinPath:     p.BinPath,
				Config:      p.Config,
				WorkDir:     p.WorkDir,
				ControlPort: p.ControlPort,
				Tags:        p.Tags,
			})
		}
		supervisor.PrintProfiles(os.Stdout, rows)

	case "status":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		p, err := store.Load(name)
		if err != nil {
			fatal(fmt.Errorf("profile %q not found: %w", name, err))
		}
		info := sup.ProfileStatus(&p)
		supervisor.PrintStatus(os.Stdout, info.Name, info.State, info.PID, orDash(info.Addr), uptimeOrDash(info.Uptime), info.MemMB, info.CPUPercent)
		fmt.Printf("  restarts: %d | health fails: %d | health ok: %v | control port: %d | match: %s | players: %d\n",
			info.RestartCount, info.HealthFails, info.HealthOK, info.ControlPort, info.MatchState, info.Players)

	case "status-all":
		profiles, err := store.List()
		if err != nil {
			fatal(fmt.Errorf("listing profiles: %w", err))
		}
		rows := make([]supervisor.ProfileRow, 0, len(profiles))
		for _, name := range profiles {
			p, err := store.Load(name)
			if err != nil {
				continue
			}
			info := sup.ProfileStatus(&p)
			if info.State == "stopped" {
				continue
			}
			rows = append(rows, supervisor.ProfileRow{
				Name:        name,
				State:       info.State,
				PID:         info.PID,
				Addr:        orDash(info.Addr),
				Uptime:      uptimeOrDash(info.Uptime),
				MemMB:       info.MemMB,
				CPU:         info.CPUPercent,
				ControlPort: info.ControlPort,
			})
		}
		if len(rows) == 0 {
			fmt.Println("no running profiles")
			return
		}
		sort.Slice(rows, func(i, j int) bool { return rows[i].Name < rows[j].Name })
		supervisor.PrintProfiles(os.Stdout, rows)

	case "health":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		p, err := store.Load(name)
		if err != nil {
			fmt.Fprintf(os.Stderr, "error: profile %q not found: %v\n", name, err)
			os.Exit(1)
		}
		p.TokenFile = supervisor.ResolveTokenPath(p.WorkDir, p.TokenFile)
		token, err := supervisor.ReadToken(p.TokenFile)
		if err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
		addr := fmt.Sprintf("127.0.0.1:%d", p.ControlPort)
		ok := supervisor.ClientHealthCheck(addr, token, 1, 2*time.Second)
		supervisor.PrintHealth(os.Stdout, name, ok, addr)
		if !ok {
			os.Exit(1)
		}

	case "metrics":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		p, err := store.Load(name)
		if err != nil {
			fatal(fmt.Errorf("profile %q not found: %w", name, err))
		}
		info := sup.ProfileStatus(&p)
		supervisor.PrintMetrics(os.Stdout, info.Name, info.State, info.PID, uptimeOrDash(info.Uptime), info.MemMB, info.CPUPercent, info.RestartCount, info.HealthFails, info.HealthOK)

	case "watch":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		if sup.IsRunning(name) {
			if err := sup.Watch(name, os.Stdout); err != nil {
				fatal(err)
			}
			return
		}
		if err := sup.TailLog(name, os.Stdout, true); err != nil {
			fatal(err)
		}

	case "exec":
		if len(args) < 2 {
			fmt.Fprintf(os.Stderr, "error: exec [<name>] <command> [args...]\n")
			os.Exit(2)
		}
		name := "default"
		cmdStart := 1
		if len(args) > 2 && !knownCommands[args[1]] && !strings.HasPrefix(args[1], "-") {
			name = args[1]
			cmdStart = 2
		}
		cmdArgs := args[cmdStart:]
		if len(cmdArgs) == 0 {
			fmt.Fprintf(os.Stderr, "error: no command specified\n")
			os.Exit(2)
		}
		if err := sup.Exec(name, cmdArgs[0], cmdArgs[1:]); err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}

	case "say-all":
		if len(args) < 2 {
			fmt.Fprintf(os.Stderr, "error: say-all <text>\n")
			os.Exit(2)
		}
		text := strings.Join(args[1:], " ")
		names, err := store.List()
		if err != nil {
			fatal(fmt.Errorf("listing profiles: %w", err))
		}
		sort.Strings(names)
		any := false
		for _, name := range names {
			if _, err := sup.ExecCommand(name, "say", []string{text}); err != nil {
				continue
			}
			fmt.Printf("  %s: queued\n", name)
			any = true
		}
		if !any {
			fmt.Println("  no running profiles")
		}

	case "profile-create":
		if len(args) < 2 {
			fmt.Fprintf(os.Stderr, "error: profile-create <name> [bin-path]\n")
			os.Exit(2)
		}
		name := args[1]
		binPath := ""
		if len(args) > 2 {
			binPath = args[2]
		} else if opts.serverBin != "" {
			binPath = opts.serverBin
		}
		if binPath == "" {
			fmt.Fprintf(os.Stderr, "error: no server binary specified (arg or --server)\n")
			os.Exit(2)
		}
		if store.Exists(name) {
			fmt.Fprintf(os.Stderr, "error: profile %q already exists\n", name)
			os.Exit(2)
		}
		p := supervisor.DefaultProfile(name, binPath)
		p.Config = opts.configFile
		p.AutoRestart = opts.autoRestart
		p.MaxRestarts = opts.maxRestarts
		p.RestartDelay = opts.restartDelay
		if err := prepareProfile(&p); err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
		if err := store.Save(p); err != nil {
			fmt.Fprintf(os.Stderr, "error: saving profile: %v\n", err)
			os.Exit(1)
		}
		fmt.Printf("created profile %q\n", name)

	case "profile-delete":
		if len(args) < 2 {
			fmt.Fprintf(os.Stderr, "error: profile-delete <name>\n")
			os.Exit(2)
		}
		name := args[1]
		if sup.IsRunning(name) {
			fmt.Fprintf(os.Stderr, "error: profile %q is running; stop it first\n", name)
			os.Exit(2)
		}
		if _, err := sup.ReadPID(name); err == nil {
			fmt.Fprintf(os.Stderr, "error: profile %q is running; stop it first\n", name)
			os.Exit(2)
		}
		if err := store.Delete(name); err != nil {
			fmt.Fprintf(os.Stderr, "error: deleting profile: %v\n", err)
			os.Exit(1)
		}
		_ = os.Remove(sup.PIDFile(name))
		_ = os.Remove(sup.LogFile(name))
		fmt.Printf("deleted profile %q\n", name)

	case "profile-show":
		if len(args) < 2 {
			fmt.Fprintf(os.Stderr, "error: profile-show <name>\n")
			os.Exit(2)
		}
		name := args[1]
		p, err := store.Load(name)
		if err != nil {
			fmt.Fprintf(os.Stderr, "error: profile %q not found: %v\n", name, err)
			os.Exit(1)
		}
		supervisor.PrintJSON(os.Stdout, string(mustJSON(p)))

	case "config-set":
		if len(args) < 4 {
			fmt.Fprintf(os.Stderr, "error: config-set <name> <key> <value>\n")
			os.Exit(2)
		}
		name := args[1]
		key := args[2]
		value := args[3]
		p, err := store.Load(name)
		if err != nil {
			fmt.Fprintf(os.Stderr, "error: profile %q not found: %v\n", name, err)
			os.Exit(1)
		}
		switch key {
		case "config":
			p.Config = value
		case "control_port":
			var port uint16
			fmt.Sscanf(value, "%d", &port)
			p.ControlPort = port
		case "token_file":
			p.TokenFile = value
		case "auto_restart":
			p.AutoRestart = value == "true" || value == "1"
		case "max_restarts":
			fmt.Sscanf(value, "%d", &p.MaxRestarts)
		case "restart_delay":
			d, err := time.ParseDuration(value)
			if err != nil {
				fmt.Fprintf(os.Stderr, "error: invalid duration %q: %v\n", value, err)
				os.Exit(2)
			}
			p.RestartDelay = d
		case "health_check_interval":
			d, err := time.ParseDuration(value)
			if err != nil {
				fmt.Fprintf(os.Stderr, "error: invalid duration %q: %v\n", value, err)
				os.Exit(2)
			}
			p.HealthCheckInterval = d
		case "work_dir":
			p.WorkDir = value
		case "env":
			if p.Env == nil {
				p.Env = make(map[string]string)
			}
			parts := strings.SplitN(value, "=", 2)
			if len(parts) == 2 {
				p.Env[parts[0]] = parts[1]
			}
		case "tag":
			p.Tags = append(p.Tags, value)
		default:
			fmt.Fprintf(os.Stderr, "error: unknown config key %q\n", key)
			os.Exit(2)
		}
		if err := store.Save(p); err != nil {
			fmt.Fprintf(os.Stderr, "error: saving profile: %v\n", err)
			os.Exit(1)
		}
		fmt.Printf("set %s=%s on profile %q\n", key, value, name)

	default:
		fmt.Fprintf(os.Stderr, "error: unknown command %q\n", args[0])
		supervisor.PrintUsage(os.Stderr)
		os.Exit(2)
	}
}

// knownCommands is the set of control-port commands understood by the server.
var knownCommands = map[string]bool{
	"status": true, "perf": true, "net": true, "players": true,
	"config": true, "log": true, "watch": true, "audit": true,
	"say": true, "kick": true, "stop": true, "reload": true,
	"ban": true, "unban": true, "banlist": true,
	"end_match": true, "start_match": true, "restart_match": true,
}

// detectServerBinary looks for the release server next to the supervisor.
func detectServerBinary() string {
	candidates := []string{
		filepath.Join("..", "..", "server_src", "target", "release", "server.exe"),
		filepath.Join("..", "server_src", "target", "release", "server.exe"),
		filepath.Join("server_src", "target", "release", "server.exe"),
		filepath.Join("..", "..", "server_src", "target", "release", "server"),
		filepath.Join("..", "server_src", "target", "release", "server"),
		filepath.Join("server_src", "target", "release", "server"),
		"server.exe",
		"server",
	}
	for _, candidate := range candidates {
		if st, err := os.Stat(candidate); err == nil && !st.IsDir() {
			if abs, err := filepath.Abs(candidate); err == nil {
				return abs
			}
			return candidate
		}
	}
	return ""
}

// prepareProfile fills in derived defaults: absolute binary path, a work
// directory next to the binary, the control port and the token file path.
func prepareProfile(p *supervisor.Profile) error {
	if p.BinPath == "" {
		return fmt.Errorf("no server binary specified for profile %q", p.Name)
	}
	abs, err := filepath.Abs(p.BinPath)
	if err != nil {
		return fmt.Errorf("resolving server binary: %w", err)
	}
	st, err := os.Stat(abs)
	if err != nil {
		return fmt.Errorf("server binary not found: %w", err)
	}
	if st.IsDir() {
		return fmt.Errorf("server binary is a directory: %s", abs)
	}
	p.BinPath = abs
	if p.WorkDir == "" {
		p.WorkDir = filepath.Dir(abs)
	}
	if p.ControlPort == 0 {
		p.ControlPort = 7778
	}
	if p.TokenFile == "" {
		p.TokenFile = "control_token.json"
	}
	p.TokenFile = supervisor.ResolveTokenPath(p.WorkDir, p.TokenFile)
	return nil
}

// startProfile starts a stored profile, creating it from the global flags on
// first use.
func startProfile(sup *supervisor.Supervisor, store *supervisor.Store, name string, opts options) error {
	if sup.IsRunning(name) {
		return fmt.Errorf("profile %q is already running", name)
	}

	p, err := store.Load(name)
	if err != nil {
		if opts.serverBin == "" {
			return fmt.Errorf("profile %q not found and no server binary configured (--server)", name)
		}
		p = supervisor.DefaultProfile(name, opts.serverBin)
		p.Config = opts.configFile
		p.AutoRestart = opts.autoRestart
		if opts.maxRestarts > 0 {
			p.MaxRestarts = opts.maxRestarts
		}
		if opts.restartDelay > 0 {
			p.RestartDelay = opts.restartDelay
		}
	}

	if err := prepareProfile(&p); err != nil {
		return err
	}
	if err := store.Save(p); err != nil {
		return fmt.Errorf("saving profile: %w", err)
	}
	return sup.Start(name, &p)
}

// runDashboard wires the supervisor to the interactive dashboard.
func runDashboard(sup *supervisor.Supervisor, store *supervisor.Store, opts options) error {
	ui := supervisor.NewUI()
	ui.SetDefaults(opts.serverBin, opts.configFile)

	// Server stdout/stderr and supervisor events go to the live log pane.
	sup.SetOutput(ui.LogWriter())

	// First run: if there are no profiles and we know a server binary, create
	// "default" automatically instead of showing an empty list.
	if names, err := store.List(); err == nil && len(names) == 0 && opts.serverBin != "" {
		p := supervisor.DefaultProfile("default", opts.serverBin)
		p.Config = opts.configFile
		p.AutoRestart = opts.autoRestart
		if opts.maxRestarts > 0 {
			p.MaxRestarts = opts.maxRestarts
		}
		if opts.restartDelay > 0 {
			p.RestartDelay = opts.restartDelay
		}
		if err := prepareProfile(&p); err == nil {
			if err := store.Save(p); err == nil {
				ui.AddLog("[supervisor] created profile \"default\" from the detected server binary")
			}
		}
	} else if len(names) == 0 {
		ui.AddLog("[supervisor] no profiles yet — press F2 to create one")
	}

	ui.SetActionCallback(func(action, name string) (string, error) {
		switch action {
		case "start":
			if err := startProfile(sup, store, name, opts); err != nil {
				return "", err
			}
			return fmt.Sprintf("started profile %q", name), nil
		case "stop":
			if err := sup.Stop(name); err != nil {
				return "", err
			}
			return fmt.Sprintf("stopped profile %q", name), nil
		case "kill":
			if err := sup.Kill(name); err != nil {
				return "", err
			}
			return fmt.Sprintf("killed profile %q", name), nil
		case "restart":
			p, err := store.Load(name)
			if err != nil {
				return "", err
			}
			if err := prepareProfile(&p); err != nil {
				return "", err
			}
			if err := sup.Restart(name, &p); err != nil {
				return "", err
			}
			return fmt.Sprintf("restarted profile %q", name), nil
		}
		return "", fmt.Errorf("unknown action %q", action)
	})

	ui.SetCreateProfileCallback(func(p supervisor.Profile) error {
		if store.Exists(p.Name) {
			return fmt.Errorf("profile %q already exists", p.Name)
		}
		if err := prepareProfile(&p); err != nil {
			return err
		}
		return store.Save(p)
	})

	ui.SetDeleteProfileCallback(func(name string) error {
		if sup.IsRunning(name) {
			return fmt.Errorf("profile %q is running; stop it first", name)
		}
		return store.Delete(name)
	})

	ui.SetCommandCallback(func(line string) (string, error) {
		return runUICommand(ui, sup, store, opts, line)
	})

	ui.SetRefreshCallback(func() {
		refreshDashboard(sup, store, ui)
	})

	// Give the tview loop a moment to start, then draw the first snapshot.
	go func() {
		time.Sleep(300 * time.Millisecond)
		refreshDashboard(sup, store, ui)
	}()

	return ui.Run()
}

// runUICommand handles one line typed into the dashboard command bar.
func runUICommand(ui *supervisor.UI, sup *supervisor.Supervisor, store *supervisor.Store, opts options, line string) (string, error) {
	fields := strings.Fields(line)
	if len(fields) == 0 {
		return "", nil
	}

	cmd := strings.ToLower(fields[0])
	args := fields[1:]

	// @profile <command> targets another profile.
	if strings.HasPrefix(cmd, "@") {
		target := strings.TrimPrefix(cmd, "@")
		if len(args) == 0 {
			return "", fmt.Errorf("usage: @<profile> <command> [args...]")
		}
		cmd = strings.ToLower(args[0])
		args = args[1:]
		return runControlCommand(sup, target, cmd, args)
	}

	switch cmd {
	case "help", "?":
		return "F1 help · F2 new · F3 start · F4 stop · F6 command · F7 restart · F8 kill · F9 delete · F5 refresh", nil

	case "profiles", "list":
		names, err := store.List()
		if err != nil {
			return "", err
		}
		sort.Strings(names)
		if len(names) == 0 {
			return "no profiles", nil
		}
		return strings.Join(names, ", "), nil

	case "start":
		name := profileOrSelected(ui, args)
		if name == "" {
			return "", fmt.Errorf("no profile selected")
		}
		if err := startProfile(sup, store, name, opts); err != nil {
			return "", err
		}
		return fmt.Sprintf("started profile %q", name), nil

	case "stop":
		name := profileOrSelected(ui, args)
		if err := sup.Stop(name); err != nil {
			return "", err
		}
		return fmt.Sprintf("stopped profile %q", name), nil

	case "kill":
		name := profileOrSelected(ui, args)
		if err := sup.Kill(name); err != nil {
			return "", err
		}
		return fmt.Sprintf("killed profile %q", name), nil

	case "restart":
		name := profileOrSelected(ui, args)
		p, err := store.Load(name)
		if err != nil {
			return "", err
		}
		if err := prepareProfile(&p); err != nil {
			return "", err
		}
		if err := sup.Restart(name, &p); err != nil {
			return "", err
		}
		return fmt.Sprintf("restarted profile %q", name), nil

	case "delete":
		name := profileOrSelected(ui, args)
		if sup.IsRunning(name) {
			return "", fmt.Errorf("profile %q is running; stop it first", name)
		}
		if err := store.Delete(name); err != nil {
			return "", err
		}
		return fmt.Sprintf("deleted profile %q", name), nil

	case "say-all":
		if len(args) == 0 {
			return "", fmt.Errorf("usage: say-all <text>")
		}
		results := sup.ExecAll("say", args)
		if len(results) == 0 {
			return "", fmt.Errorf("no running profiles")
		}
		var b strings.Builder
		names := make([]string, 0, len(results))
		for name := range results {
			names = append(names, name)
		}
		sort.Strings(names)
		for _, name := range names {
			if err := results[name]; err != nil {
				fmt.Fprintf(&b, "%s: %v\n", name, err)
			} else {
				fmt.Fprintf(&b, "%s: queued\n", name)
			}
		}
		return strings.TrimRight(b.String(), "\n"), nil

	case "status-all":
		all := sup.StatusAll()
		if len(all) == 0 {
			return "no running profiles", nil
		}
		names := make([]string, 0, len(all))
		for name := range all {
			names = append(names, name)
		}
		sort.Strings(names)
		var b strings.Builder
		for _, name := range names {
			info := all[name]
			fmt.Fprintf(&b, "%s: %s  pid=%d  match=%s  players=%d  mem=%.1fMB\n",
				name, info.State, info.PID, info.MatchState, info.Players, info.MemMB)
		}
		return strings.TrimRight(b.String(), "\n"), nil
	}

	name := ui.SelectedProfile()
	if name == "" {
		return "", fmt.Errorf("no profile selected; use @<profile> %s", cmd)
	}
	return runControlCommand(sup, name, cmd, args)
}

func profileOrSelected(ui *supervisor.UI, args []string) string {
	if len(args) > 0 {
		return args[0]
	}
	return ui.SelectedProfile()
}

func runControlCommand(sup *supervisor.Supervisor, name, command string, args []string) (string, error) {
	if command == "watch" {
		return "", fmt.Errorf("watch streams events and is not available in the dashboard")
	}
	if !knownCommands[command] {
		return "", fmt.Errorf("unknown command %q (try help)", command)
	}
	data, err := sup.ExecCommand(name, command, args)
	if err != nil {
		return "", err
	}
	out, err := json.Marshal(data)
	if err != nil {
		return "", err
	}
	return string(out), nil
}

// refreshDashboard reloads profiles and their live status into the UI.
func refreshDashboard(sup *supervisor.Supervisor, store *supervisor.Store, ui *supervisor.UI) {
	names, err := store.List()
	if err != nil {
		ui.SetError(fmt.Sprintf("list profiles: %v", err))
		return
	}
	sort.Strings(names)

	rows := make([]supervisor.ProfileRow, 0, len(names))
	statuses := make(map[string]*supervisor.StatusInfo, len(names))

	for _, name := range names {
		p, err := store.Load(name)
		if err != nil {
			continue
		}
		row := supervisor.ProfileRow{
			Name:        name,
			State:       "stopped",
			Addr:        "-",
			Uptime:      "-",
			BinPath:     p.BinPath,
			Config:      p.Config,
			WorkDir:     p.WorkDir,
			ControlPort: p.ControlPort,
			Tags:        p.Tags,
		}
		if info, err := sup.Status(name); err == nil {
			row.State = info.State
			row.PID = info.PID
			row.Addr = orDash(info.Addr)
			row.Uptime = info.Uptime.Round(time.Second).String()
			row.MemMB = info.MemMB
			row.CPU = info.CPUPercent
			st := info
			statuses[name] = &st
		} else if sup.IsRunning(name) {
			row.State = "starting"
		}
		rows = append(rows, row)
	}

	ui.UpdateProfiles(rows, statuses)
}

func orDash(s string) string {
	if s == "" {
		return "-"
	}
	return s
}

func mustJSON(v interface{}) []byte {
	data, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		return []byte("{}")
	}
	return data
}
