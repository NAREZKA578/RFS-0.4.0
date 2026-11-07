# RFS Server Supervisor (надзиратель)

Standalone Go program (outside the Rust workspace) that manages RFS server
processes. Part of the server management interface per
`.DOC/new/server-interface-plan.md` (sections 3, 5, 8).

## What it does

- **Launch / stop / kill / restart** server profiles (each profile = server binary +
  config + arguments + control port).
- **Auto-restart** on unexpected exit (`--auto-restart`, `--restart-delay`, `--max-restarts`).
- **Health-check** via control port `status` command; marks unhealthy instances.
- **Monitor** each process: PID, state, uptime, memory, CPU, restart count.
- **Read** server stdout/stderr and stream it on demand (`watch`).
- **Connect** to the server's control port (`127.0.0.1:<control_port>`) and
  forward commands (`status`, `perf`, `net`, `players`, `config`, `log`,
  `watch`, `say`, `kick`, `stop`, `reload`, `ban`, `unban`, `banlist`,
  `end_match`, `start_match`, `restart_match`).
- **Mass operations**: `stop-all`, `kill-all`, `restart-all`, `say-all`,
  `status-all`, `exec-all` via `exec`.
- **Store profiles** as JSON files in a profiles directory (survives
  supervisor restarts).
- **Configure profiles** at runtime: `config-set`, `profile-create`,
  `profile-delete`, `profile-show`.
- **Text UI** for console operators (`list`, `status`, `metrics`, `health`,
  `exec`).

## Project layout

```
ops/orchestrator/
├── go.mod              # Go module (no external deps — builds with plain go)
├── main.go             # CLI entry point (all commands)
├── supervisor.go       # Process management, auto-restart, health-check,
│                       # control-port client, metrics
├── client.go           # Control-port protocol client (token hello + JSON commands,
│                       # per-command helpers: Status, Perf, Net, Players, ...)
├── profile.go          # Profile store (JSON profiles on disk, DefaultProfile)
├── metrics.go          # OS-specific process memory/CPU reading
├── ui.go               # Terminal UI (tables, status, metrics, health, boxes)
├── Makefile            # Build and test targets
├── build.ps1           # Windows PowerShell build script
├── README.md           # This file
├── supervisor_test.go  # Tests: New, IsRunning, Status, StatusAll, ExecAll,
│                       # ClientHealthCheck
├── client_test.go      # Tests: hello, command, wrong token, unknown command
├── profile_test.go     # Tests: save/load/list/delete/exists/DefaultProfile
└── ui_test.go          # Tests: profile table, status, metrics, health, box
```

## Building and running

Requires Go 1.22+. No external dependencies.

```bash
cd ops/orchestrator
go build -o rfs-orchestrator .
go test ./...

# Basic usage
./rfs-orchestrator --server /path/to/rfs-server start main
./rfs-orchestrator status main
./rfs-orchestrator exec main status
./rfs-orchestrator exec main say "server restarting soon"
./rfs-orchestrator stop main
```

## Full command reference

| Command | Description |
|---------|-------------|
| `start [name]` | Start a profile (default: "default") |
| `stop [name]` | Gracefully stop a profile |
| `stop-all` | Stop all running profiles |
| `kill [name]` | Kill a profile immediately |
| `kill-all` | Kill all profiles immediately |
| `restart [name]` | Restart a profile |
| `restart-all` | Restart all profiles |
| `list` | List all profiles |
| `status [name]` | Detailed status for a profile |
| `status-all` | Status for all running profiles |
| `health [name]` | Health-check via control port |
| `metrics [name]` | Memory/CPU/restart metrics |
| `watch [name]` | Stream server logs to stdout |
| `exec [<name>] <cmd> [args]` | Send a control command to a profile |
| `say-all <text>` | Announcement to all running servers |
| `profile-create <name> [bin]` | Create a new profile |
| `profile-delete <name>` | Delete a profile (must be stopped) |
| `profile-show <name>` | Show profile JSON |
| `config-set <name> <key> <value>` | Set a profile config key |

### `config-set` keys

| Key | Example | Description |
|-----|---------|-------------|
| `config` | `config-set main server_config.json` | Server config file path |
| `control_port` | `config-set main control_port 7778` | Control port number |
| `token_file` | `config-set main token_file control_token.json` | Token file name |
| `auto_restart` | `config-set main auto_restart true` | Auto-restart on exit |
| `max_restarts` | `config-set main max_restarts 5` | Max restart attempts |
| `restart_delay` | `config-set main restart_delay 10s` | Delay between restarts |
| `health_check_interval` | `config-set main health_check_interval 15s` | Health-check period |
| `work_dir` | `config-set main /opt/rfs` | Server working directory |
| `env KEY=VALUE` | `config-set main env RUST_LOG=info` | Environment variable |
| `tag NAME` | `config-set main tag prod` | Add a tag for grouping |

## Profile JSON

Profiles are stored as `<name>.json` in the `--profiles` directory:

```json
{
  "name": "main",
  "bin_path": "/opt/rfs/server",
  "config": "server_config.json",
  "args": ["--ships", "15"],
  "work_dir": "/opt/rfs",
  "control_port": 7778,
  "token_file": "control_token.json",
  "auto_restart": true,
  "max_restarts": 3,
  "restart_delay": "5s",
  "health_check_interval": "10s",
  "health_check_failures": 3,
  "env": { "RUST_LOG": "info" },
  "tags": ["prod"],
  "created_at": "2026-11-07T10:00:00Z",
  "updated_at": "2026-11-07T10:05:00Z"
}
```

## Control port commands available via `exec`

The supervisor forwards any control-port command to the server:

`status`, `perf`, `net`, `players`, `config`, `log`, `watch`, `audit`,
`say`, `kick`, `stop`, `reload`, `ban`, `unban`, `banlist`,
`end_match`, `start_match`, `restart_match`.

## Running tests

```bash
cd ops/orchestrator
go test -v ./...
```

## Building for Windows (PowerShell)

```powershell
.\build.ps1 build
.\build.ps1 test
.\build.ps1 run C:\path\to\rfs-server.exe
```

## Relation to the panel

The supervisor is the middle tier between the panel window (Rust, `crates/panel`)
and the RFS server processes:

```
Panel window ──► Supervisor (Go) ──► RFS server: control port
```

The supervisor exposes every server command over the control port; the panel
connects to it the same way `ctl` does. The supervisor itself can be driven
from the command line today, before the panel UI exists.

## Supervisor guide

See `ops/SUPERVISOR_GUIDE.md` for a language-agnostic guide to building your
own supervisor.
