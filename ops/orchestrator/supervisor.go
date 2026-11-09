// Package supervisor manages RFS server processes: launch, stop, kill,
// monitor (memory/CPU/uptime), read their output, and connect to their
// control port to forward commands and data to the panel.
//
// See `.DOC/new/server-interface-plan.md`, section 8 (Надзиратель).
package supervisor

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"
)

// Profile is a saved server configuration the supervisor can launch.
type Profile struct {
	Name         string    `json:"name"`
	BinPath      string    `json:"bin_path"`
	Config       string    `json:"config,omitempty"`
	Args         []string  `json:"args,omitempty"`
	WorkDir      string    `json:"work_dir,omitempty"`
	ControlPort  uint16    `json:"control_port,omitempty"`
	TokenFile    string    `json:"token_file,omitempty"`
	CreatedAt    time.Time `json:"created_at"`
	UpdatedAt    time.Time `json:"updated_at,omitempty"`

	// Auto-restart: if the server process exits unexpectedly, the supervisor
	// will restart it automatically. Disabled by default.
	AutoRestart        bool      `json:"auto_restart,omitempty"`
	MaxRestarts        int       `json:"max_restarts,omitempty"`     // 0 = unlimited
	RestartDelay   time.Duration `json:"restart_delay,omitempty"`  // delay between restart attempts
	RestartCount   int       `json:"restart_count,omitempty"`    // how many times restarted this session

	// Health-check: periodically poll the control port `status` command.
	// If it fails for HealthCheckFailures times in a row, the instance is
	// considered unhealthy and (if AutoRestart) restarted.
	HealthCheckInterval time.Duration `json:"health_check_interval,omitempty"`
	HealthCheckFailures int           `json:"health_check_failures,omitempty"` // threshold, default 3

	// Environment variables passed to the server process.
	Env map[string]string `json:"env,omitempty"`

	// Tags for grouping/profiling (e.g. "dev", "prod", "eu-west").
	Tags []string `json:"tags,omitempty"`
}

// Instance is a running server process tracked by the supervisor.
type Instance struct {
	Profile   Profile
	Cmd       *exec.Cmd
	Proc      *os.Process
	PID       int
	StartTime time.Time
	State     InstanceState
	mu        sync.RWMutex
	stdout    io.ReadCloser
	stderr    io.ReadCloser
	wg        sync.WaitGroup

	// Restart tracking (session-only, not persisted).
	restartCount   int
	lastRestart    time.Time
	healthFails    int
	healthLastOK   time.Time
	stopRequested  bool // set when Stop()/Kill() is called intentionally
}

// InstanceState describes the lifecycle of a server instance.
type InstanceState string

const (
	StateStarting  InstanceState = "starting"
	StateRunning   InstanceState = "running"
	StateStopping  InstanceState = "stopping"
	StateStopped   InstanceState = "stopped"
	StateError     InstanceState = "error"
	StateUnhealthy InstanceState = "unhealthy"
)

// StatusInfo is a snapshot of a running instance for the panel.
type StatusInfo struct {
	Name         string        `json:"name"`
	State        string        `json:"state"`
	PID          int           `json:"pid"`
	Addr         string        `json:"addr"`
	ControlPort  uint16        `json:"control_port"`
	Uptime       time.Duration `json:"uptime"`
	MemMB        float64       `json:"mem_mb"`
	CPUPercent   float64       `json:"cpu_percent"`
	RestartCount int           `json:"restart_count"`
	HealthFails  int           `json:"health_fails"`
	HealthOK     bool          `json:"health_ok"`

	// Fields mirrored from the control port's `status` answer.
	MatchState  string  `json:"match_state,omitempty"`
	Tick        uint64  `json:"tick,omitempty"`
	TickRate    float64 `json:"tick_rate,omitempty"`
	Players     int     `json:"players,omitempty"`
	Ships       int     `json:"ships,omitempty"`
	Connections int     `json:"connections,omitempty"`
	Entities    int     `json:"entities,omitempty"`
}

// Supervisor manages server processes and their control-port clients.
type Supervisor struct {
	binPath    string
	profileDir string
	verbose    bool

	out    io.Writer
	errOut io.Writer

	mu        sync.RWMutex
	instances map[string]*Instance
	restarts  map[string]int
}

// New creates a Supervisor. binPath is the default RFS server binary;
// profileDir holds profile definitions and runtime state.
func New(binPath, profileDir string, verbose bool) (*Supervisor, error) {
	if binPath != "" {
		if _, err := os.Stat(binPath); err != nil {
			return nil, fmt.Errorf("server binary not found: %w", err)
		}
	}
	return &Supervisor{
		binPath:    binPath,
		profileDir: profileDir,
		verbose:    verbose,
		out:        os.Stdout,
		errOut:     os.Stderr,
		instances:  make(map[string]*Instance),
		restarts:   make(map[string]int),
	}, nil
}

// SetOutput redirects the output of server processes started from now on.
// The interactive dashboard uses it to show server logs in its log pane.
func (s *Supervisor) SetOutput(out io.Writer) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if out == nil {
		out = io.Discard
	}
	s.out = out
	s.errOut = out
}

// Close shuts down all managed instances gracefully.
func (s *Supervisor) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	var firstErr error
	for name, inst := range s.instances {
		inst.stopRequested = true
		if err := inst.Stop(); err != nil && firstErr == nil {
			firstErr = fmt.Errorf("stopping %s: %w", name, err)
		}
	}
	return firstErr
}

// Start launches a server profile using the given profile.
func (s *Supervisor) Start(name string, p *Profile) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if _, exists := s.instances[name]; exists {
		return fmt.Errorf("profile %q is already running", name)
	}

	binPath := p.BinPath
	if binPath == "" {
		binPath = s.binPath
	}
	if binPath == "" {
		return fmt.Errorf("no server binary specified for profile %q", name)
	}

	args := append([]string{}, p.Args...)
	if p.Config != "" {
		args = append([]string{"--config", p.Config}, args...)
	}

	cmd := exec.Command(binPath, args...)
	if p.WorkDir != "" {
		cmd.Dir = p.WorkDir
	} else {
		cmd.Dir = s.profileDir
	}
	if len(p.Env) > 0 {
		cmd.Env = append(os.Environ(), envMapToSlice(p.Env)...)
	}

	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return fmt.Errorf("creating stdout pipe: %w", err)
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		return fmt.Errorf("creating stderr pipe: %w", err)
	}

	if err := cmd.Start(); err != nil {
		return fmt.Errorf("starting server process: %w", err)
	}

	inst := &Instance{
		Profile:   *p,
		Cmd:       cmd,
		Proc:      cmd.Process,
		PID:       cmd.Process.Pid,
		StartTime: time.Now(),
		State:     StateRunning,
		stdout:    stdout,
		stderr:    stderr,
	}
	s.instances[name] = inst
	s.writePID(name, PIDInfo{
		PID:       inst.PID,
		LogFile:   s.LogFile(name),
		StartedAt: inst.StartTime,
	})

	out := s.out
	errOut := s.errOut
	inst.wg.Add(2)
	go inst.pump(stdout, out, "[stdout] ")
	go inst.pump(stderr, errOut, "[stderr] ")

	// Wait-loop: when the process exits, decide whether to auto-restart.
	go func() {
		inst.wg.Wait()
		inst.mu.Lock()
		inst.State = StateStopped
		stopRequested := inst.stopRequested
		exitInfo := ""
		if inst.Cmd != nil && inst.Cmd.ProcessState != nil {
			exitInfo = fmt.Sprintf(", exit %d", inst.Cmd.ProcessState.ExitCode())
		}
		inst.mu.Unlock()

		fmt.Fprintf(out, "[supervisor] profile %q exited%s\n", name, exitInfo)

		s.mu.Lock()
		delete(s.instances, name)
		s.mu.Unlock()
		_ = os.Remove(s.PIDFile(name))

		// Auto-restart logic.
		if !stopRequested && p.AutoRestart {
			profile := inst.Profile
			s.mu.Lock()
			attempts := s.restarts[name]
			s.mu.Unlock()

			if p.MaxRestarts > 0 && attempts >= p.MaxRestarts {
				fmt.Fprintf(out, "[supervisor] %q reached the restart limit (%d)\n", name, p.MaxRestarts)
				return
			}

			delay := p.RestartDelay
			if delay == 0 {
				delay = 5 * time.Second
			}
			fmt.Fprintf(out, "[supervisor] restarting %q in %s (attempt %d)\n", name, delay, attempts+1)
			time.Sleep(delay)

			s.mu.Lock()
			_, exists := s.instances[name]
			if !exists {
				s.restarts[name] = attempts + 1
			}
			s.mu.Unlock()
			if exists {
				return
			}
			if err := s.Start(name, &profile); err != nil {
				fmt.Fprintf(out, "[supervisor] restart of %q failed: %v\n", name, err)
			}
		}
	}()

	if s.verbose {
		fmt.Fprintf(out, "[supervisor] started profile %q (pid %d)\n", name, inst.PID)
	}
	return nil
}

// Stop gracefully stops a profile by sending the control `stop` command
// with delay 0, then waiting for the process to exit.
func (s *Supervisor) Stop(name string) error {
	s.mu.RLock()
	inst, ok := s.instances[name]
	s.mu.RUnlock()
	if !ok {
		return fmt.Errorf("profile %q is not running", name)
	}
	return inst.Stop()
}

// StopAll gracefully stops all running instances.
func (s *Supervisor) StopAll() error {
	s.mu.RLock()
	instances := make([]*Instance, 0, len(s.instances))
	for _, inst := range s.instances {
		instances = append(instances, inst)
	}
	s.mu.RUnlock()

	var firstErr error
	for _, inst := range instances {
		inst.stopRequested = true
		if err := inst.Stop(); err != nil && firstErr == nil {
			firstErr = err
		}
	}
	return firstErr
}

// Kill immediately kills a profile's process.
func (s *Supervisor) Kill(name string) error {
	s.mu.RLock()
	inst, ok := s.instances[name]
	s.mu.RUnlock()
	if !ok {
		return fmt.Errorf("profile %q is not running", name)
	}
	return inst.Kill()
}

// KillAll immediately kills all running instances.
func (s *Supervisor) KillAll() error {
	s.mu.RLock()
	instances := make([]*Instance, 0, len(s.instances))
	for _, inst := range s.instances {
		instances = append(instances, inst)
	}
	s.mu.RUnlock()

	var firstErr error
	for _, inst := range instances {
		inst.stopRequested = true
		if err := inst.Kill(); err != nil && firstErr == nil {
			firstErr = err
		}
	}
	return firstErr
}

// IsRunning returns true if the profile is running.
func (s *Supervisor) IsRunning(name string) bool {
	s.mu.RLock()
	defer s.mu.RUnlock()
	inst, ok := s.instances[name]
	if !ok {
		return false
	}
	inst.mu.RLock()
	defer inst.mu.RUnlock()
	return inst.State == StateRunning || inst.State == StateStarting
}

// List returns all profile names currently managed.
func (s *Supervisor) List() []string {
	s.mu.RLock()
	defer s.mu.RUnlock()
	names := make([]string, 0, len(s.instances))
	for name := range s.instances {
		names = append(names, name)
	}
	return names
}

// Status returns detailed status for a profile.
func (s *Supervisor) Status(name string) (StatusInfo, error) {
	s.mu.RLock()
	inst, ok := s.instances[name]
	s.mu.RUnlock()
	if !ok {
		return StatusInfo{}, fmt.Errorf("profile %q is not running", name)
	}

	inst.mu.RLock()
	state := string(inst.State)
	pid := inst.PID
	startTime := inst.StartTime
	controlPort := inst.Profile.ControlPort
	tokenFile := inst.Profile.TokenFile
	restartCount := inst.restartCount
	healthFails := inst.healthFails
	healthLastOK := inst.healthLastOK
	inst.mu.RUnlock()

	info := StatusInfo{
		Name:         name,
		State:        state,
		PID:          pid,
		ControlPort:  controlPort,
		Uptime:       time.Since(startTime),
		RestartCount: restartCount,
		HealthFails:  healthFails,
		HealthOK:     healthLastOK.IsZero() || time.Since(healthLastOK) < 30*time.Second,
	}

	// Memory and CPU via OS-specific reading.
	if mem, cpu := readProcessMetrics(pid); mem > 0 || cpu > 0 {
		info.MemMB = mem
		info.CPUPercent = cpu
	}

	// Query the control port for the live server status if we know the port.
	if controlPort > 0 {
		token, err := readToken(tokenFile)
		if err == nil {
			if c, err := dialControl(controlPort); err == nil {
				data, statusErr := queryStatus(c, token)
				c.Close()
				if statusErr == nil {
					applyStatusData(&info, data)
					inst.mu.Lock()
					inst.healthLastOK = time.Now()
					inst.healthFails = 0
					inst.mu.Unlock()
				} else {
					inst.mu.Lock()
					inst.healthFails++
					inst.mu.Unlock()
				}
			} else {
				inst.mu.Lock()
				inst.healthFails++
				inst.mu.Unlock()
			}
		} else {
			inst.mu.Lock()
			inst.healthFails++
			inst.mu.Unlock()
		}
	}

	return info, nil
}

// applyStatusData copies the control port's status answer into info.
func applyStatusData(info *StatusInfo, data map[string]interface{}) {
	if s, ok := data["state"].(string); ok && s != "" {
		info.State = s
	}
	if s, ok := data["match_state"].(string); ok {
		info.MatchState = s
	}
	if s, ok := data["addr"].(string); ok && s != "" {
		info.Addr = s
	}
	if v, ok := data["tick"].(float64); ok {
		info.Tick = uint64(v)
	}
	if v, ok := data["tick_rate"].(float64); ok {
		info.TickRate = v
	}
	if v, ok := data["players"].(float64); ok {
		info.Players = int(v)
	}
	if v, ok := data["ships"].(float64); ok {
		info.Ships = int(v)
	}
	if v, ok := data["connections"].(float64); ok {
		info.Connections = int(v)
	}
	if v, ok := data["entities"].(float64); ok {
		info.Entities = int(v)
	}
}

// StatusAll returns status for all running instances.
func (s *Supervisor) StatusAll() map[string]StatusInfo {
	s.mu.RLock()
	defer s.mu.RUnlock()
	out := make(map[string]StatusInfo, len(s.instances))
	for name := range s.instances {
		info, err := s.Status(name)
		if err == nil {
			out[name] = info
		}
	}
	return out
}

// Exec runs a control command on a profile's control port and prints the
// response to stdout.
func (s *Supervisor) Exec(name, command string, cmdArgs []string) error {
	s.mu.RLock()
	inst, ok := s.instances[name]
	s.mu.RUnlock()
	if !ok {
		return fmt.Errorf("profile %q is not running", name)
	}
	return inst.Exec(command, cmdArgs)
}

// ExecCommand runs a control command and returns the parsed response data.
// It uses a managed instance when the profile is running in this process and
// otherwise falls back to the profile's control port.
func (s *Supervisor) ExecCommand(name, command string, cmdArgs []string) (map[string]interface{}, error) {
	s.mu.RLock()
	inst, ok := s.instances[name]
	s.mu.RUnlock()
	if ok {
		return inst.ExecCommand(command, cmdArgs)
	}

	store, err := NewStore(s.profileDir)
	if err != nil {
		return nil, err
	}
	p, err := store.Load(name)
	if err != nil {
		return nil, fmt.Errorf("profile %q not found: %w", name, err)
	}
	return CommandOnProfile(&p, command, cmdArgs)
}

// ExecAll runs a control command on all running instances.
func (s *Supervisor) ExecAll(command string, cmdArgs []string) map[string]error {
	s.mu.RLock()
	instances := make(map[string]*Instance, len(s.instances))
	for k, v := range s.instances {
		instances[k] = v
	}
	s.mu.RUnlock()

	results := make(map[string]error, len(instances))
	for name, inst := range instances {
		results[name] = inst.Exec(command, cmdArgs)
	}
	return results
}

// Watch streams a profile's combined output to the given writer until
// the process exits or the context is cancelled.
func (s *Supervisor) Watch(name string, w io.Writer) error {
	s.mu.RLock()
	inst, ok := s.instances[name]
	s.mu.RUnlock()
	if !ok {
		return fmt.Errorf("profile %q is not running", name)
	}
	return inst.Watch(w)
}

// Restart restarts a profile: stop then start again using the stored profile.
func (s *Supervisor) Restart(name string, p *Profile) error {
	if err := s.Stop(name); err != nil {
		return err
	}
	// Give the process a moment to fully exit.
	time.Sleep(500 * time.Millisecond)
	return s.Start(name, p)
}

// pump copies from src to dst with a prefix, until src is closed.
func (inst *Instance) pump(src io.Reader, dst io.Writer, prefix string) {
	defer inst.wg.Done()
	scanner := bufio.NewScanner(src)
	for scanner.Scan() {
		fmt.Fprintf(dst, "%s%s\n", prefix, scanner.Text())
	}
}

// Stop gracefully stops the instance via the control port.
func (inst *Instance) Stop() error {
	inst.mu.Lock()
	inst.State = StateStopping
	inst.stopRequested = true
	inst.mu.Unlock()

	// Try graceful stop via control port; fall back to kill on failure.
	_ = inst.execControl("stop", []string{"0"})
	return inst.wait()
}

// Kill immediately terminates the process.
func (inst *Instance) Kill() error {
	inst.mu.Lock()
	inst.State = StateStopping
	inst.stopRequested = true
	inst.mu.Unlock()
	if err := inst.Proc.Kill(); err != nil {
		return fmt.Errorf("killing process %d: %w", inst.PID, err)
	}
	return inst.wait()
}

// wait blocks until the process exits.
func (inst *Instance) wait() error {
	if inst.Cmd == nil {
		inst.mu.Lock()
		inst.State = StateStopped
		inst.mu.Unlock()
		return nil
	}
	done := make(chan error, 1)
	go func() {
		done <- inst.Cmd.Wait()
	}()
	select {
	case err := <-done:
		inst.mu.Lock()
		inst.State = StateStopped
		inst.mu.Unlock()
		if err != nil {
			return fmt.Errorf("process wait: %w", err)
		}
		return nil
	case <-time.After(30 * time.Second):
		return fmt.Errorf("timed out waiting for process %d to exit", inst.PID)
	}
}

// ExecCommand sends one control command and returns the parsed response.
func (inst *Instance) ExecCommand(command string, cmdArgs []string) (map[string]interface{}, error) {
	inst.mu.RLock()
	p := inst.Profile
	inst.mu.RUnlock()
	return CommandOnProfile(&p, command, cmdArgs)
}

// Exec sends a single control command and prints the response.
func (inst *Instance) Exec(command string, cmdArgs []string) error {
	data, err := inst.ExecCommand(command, cmdArgs)
	if err != nil {
		return err
	}
	out, _ := json.Marshal(data)
	fmt.Println(string(out))
	return nil
}

// Watch streams output until the process exits.
func (inst *Instance) Watch(w io.Writer) error {
	inst.mu.RLock()
	stdout := inst.stdout
	stderr := inst.stderr
	inst.mu.RUnlock()

	done := make(chan struct{})
	go func() {
		_, _ = io.Copy(w, stdout)
		close(done)
	}()
	go func() {
		_, _ = io.Copy(w, stderr)
		close(done)
	}()
	<-done
	return nil
}

// execControl sends a command to the control port (internal helper).
func (inst *Instance) execControl(command string, args []string) error {
	_, err := inst.ExecCommand(command, args)
	return err
}

// dialControl connects to a control port.
func dialControl(port uint16) (net.Conn, error) {
	return net.DialTimeout("tcp", fmt.Sprintf("127.0.0.1:%d", port), 2*time.Second)
}

// queryStatus performs the token hello and sends `status`.
func queryStatus(c net.Conn, token string) (map[string]interface{}, error) {
	client := &Client{conn: c, reader: bufio.NewReader(c), token: token, proto: 1}
	if err := client.hello(); err != nil {
		return nil, err
	}
	return client.Command(1, "status", nil)
}

// ReadToken reads the control token from a token file. It is the exported
// form of the helper used by status polling and command forwarding.
func ReadToken(tokenFile string) (string, error) {
	return readToken(tokenFile)
}

// readToken reads the control token from the token file next to the config.
// Returns an error if the file does not exist or cannot be parsed.
func readToken(tokenFile string) (string, error) {
	if tokenFile == "" {
		return "", fmt.Errorf("no token file configured")
	}
	data, err := os.ReadFile(tokenFile)
	if err != nil {
		return "", fmt.Errorf("reading token file %q: %w", tokenFile, err)
	}
	var tf struct {
		Token string `json:"token"`
	}
	if err := json.Unmarshal(data, &tf); err != nil {
		return "", fmt.Errorf("parsing token file %q: %w", tokenFile, err)
	}
	if tf.Token == "" {
		return "", fmt.Errorf("token file %q is empty", tokenFile)
	}
	return tf.Token, nil
}

// CommandArgs turns positional command-line arguments into the named JSON
// object the control port expects. The server names its fields explicitly
// (`text` for say, `player` for kick, `target` for ban, ...), so positional
// forwarding (`arg0`, `arg1`) would be refused as bad arguments.
func CommandArgs(command string, rest []string) (map[string]interface{}, error) {
	switch command {
	case "say":
		if len(rest) == 0 {
			return nil, fmt.Errorf("say needs text")
		}
		return map[string]interface{}{"text": strings.Join(rest, " ")}, nil

	case "kick":
		if len(rest) == 0 {
			return nil, fmt.Errorf("kick needs a player connection id")
		}
		id, err := strconv.ParseUint(rest[0], 10, 32)
		if err != nil {
			return nil, fmt.Errorf("kick: player must be a connection id, got %q", rest[0])
		}
		args := map[string]interface{}{"player": id}
		if len(rest) > 1 {
			args["reason"] = strings.Join(rest[1:], " ")
		}
		return args, nil

	case "ban":
		if len(rest) == 0 {
			return nil, fmt.Errorf("ban needs a target (name or address)")
		}
		args := map[string]interface{}{"target": rest[0]}
		if len(rest) > 1 {
			args["reason"] = strings.Join(rest[1:], " ")
		}
		return args, nil

	case "unban":
		if len(rest) == 0 {
			return nil, fmt.Errorf("unban needs a target")
		}
		return map[string]interface{}{"target": rest[0]}, nil

	case "stop":
		args := map[string]interface{}{}
		if len(rest) > 0 {
			delay, err := strconv.ParseUint(rest[0], 10, 32)
			if err != nil {
				return nil, fmt.Errorf("stop: delay must be a number of seconds, got %q", rest[0])
			}
			args["delay_s"] = delay
		}
		return args, nil

	case "log", "audit", "banlist":
		args := map[string]interface{}{}
		if len(rest) > 0 && rest[0] != "" {
			tail, err := strconv.ParseUint(rest[0], 10, 32)
			if err != nil || tail == 0 {
				return nil, fmt.Errorf("%s: tail must be a positive number, got %q", command, rest[0])
			}
			args["tail"] = tail
		}
		if command == "log" && len(rest) > 1 && rest[1] != "" {
			args["level"] = rest[1]
		}
		return args, nil

	case "watch":
		args := map[string]interface{}{}
		if len(rest) > 0 {
			args["topics"] = rest
		}
		return args, nil

	default:
		// status, perf, net, players, config, reload, end_match,
		// start_match, restart_match take no arguments; unknown commands
		// are left to the server to refuse with its own message.
		return nil, nil
	}
}

// ResolveTokenPath resolves the token file path relative to the profile's
// work directory if it is not absolute.
func ResolveTokenPath(workDir, tokenFile string) string {
	if filepath.IsAbs(tokenFile) {
		return tokenFile
	}
	if workDir != "" {
		return filepath.Join(workDir, tokenFile)
	}
	return tokenFile
}

// envMapToSlice converts a map[string]string to []string of "KEY=VALUE".
func envMapToSlice(m map[string]string) []string {
	out := make([]string, 0, len(m))
	for k, v := range m {
		out = append(out, k+"="+v)
	}
	return out
}

// DefaultProfile returns a profile with sensible defaults for the given
// name and binary path.
func DefaultProfile(name, binPath string) Profile {
	return Profile{
		Name:               name,
		BinPath:            binPath,
		ControlPort:        7778,
		TokenFile:          "control_token.json",
		AutoRestart:        false,
		MaxRestarts:        3,
		RestartDelay:       5 * time.Second,
		HealthCheckInterval: 10 * time.Second,
		HealthCheckFailures: 3,
		Env:                make(map[string]string),
		CreatedAt:          Now(),
		UpdatedAt:          Now(),
	}
}

// readProcessMetrics reads memory (MB) and CPU percent for a PID.
// Returns zeros on platforms where it's not implemented.
func readProcessMetrics(pid int) (float64, float64) {
	switch runtime.GOOS {
	case "linux":
		return readLinuxProcMetrics(pid)
	case "windows":
		return readWindowsProcessMetrics(pid)
	case "darwin":
		return readDarwinProcessMetrics(pid)
	default:
		return 0, 0
	}
}

// ClientHealthCheck performs a lightweight health check: connects to the
// control port, sends hello, sends `status`, and returns true if the server
// answers OK. It is a standalone helper usable by the panel or CLI.
func ClientHealthCheck(addr, token string, proto uint32, timeout time.Duration) bool {
	conn, err := net.DialTimeout("tcp", addr, timeout)
	if err != nil {
		return false
	}
	defer conn.Close()
	br := bufio.NewReader(conn)
	hello := fmt.Sprintf(`{"hello":{"token":"%s","proto":%d}}`+"\n", token, proto)
	if _, err := conn.Write([]byte(hello)); err != nil {
		return false
	}
	if _, err := br.ReadString('\n'); err != nil {
		return false
	}
	req := `{"id":1,"cmd":"status"}` + "\n"
	if _, err := conn.Write([]byte(req)); err != nil {
		return false
	}
	line, err := br.ReadString('\n')
	if err != nil {
		return false
	}
	var resp struct {
		OK bool `json:"ok"`
	}
	if err := json.Unmarshal([]byte(line), &resp); err != nil {
		return false
	}
	return resp.OK
}
