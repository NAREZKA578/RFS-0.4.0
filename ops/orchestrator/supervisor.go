// Package supervisor manages RFS server processes: launch, stop, kill,
// monitor (memory/CPU/uptime), read their output, and connect to their
// control port to forward commands and data to the panel.
//
// See `.DOC/new/server-interface-plan.md`, section 8 (Надзиратель).
package supervisor

import (
	"bufio"
	"encoding/json"
	"errors"
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
//
// The Profile copy is written once at registration and never mutated
// afterwards, so it can be read without the instance lock.
type Instance struct {
	Profile Profile
	Cmd     *exec.Cmd
	Proc    *os.Process
	PID     int
	StartTime time.Time
	State     InstanceState
	mu        sync.RWMutex
	wg        sync.WaitGroup

	// Output fan-out. The pumps are the only readers of the pipes; watchers
	// (Watch) receive copies of every line they deliver, because two readers
	// on one pipe would race for bytes (Bug №768).
	watchers []*instanceWatcher

	// Lifecycle: waitDone closes when the single cmd.Wait() returned (its
	// error is then in waitErr); exit closes when the exit path finished.
	// Stop and Kill observe the exit through waitDone — os/exec forbids
	// concurrent Wait calls, so they must not Wait themselves (Bug №765).
	waitDone chan struct{}
	waitErr  error
	exit     chan struct{}

	// stopTimeout bounds each wait for process exit in Stop and Kill. Start
	// sets the production default; tests may shrink it.
	stopTimeout time.Duration

	// Restart tracking lives in Supervisor.restartTotals (session-only, not
	// persisted): the counter must survive instance changes, see Bug №770.
	healthFails   int
	healthLastOK  time.Time
	stopRequested bool // set when Stop()/Kill() is called intentionally

	// onUnhealthy is invoked when the health-check threshold is crossed.
	onUnhealthy func(fails int)
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
	pending   map[string]*pendingRestart // auto-restarts waiting out their delay
	// restartTotals counts every auto-restart a profile went through this
	// session. It survives instance changes (the limit must not reset with
	// each new process) and Status reports it as the restart count.
	restartTotals map[string]int

	done      chan struct{} // closed by Close; stopped by the first Close call
	closeOnce sync.Once
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
		binPath:       binPath,
		profileDir:    profileDir,
		verbose:       verbose,
		out:           os.Stdout,
		errOut:        os.Stderr,
		instances:     make(map[string]*Instance),
		pending:       make(map[string]*pendingRestart),
		restartTotals: make(map[string]int),
		done:          make(chan struct{}),
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

// Close shuts down all managed instances gracefully and cancels every
// auto-restart that is still waiting out its delay. It does not hold the
// supervisor lock while stopping (Bug №766): each stop can take two stop
// timeouts, and Status, List and Start must stay usable during a long
// shutdown.
func (s *Supervisor) Close() error {
	s.closeOnce.Do(func() { close(s.done) })
	s.cancelPendingRestarts()

	s.mu.RLock()
	instances := make(map[string]*Instance, len(s.instances))
	for name, inst := range s.instances {
		instances[name] = inst
	}
	s.mu.RUnlock()

	var firstErr error
	for name, inst := range instances {
		inst.mu.Lock()
		inst.stopRequested = true
		inst.mu.Unlock()
		if err := inst.Stop(); err != nil && firstErr == nil {
			firstErr = fmt.Errorf("stopping %s: %w", name, err)
		}
	}
	return firstErr
}

// Start launches a server profile using the given profile.
//
// If an auto-restart for this profile is still sleeping in its delay
// window, it is cancelled first: an explicit start is the newer intent
// (Bug №774).
func (s *Supervisor) Start(name string, p *Profile) error {
	s.cancelPendingRestart(name)

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
		Profile:     *p,
		Cmd:         cmd,
		Proc:        cmd.Process,
		PID:         cmd.Process.Pid,
		StartTime:   time.Now(),
		State:       StateRunning,
		stopTimeout: 30 * time.Second,
		waitDone:    make(chan struct{}),
		exit:        make(chan struct{}),
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

	// The monitor goroutine owns cmd.Wait — exactly one goroutine may call
	// it, and Stop/Kill observe the result through waitDone instead. The
	// old exit path never called Wait: it only watched the pipes for EOF,
	// so a dead server whose pipe was still held (say, by a grandchild
	// that inherited the handle) stayed "running" forever (Bug №765).
	profile := *p
	inst.onUnhealthy = func(fails int) {
		s.restartUnhealthy(name, profile, inst, out, fails)
	}
	go inst.monitor(s, name, profile, out)
	go inst.healthWatch(profile)

	if s.verbose {
		fmt.Fprintf(out, "[supervisor] started profile %q (pid %d)\n", name, inst.PID)
	}
	return nil
}

// Stop gracefully stops a profile by sending the control `stop` command
// with delay 0, then waiting for the process to exit. If a scheduled
// auto-restart is sleeping for this profile, it is cancelled instead: the
// server is not running, and a stop must mean it does not come back
// (Bug №774).
func (s *Supervisor) Stop(name string) error {
	s.mu.RLock()
	inst, ok := s.instances[name]
	pend := s.pending[name]
	s.mu.RUnlock()
	if ok {
		return inst.Stop()
	}
	if pend != nil {
		return s.handlePendingRestart(name, pend, func(i *Instance) error { return i.Stop() })
	}
	return fmt.Errorf("profile %q is not running", name)
}

// StopAll gracefully stops all running instances and cancels pending
// auto-restarts.
func (s *Supervisor) StopAll() error {
	s.cancelPendingRestarts()
	s.mu.RLock()
	instances := make([]*Instance, 0, len(s.instances))
	for _, inst := range s.instances {
		instances = append(instances, inst)
	}
	s.mu.RUnlock()

	var firstErr error
	for _, inst := range instances {
		inst.mu.Lock()
		inst.stopRequested = true
		inst.mu.Unlock()
		if err := inst.Stop(); err != nil && firstErr == nil {
			firstErr = err
		}
	}
	return firstErr
}

// Kill immediately kills a profile's process. A scheduled auto-restart is
// cancelled the same way Stop cancels it.
func (s *Supervisor) Kill(name string) error {
	s.mu.RLock()
	inst, ok := s.instances[name]
	pend := s.pending[name]
	s.mu.RUnlock()
	if ok {
		return inst.Kill()
	}
	if pend != nil {
		return s.handlePendingRestart(name, pend, func(i *Instance) error { return i.Kill() })
	}
	return fmt.Errorf("profile %q is not running", name)
}

// KillAll immediately kills all running instances and cancels pending
// auto-restarts.
func (s *Supervisor) KillAll() error {
	s.cancelPendingRestarts()
	s.mu.RLock()
	instances := make([]*Instance, 0, len(s.instances))
	for _, inst := range s.instances {
		instances = append(instances, inst)
	}
	s.mu.RUnlock()

	var firstErr error
	for _, inst := range instances {
		inst.mu.Lock()
		inst.stopRequested = true
		inst.mu.Unlock()
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
	healthFails := inst.healthFails
	healthLastOK := inst.healthLastOK
	inst.mu.RUnlock()

	// The restart counter is per profile, not per instance: it must survive
	// every instance change, so it is read from the supervisor map (№770).
	s.mu.RLock()
	restartCount := s.restartTotals[name]
	s.mu.RUnlock()

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
		if data, err := inst.healthQuery(controlPort, tokenFile); err == nil {
			applyStatusData(&info, data)
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

// StatusAll returns status for all running instances. It snapshots the
// names first and releases the supervisor lock: Status performs slow
// control-port queries, and holding the lock across them blocked Start and
// Stop — and the recursive read lock deadlocked against any queued writer
// (Bug №780).
func (s *Supervisor) StatusAll() map[string]StatusInfo {
	s.mu.RLock()
	names := make([]string, 0, len(s.instances))
	for name := range s.instances {
		names = append(names, name)
	}
	s.mu.RUnlock()

	out := make(map[string]StatusInfo, len(names))
	for _, name := range names {
		if info, err := s.Status(name); err == nil {
			out[name] = info
		}
	}
	return out
}

// Exec runs a control command on a profile's control port and prints the
// response to stdout.
//
// Bug №789: this used to look only at `s.instances` — the profiles managed
// by this very process. A CLI invocation is a fresh one-shot process whose
// instance map is always empty, while `start` leaves a detached server
// behind, so a perfectly live server was unreachable: every `exec` answered
// `profile "…" is not running` while status/health/metrics saw it fine
// through the control-port fallback. The command now goes through
// ExecCommand, which falls back to the profile's control port when no
// managed instance exists — the same path the dashboard's control commands
// use. For a managed instance the behaviour is unchanged: that is
// `inst.Exec`'s own implementation (ExecCommand + one JSON line).
func (s *Supervisor) Exec(name, command string, cmdArgs []string) error {
	data, err := s.ExecCommand(name, command, cmdArgs)
	if err != nil {
		return err
	}
	out, err := json.Marshal(data)
	if err != nil {
		return err
	}
	fmt.Println(string(out))
	return nil
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

// ExecAll runs a control command on all running instances. It returns the
// per-profile errors and prints nothing: printing would pour raw JSON onto
// the dashboard's fullscreen TUI (Bug №778).
func (s *Supervisor) ExecAll(command string, cmdArgs []string) map[string]error {
	s.mu.RLock()
	instances := make(map[string]*Instance, len(s.instances))
	for k, v := range s.instances {
		instances[k] = v
	}
	s.mu.RUnlock()

	results := make(map[string]error, len(instances))
	for name, inst := range instances {
		_, results[name] = inst.ExecCommand(command, cmdArgs)
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

// Restart restarts a profile: stop it if it is running, wait until the exit
// path has removed the instance, then start it again from the given profile.
// A stopped profile is simply started — "restart" of something that is not
// running is a start, not an error (Bug №773).
func (s *Supervisor) Restart(name string, p *Profile) error {
	s.mu.RLock()
	_, running := s.instances[name]
	s.mu.RUnlock()
	if !running {
		return s.Start(name, p)
	}
	if err := s.Stop(name); err != nil {
		return err
	}
	// Stop returns once cmd.Wait has returned; the exit path then removes
	// the instance after the pipes drain. Wait for that instead of guessing
	// a fixed delay, which raced the removal and could fail Start with
	// "already running" (Bug №773).
	deadline := time.Now().Add(15 * time.Second)
	for {
		s.mu.RLock()
		_, exists := s.instances[name]
		pend := s.pending[name]
		s.mu.RUnlock()
		if !exists && pend == nil {
			break
		}
		if time.Now().After(deadline) {
			return fmt.Errorf("profile %q did not stop within %s", name, 15*time.Second)
		}
		time.Sleep(50 * time.Millisecond)
	}
	return s.Start(name, p)
}

// instanceWatcher is one Watch consumer: a buffered line channel plus a
// done channel the watcher closes when it stops reading (writer failure),
// so a stalled consumer cannot block the pumps forever.
type instanceWatcher struct {
	ch   chan string
	done chan struct{}
}

// pendingRestart is an auto-restart waiting out its delay. It can be
// cancelled (Stop or Start during the window, Bug №774) and reports through
// done when it has given up its claim on the profile name.
type pendingRestart struct {
	cancel chan struct{}
	done   chan struct{}
	once   sync.Once
	taken  bool // set under the supervisor lock when the restart is launching
}

// Cancel aborts the delayed wait. After the wait already fired it has no
// effect.
func (p *pendingRestart) Cancel() {
	p.once.Do(func() { close(p.cancel) })
}

// pump copies lines from src to dst with a prefix, until src is closed.
//
// bufio.Scanner is deliberately not used: its 64 KiB token cap silently
// killed the pump on a long server line, which made the supervisor declare
// the process exited while it was still running and auto-restart into a
// duplicate (Bug №769). ReadString handles a line of any length.
func (inst *Instance) pump(src io.Reader, dst io.Writer, prefix string) {
	defer inst.wg.Done()
	reader := bufio.NewReaderSize(src, 64*1024)
	for {
		line, err := reader.ReadString('\n')
		if len(line) > 0 {
			if !strings.HasSuffix(line, "\n") {
				line += "\n"
			}
			inst.deliver(dst, prefix+line)
		}
		if err != nil {
			if err != io.EOF {
				fmt.Fprintf(dst, "[supervisor] output read error: %v\n", err)
			}
			return
		}
	}
}

// deliver writes one output line to the instance's main output and to every
// registered watcher.
func (inst *Instance) deliver(dst io.Writer, line string) {
	fmt.Fprint(dst, line)
	inst.mu.RLock()
	watchers := make([]*instanceWatcher, len(inst.watchers))
	copy(watchers, inst.watchers)
	inst.mu.RUnlock()
	for _, w := range watchers {
		select {
		case w.ch <- line:
		case <-w.done:
		}
	}
}

// Stop gracefully stops the instance via the control port, then waits for
// the process to exit. If the server ignores the stop command, the kill
// fallback the Stop comment always promised actually happens now
// (Bug №777); a hung server is stopped, not waited on for 30 seconds.
// A process that died because Stop killed it reports "exit status 1" from
// cmd.Wait — that is the expected outcome of the kill, not a stop failure.
func (inst *Instance) Stop() error {
	inst.mu.Lock()
	inst.State = StateStopping
	inst.stopRequested = true
	limit := inst.stopTimeout
	inst.mu.Unlock()

	// Try graceful stop via control port.
	_ = inst.execControl("stop", []string{"0"})
	err := inst.waitExit(limit)
	if errors.Is(err, errWaitTimeout) {
		inst.mu.RLock()
		proc := inst.Proc
		inst.mu.RUnlock()
		if proc != nil {
			_ = proc.Kill()
			if err := inst.waitExit(limit); errors.Is(err, errWaitTimeout) {
				inst.mu.RLock()
				pid := inst.PID
				inst.mu.RUnlock()
				return fmt.Errorf("timed out waiting for process %d to exit", pid)
			}
			return nil
		}
		inst.mu.RLock()
		pid := inst.PID
		inst.mu.RUnlock()
		return fmt.Errorf("timed out waiting for process %d to exit", pid)
	}
	if err != nil {
		return fmt.Errorf("process wait: %w", err)
	}
	return nil
}

// Kill immediately terminates the process. Killing an already-dead process
// is not an error — waitExit decides whether the instance is gone — and an
// instance registered without a real process stops cleanly instead of
// panicking on a nil Proc (Bug №777). The "exit status" of the killed
// process is the expected outcome of the kill, not an error.
func (inst *Instance) Kill() error {
	inst.mu.Lock()
	inst.State = StateStopping
	inst.stopRequested = true
	limit := inst.stopTimeout
	proc := inst.Proc
	pid := inst.PID
	inst.mu.Unlock()
	if proc == nil {
		inst.mu.Lock()
		inst.State = StateStopped
		inst.mu.Unlock()
		return nil
	}
	_ = proc.Kill()
	if err := inst.waitExit(limit); errors.Is(err, errWaitTimeout) {
		return fmt.Errorf("timed out waiting for process %d to exit", pid)
	}
	return nil
}

// errWaitTimeout marks that the process did not exit within the wait limit.
var errWaitTimeout = errors.New("timed out waiting for the process to exit")

// waitExit waits until the single cmd.Wait goroutine reports the exit. It
// never calls cmd.Wait itself: os/exec forbids concurrent Wait calls
// (Bug №765), so Stop and Kill observe the exit through waitDone.
func (inst *Instance) waitExit(timeout time.Duration) error {
	if inst.Cmd == nil {
		// Registered without a real process (tests, mock instances).
		inst.mu.Lock()
		inst.State = StateStopped
		inst.mu.Unlock()
		return nil
	}
	timer := time.NewTimer(timeout)
	defer timer.Stop()
	select {
	case <-inst.waitDone:
		inst.mu.RLock()
		err := inst.waitErr
		inst.mu.RUnlock()
		return err
	case <-timer.C:
		return errWaitTimeout
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

// Watch streams a copy of the instance's combined output to w until the
// process exits. The pumps stay the only readers of the pipes — two readers
// on one pipe would race for bytes (Bug №768) — so Watch subscribes to the
// same lines they deliver.
func (inst *Instance) Watch(w io.Writer) error {
	wch := &instanceWatcher{ch: make(chan string, 256), done: make(chan struct{})}
	inst.mu.Lock()
	inst.watchers = append(inst.watchers, wch)
	inst.mu.Unlock()
	defer func() {
		close(wch.done)
		inst.mu.Lock()
		for i, x := range inst.watchers {
			if x == wch {
				inst.watchers = append(inst.watchers[:i], inst.watchers[i+1:]...)
				break
			}
		}
		inst.mu.Unlock()
	}()
	for line := range wch.ch {
		if _, err := io.WriteString(w, line); err != nil {
			return err
		}
	}
	return nil
}

// monitor owns cmd.Wait — exactly one goroutine may call it — waits for the
// pumps to drain, and then runs the exit path: final state, supervisor
// bookkeeping, and the auto-restart decision. The old exit path skipped
// cmd.Wait entirely and only watched the pipes for EOF, so a dead server
// whose pipe was still held by an inherited handle was never reaped
// (Bug №765).
func (inst *Instance) monitor(s *Supervisor, name string, profile Profile, out io.Writer) {
	waitErr := inst.Cmd.Wait()
	inst.mu.Lock()
	inst.waitErr = waitErr
	inst.mu.Unlock()
	close(inst.waitDone)

	// cmd.Wait closed the pipes; the pumps drain what is buffered and exit.
	inst.wg.Wait()

	inst.mu.Lock()
	watchers := inst.watchers
	inst.watchers = nil
	stopRequested := inst.stopRequested
	inst.State = StateStopped
	exitInfo := ""
	if inst.Cmd.ProcessState != nil {
		exitInfo = fmt.Sprintf(", exit %d", inst.Cmd.ProcessState.ExitCode())
	}
	inst.mu.Unlock()
	for _, w := range watchers {
		close(w.ch)
	}

	fmt.Fprintf(out, "[supervisor] profile %q exited%s\n", name, exitInfo)

	s.mu.Lock()
	delete(s.instances, name)
	s.mu.Unlock()
	_ = os.Remove(s.PIDFile(name))
	close(inst.exit)

	if !stopRequested && profile.AutoRestart {
		s.scheduleRestart(name, profile, inst, out)
	}
}

// scheduleRestart waits out the restart delay and starts the profile again.
// The wait is cancellable, so a Stop or Start during the delay window wins
// (Bug №774) instead of the server coming back anyway. The restart counter
// is kept in Supervisor.restartTotals and is cumulative per profile: a
// per-instance counter would reset with every restart, turning MaxRestarts
// into a per-process limit — that is, no limit at all (Bug №770).
func (s *Supervisor) scheduleRestart(name string, profile Profile, inst *Instance, out io.Writer) {
	s.mu.RLock()
	attempts := s.restartTotals[name]
	s.mu.RUnlock()
	if profile.MaxRestarts > 0 && attempts >= profile.MaxRestarts {
		fmt.Fprintf(out, "[supervisor] %q reached the restart limit (%d)\n", name, profile.MaxRestarts)
		return
	}
	delay := profile.RestartDelay
	if delay <= 0 {
		delay = 5 * time.Second
	}

	pend := &pendingRestart{cancel: make(chan struct{}), done: make(chan struct{})}
	s.mu.Lock()
	s.pending[name] = pend
	s.mu.Unlock()
	defer func() {
		s.mu.Lock()
		if s.pending[name] == pend {
			delete(s.pending, name)
		}
		s.mu.Unlock()
		close(pend.done)
	}()

	fmt.Fprintf(out, "[supervisor] restarting %q in %s (attempt %d)\n", name, delay, attempts+1)
	select {
	case <-time.After(delay):
	case <-pend.cancel:
		fmt.Fprintf(out, "[supervisor] pending restart of %q cancelled\n", name)
		return
	}

	s.mu.Lock()
	if _, exists := s.instances[name]; exists {
		s.mu.Unlock()
		return
	}
	if s.pending[name] != pend {
		s.mu.Unlock()
		return
	}
	pend.taken = true
	s.restartTotals[name]++
	s.mu.Unlock()

	if err := s.Start(name, &profile); err != nil {
		fmt.Fprintf(out, "[supervisor] restart of %q failed: %v\n", name, err)
	}
}

// cancelPendingRestart is used by Start: an explicit start supersedes a
// sleeping auto-restart. Taken restarts are left alone — their owner is
// inside Start right now, and waiting on them would deadlock.
func (s *Supervisor) cancelPendingRestart(name string) {
	s.mu.RLock()
	pend := s.pending[name]
	s.mu.RUnlock()
	if pend == nil || pend.taken {
		return
	}
	pend.Cancel()
	select {
	case <-pend.done:
	case <-time.After(2 * time.Second):
	}
}

// cancelPendingRestarts cancels every sleeping auto-restart (Close,
// StopAll, KillAll).
func (s *Supervisor) cancelPendingRestarts() {
	s.mu.RLock()
	pendings := make([]*pendingRestart, 0, len(s.pending))
	for _, pend := range s.pending {
		pendings = append(pendings, pend)
	}
	s.mu.RUnlock()
	for _, pend := range pendings {
		if pend.taken {
			continue
		}
		pend.Cancel()
		select {
		case <-pend.done:
		case <-time.After(2 * time.Second):
		}
	}
}

// handlePendingRestart resolves a Stop/Kill aimed at a profile whose
// instance is gone but whose auto-restart is still pending: cancel it — or,
// if a new instance is already launching, stop that one (Bug №774).
func (s *Supervisor) handlePendingRestart(name string, pend *pendingRestart, stop func(*Instance) error) error {
	if pend.taken {
		deadline := time.Now().Add(3 * time.Second)
		for time.Now().Before(deadline) {
			s.mu.RLock()
			inst, ok := s.instances[name]
			s.mu.RUnlock()
			if ok {
				return stop(inst)
			}
			time.Sleep(50 * time.Millisecond)
		}
		return fmt.Errorf("restart of %q is already in progress; the new instance was not stopped", name)
	}
	pend.Cancel()
	select {
	case <-pend.done:
	case <-time.After(2 * time.Second):
	}
	return nil
}

// healthWatch periodically queries the control port of a running instance
// through the same counters the on-demand Status queries use. The old code
// stored HealthCheckInterval and never checked anything with it: nothing
// ever marked an instance unhealthy (Bug №776).
func (inst *Instance) healthWatch(profile Profile) {
	if profile.ControlPort == 0 {
		return
	}
	interval := profile.HealthCheckInterval
	if interval <= 0 {
		interval = 10 * time.Second
	}
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-inst.exit:
			return
		case <-ticker.C:
			_, _ = inst.healthQuery(profile.ControlPort, profile.TokenFile)
		}
	}
}

// healthQuery asks the control port for `status` and updates the health
// counters. A token that cannot be read is a configuration problem, not
// evidence about the server's health, so it does not count as a failure
// (Bug №772); only failures to reach the control port or to get an answer
// do.
func (inst *Instance) healthQuery(controlPort uint16, tokenFile string) (map[string]interface{}, error) {
	token, err := readToken(tokenFile)
	if err != nil {
		return nil, err
	}
	c, err := dialControl(controlPort)
	if err != nil {
		inst.noteHealthFailure()
		return nil, err
	}
	defer c.Close()
	data, err := queryStatus(c, token)
	if err != nil {
		inst.noteHealthFailure()
		return nil, err
	}
	inst.noteHealthSuccess()
	return data, nil
}

// noteHealthFailure counts one failed health check and, when the threshold
// is reached, marks the instance unhealthy and calls onUnhealthy.
func (inst *Instance) noteHealthFailure() {
	inst.mu.Lock()
	inst.healthFails++
	fails := inst.healthFails
	threshold := inst.Profile.HealthCheckFailures
	if threshold <= 0 {
		threshold = 3
	}
	crossed := fails == threshold
	if crossed && (inst.State == StateRunning || inst.State == StateStarting) {
		inst.State = StateUnhealthy
	}
	fn := inst.onUnhealthy
	inst.mu.Unlock()
	if crossed && fn != nil {
		fn(fails)
	}
}

// noteHealthSuccess resets the failure counter, remembers the time, and
// recovers an unhealthy instance back to running.
func (inst *Instance) noteHealthSuccess() {
	inst.mu.Lock()
	inst.healthFails = 0
	inst.healthLastOK = time.Now()
	if inst.State == StateUnhealthy {
		inst.State = StateRunning
	}
	inst.mu.Unlock()
}

// restartUnhealthy is invoked when the health threshold is crossed: with
// AutoRestart the hung server is stopped and started again. The unhealthy
// restart counts against the same cumulative limit as crash restarts —
// without this, a server that answers the port but never becomes healthy
// would be restarted forever (Bug №776).
func (s *Supervisor) restartUnhealthy(name string, profile Profile, inst *Instance, out io.Writer, fails int) {
	inst.mu.RLock()
	stopRequested := inst.stopRequested
	state := inst.State
	inst.mu.RUnlock()
	if !profile.AutoRestart || stopRequested || state != StateUnhealthy {
		return
	}
	s.mu.Lock()
	total := s.restartTotals[name]
	if profile.MaxRestarts > 0 && total >= profile.MaxRestarts {
		s.mu.Unlock()
		fmt.Fprintf(out, "[supervisor] %q unhealthy but reached the restart limit (%d)\n", name, profile.MaxRestarts)
		return
	}
	s.restartTotals[name] = total + 1
	s.mu.Unlock()
	fmt.Fprintf(out, "[supervisor] %q unhealthy after %d failed health checks; restarting\n", name, fails)
	if err := s.Restart(name, &profile); err != nil {
		fmt.Fprintf(out, "[supervisor] restart of unhealthy %q failed: %v\n", name, err)
	}
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
	client := &Client{conn: c, reader: bufio.NewReader(c), token: token, proto: 1, timeout: controlReadTimeout}
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
