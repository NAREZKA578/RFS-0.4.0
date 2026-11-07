// Package main implements the RFS server supervisor (надзиратель).
//
// The supervisor is a standalone Go program (outside the Rust workspace)
// that launches, stops and kills server processes; reads their output;
// measures memory and CPU; checks port availability; stores profiles;
// connects to the server's control port and forwards data/commands to
// the panel window.
//
// See `.DOC/new/server-interface-plan.md`, sections 3, 5 and 8.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"log"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"rfs-orchestrator/supervisor"
)

func main() {
	var (
		profileDir   = flag.String("profiles", "profiles", "directory for server profiles")
		serverBin    = flag.String("server", "", "default path to the RFS server binary")
		configFile   = flag.String("config", "", "default path to a server config file")
		verbose      = flag.Bool("v", false, "verbose logging")
		autoRestart  = flag.Bool("auto-restart", false, "auto-restart servers on unexpected exit")
		restartDelay = flag.Duration("restart-delay", 5*time.Second, "delay between auto-restart attempts")
		maxRestarts  = flag.Int("max-restarts", 3, "maximum auto-restart attempts (0 = unlimited)")
		ui           = flag.Bool("ui", false, "launch interactive terminal dashboard")
	)
	flag.Parse()

	absProfileDir, err := filepath.Abs(*profileDir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: resolving --profiles path: %v\n", err)
		os.Exit(2)
	}
	if err := os.MkdirAll(absProfileDir, 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "error: creating profiles directory: %v\n", err)
		os.Exit(2)
	}

	store, err := supervisor.NewStore(absProfileDir)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: creating profile store: %v\n", err)
		os.Exit(1)
	}

	sup, err := supervisor.New(*serverBin, absProfileDir, *verbose)
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: creating supervisor: %v\n", err)
		os.Exit(1)
	}
	defer sup.Close()

	// If UI mode, run the dashboard
	if *ui {
		runDashboard(sup, store)
		return
	}

	args := flag.Args()
	if len(args) == 0 {
		supervisor.PrintUsage(os.Stdout)
		os.Exit(2)
	}

	switch args[0] {
	case "start":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		p, err := store.Load(name)
		if err != nil {
			if *serverBin == "" {
				fmt.Fprintf(os.Stderr, "error: profile %q not found and --server not specified\n", name)
				os.Exit(2)
			}
			p = supervisor.DefaultProfile(name, *serverBin)
			p.Config = *configFile
			p.AutoRestart = *autoRestart
			p.MaxRestarts = *maxRestarts
			p.RestartDelay = *restartDelay
			if err := store.Save(p); err != nil {
				fmt.Fprintf(os.Stderr, "error: saving profile: %v\n", err)
				os.Exit(1)
			}
			fmt.Printf("created profile %q (token file: %s)\n", name, p.TokenFile)
		}

		p.TokenFile = supervisor.ResolveTokenPath(p.WorkDir, p.TokenFile)
		if err := store.Save(p); err != nil {
			fmt.Fprintf(os.Stderr, "error: saving profile: %v\n", err)
			os.Exit(1)
		}

		if err := sup.Start(name, &p); err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
		fmt.Printf("started profile %q\n", name)

	case "stop":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		if err := sup.Stop(name); err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
		fmt.Printf("stopped profile %q\n", name)

	case "stop-all":
		if err := sup.StopAll(); err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
		fmt.Println("stopped all profiles")

	case "kill":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		if err := sup.Kill(name); err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
		fmt.Printf("killed profile %q\n", name)

	case "kill-all":
		if err := sup.KillAll(); err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
		fmt.Println("killed all profiles")

	case "restart":
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
		if err := sup.Restart(name, &p); err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
		fmt.Printf("restarted profile %q\n", name)

	case "restart-all":
		profiles, err := store.List()
		if err != nil {
			fmt.Fprintf(os.Stderr, "error: listing profiles: %v\n", err)
			os.Exit(1)
		}
		for _, pname := range profiles {
			p, err := store.Load(pname)
			if err != nil {
				fmt.Fprintf(os.Stderr, "error: loading profile %q: %v\n", pname, err)
				continue
			}
			p.TokenFile = supervisor.ResolveTokenPath(p.WorkDir, p.TokenFile)
			if err := sup.Restart(pname, &p); err != nil {
				fmt.Fprintf(os.Stderr, "error restarting %q: %v\n", pname, err)
				continue
			}
			fmt.Printf("restarted profile %q\n", pname)
		}

	case "list":
		profiles, err := store.List()
		if err != nil {
			fmt.Fprintf(os.Stderr, "error: listing profiles: %v\n", err)
			os.Exit(1)
		}
		if len(profiles) == 0 {
			fmt.Println("no profiles")
			return
		}
		rows := make([]supervisor.ProfileRow, 0, len(profiles))
		running := sup.List()
		runningSet := make(map[string]bool, len(running))
		for _, n := range running {
			runningSet[n] = true
		}
		for _, pname := range profiles {
			p, err := store.Load(pname)
			if err != nil {
				continue
			}
			state := "stopped"
			if runningSet[pname] {
				state = "running"
			}
			rows = append(rows, supervisor.ProfileRow{
				Name:   pname,
				State:  state,
				PID:    0,
				Addr:   "-",
				Uptime: "-",
				MemMB:  0,
				CPU:    0,
				Tags:   p.Tags,
			})
		}
		supervisor.PrintProfiles(os.Stdout, rows)

	case "status":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		info, err := sup.Status(name)
		if err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
		supervisor.PrintStatus(os.Stdout, info.Name, info.State, info.PID, info.Addr, info.Uptime.String(), info.MemMB, info.CPUPercent)
		fmt.Printf("  restarts: %d | health fails: %d | health ok: %v | control port: %d\n",
			info.RestartCount, info.HealthFails, info.HealthOK, info.ControlPort)

	case "status-all":
		all := sup.StatusAll()
		if len(all) == 0 {
			fmt.Println("no running profiles")
			return
		}
		rows := make([]supervisor.ProfileRow, 0, len(all))
		for name, info := range all {
			rows = append(rows, supervisor.ProfileRow{
				Name:   name,
				State:  info.State,
				PID:    info.PID,
				Addr:   info.Addr,
				Uptime: info.Uptime.String(),
				MemMB:  info.MemMB,
				CPU:    info.CPUPercent,
			})
		}
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
		ok := supervisor.ClientHealthCheck(fmt.Sprintf("127.0.0.1:%d", p.ControlPort), p.TokenFile, 1, 2*time.Second)
		if ok {
			fmt.Printf("profile %q is healthy (control port %d)\n", name, p.ControlPort)
		} else {
			fmt.Printf("profile %q is UNHEALTHY (control port %d not responding)\n", name, p.ControlPort)
			os.Exit(1)
		}

	case "metrics":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		info, err := sup.Status(name)
		if err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
		supervisor.PrintMetrics(os.Stdout, info.Name, info.State, info.PID, info.Uptime.String(), info.MemMB, info.CPUPercent, info.RestartCount, info.HealthFails, info.HealthOK)

	case "watch":
		name := "default"
		if len(args) > 1 {
			name = args[1]
		}
		if err := sup.Watch(name, os.Stdout); err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}

	case "exec":
		if len(args) < 2 {
			fmt.Fprintf(os.Stderr, "error: exec [<name>] <command> [args...]\n")
			os.Exit(2)
		}
		name := "default"
		cmdStart := 1
		knownCommands := map[string]bool{
			"status": true, "perf": true, "net": true, "players": true,
			"config": true, "log": true, "watch": true, "audit": true,
			"say": true, "kick": true, "stop": true, "reload": true,
			"ban": true, "unban": true, "banlist": true,
			"end_match": true, "start_match": true, "restart_match": true,
		}
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
		results := sup.ExecAll("say", []string{text})
		for name, err := range results {
			if err != nil {
				fmt.Printf("  %s: error: %v\n", name, err)
			} else {
				fmt.Printf("  %s: queued\n", name)
			}
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
		} else if *serverBin != "" {
			binPath = *serverBin
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
		p.Config = *configFile
		p.AutoRestart = *autoRestart
		p.MaxRestarts = *maxRestarts
		p.RestartDelay = *restartDelay
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
		if err := store.Delete(name); err != nil {
			fmt.Fprintf(os.Stderr, "error: deleting profile: %v\n", err)
			os.Exit(1)
		}
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

func mustJSON(v interface{}) []byte {
	data, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		return []byte("{}")
	}
	return data
}

// runDashboard runs the full-screen terminal dashboard.
func runDashboard(sup *rfs_orchestrator.Supervisor, store *supervisor.Store) {
	ui := supervisor.NewUI()

	// Set up callbacks
	ui.SetProfileSelectCallback(func(name string) {
		info, err := sup.Status(name)
		if err == nil {
			ui.UpdateDetails(info)
		}
	})

	ui.SetCommandCallback(func(cmd string) {
		parts := strings.Fields(cmd)
		if len(parts) == 0 {
			return
		}
		profileName := parts[0]
		cmdArgs := parts[1:]
		if err := sup.Exec(profileName, cmdArgs[0], cmdArgs[1:]); err != nil {
			ui.SetStatusWithColor(fmt.Sprintf("Command failed: %v", err), supervisor.ColorRed)
		} else {
			ui.SetStatus(fmt.Sprintf("Command executed on %s", profileName))
		}
	})

	ui.SetCreateProfileCallback(func(name, binPath string) {
		p := supervisor.DefaultProfile(name, binPath)
		if err := store.Save(p); err != nil {
			ui.SetStatusWithColor(fmt.Sprintf("Create failed: %v", err), supervisor.ColorRed)
			return
		}
		ui.SetStatus(fmt.Sprintf("Created profile %q", name))
		go refreshDashboard(sup, store, ui)
	})

	ui.SetRefreshCallback(func() {
		go refreshDashboard(sup, store, ui)
	})

	// Initial refresh
	go refreshDashboard(sup, store, ui)

	// Run UI (blocks until exit)
	if err := ui.Run(); err != nil {
		fmt.Fprintf(os.Stderr, "error: ui: %v\n", err)
		os.Exit(1)
	}
}

// refreshDashboard refreshes the dashboard data.
func refreshDashboard(sup *rfs_orchestrator.Supervisor, store *supervisor.Store, ui *supervisor.UI) {
	profiles, err := store.List()
	if err != nil {
		ui.SetStatusWithColor(fmt.Sprintf("List failed: %v", err), supervisor.ColorRed)
		return
	}

	rows := make([]supervisor.ProfileRow, 0, len(profiles))
	statuses := make(map[string]*supervisor.StatusInfo)

	for _, pname := range profiles {
		p, err := store.Load(pname)
		if err != nil {
			continue
		}
		state := "stopped"
		if sup.IsRunning(pname) {
			state = "running"
		}
		info, err := sup.Status(pname)
		if err == nil {
			state = info.State
			statuses[pname] = info
		}
		rows = append(rows, supervisor.ProfileRow{
			Name:   pname,
			State:  state,
			PID:    0,
			Addr:   "-",
			Uptime: "-",
			MemMB:  0,
			CPU:    0,
			Tags:   p.Tags,
		})
		if info != nil {
			rows[len(rows)-1].PID = info.PID
			rows[len(rows)-1].Addr = info.Addr
			rows[len(rows)-1].Uptime = info.Uptime.String()
			rows[len(rows)-1].MemMB = info.MemMB
			rows[len(rows)-1].CPU = info.CPUPercent
		}
	}

	ui.UpdateProfiles(rows, statuses)
}