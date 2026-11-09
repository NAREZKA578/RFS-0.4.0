// Runtime helpers for profiles managed outside the supervisor process.
//
// The interactive dashboard owns its server processes directly (supervisor.go).
// The one-shot command line works without a long-lived supervisor: it launches
// a server as an independent process, records its PID next to the profile, and
// talks to it through the server's own control port. The same helpers let
// `status`, `exec` and `health` inspect a server started by the dashboard.
package supervisor

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"time"
)

// PIDInfo is the runtime record of a server process started for a profile.
type PIDInfo struct {
	PID       int       `json:"pid"`
	LogFile   string    `json:"log_file,omitempty"`
	StartedAt time.Time `json:"started_at"`
}

// PIDFile returns the path of the runtime PID file for a profile.
func (s *Supervisor) PIDFile(name string) string {
	return filepath.Join(s.profileDir, name+".pid")
}

// LogFile returns the default output log path of a background profile.
func (s *Supervisor) LogFile(name string) string {
	return filepath.Join(s.profileDir, name+".log")
}

// writePID records a running process for a profile. Failures are not fatal:
// the PID file is a convenience for external commands, not a source of truth.
func (s *Supervisor) writePID(name string, info PIDInfo) error {
	data, err := json.MarshalIndent(info, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(s.PIDFile(name), data, 0o644)
}

// ReadPID returns the recorded process for a profile and verifies that it is
// still alive. A stale PID file is removed.
func (s *Supervisor) ReadPID(name string) (PIDInfo, error) {
	data, err := os.ReadFile(s.PIDFile(name))
	if err != nil {
		return PIDInfo{}, err
	}
	var info PIDInfo
	if err := json.Unmarshal(data, &info); err != nil {
		return PIDInfo{}, fmt.Errorf("parsing PID file %q: %w", s.PIDFile(name), err)
	}
	if info.PID <= 0 {
		return PIDInfo{}, fmt.Errorf("PID file %q has no pid", s.PIDFile(name))
	}
	if !processAlive(info.PID) {
		_ = os.Remove(s.PIDFile(name))
		return PIDInfo{}, fmt.Errorf("process %d is not running", info.PID)
	}
	return info, nil
}

// StartDetached launches a profile as an independent process whose stdout and
// stderr go to logFile. The process is not registered with the supervisor and
// keeps running after this program exits. It is stopped through its control
// port (`StopDetached`) or killed by PID (`KillDetached`).
func (s *Supervisor) StartDetached(name string, p *Profile, logFile string) (PIDInfo, error) {
	if info, err := s.ReadPID(name); err == nil {
		return PIDInfo{}, fmt.Errorf("profile %q is already running (pid %d)", name, info.PID)
	}

	binPath := p.BinPath
	if binPath == "" {
		binPath = s.binPath
	}
	if binPath == "" {
		return PIDInfo{}, fmt.Errorf("no server binary specified for profile %q", name)
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

	if logFile == "" {
		logFile = s.LogFile(name)
	}
	if err := os.MkdirAll(filepath.Dir(logFile), 0o755); err != nil {
		return PIDInfo{}, fmt.Errorf("creating log directory: %w", err)
	}
	f, err := os.OpenFile(logFile, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	if err != nil {
		return PIDInfo{}, fmt.Errorf("opening log file %q: %w", logFile, err)
	}
	cmd.Stdout = f
	cmd.Stderr = f
	detachProcess(cmd)

	if err := cmd.Start(); err != nil {
		f.Close()
		return PIDInfo{}, fmt.Errorf("starting %s: %w", binPath, err)
	}
	f.Close()

	info := PIDInfo{
		PID:       cmd.Process.Pid,
		LogFile:   logFile,
		StartedAt: time.Now(),
	}
	if err := s.writePID(name, info); err != nil {
		_ = cmd.Process.Kill()
		return PIDInfo{}, fmt.Errorf("writing PID file: %w", err)
	}
	_ = cmd.Process.Release()
	return info, nil
}

// KillDetached kills a process recorded for a profile.
func (s *Supervisor) KillDetached(name string) error {
	info, err := s.ReadPID(name)
	if err != nil {
		return fmt.Errorf("profile %q is not running: %w", name, err)
	}
	proc, err := os.FindProcess(info.PID)
	if err != nil {
		return fmt.Errorf("finding process %d: %w", info.PID, err)
	}
	if err := proc.Kill(); err != nil {
		return fmt.Errorf("killing process %d: %w", info.PID, err)
	}
	_ = os.Remove(s.PIDFile(name))
	return nil
}

// StopDetached asks a running profile to stop through its control port and
// waits (up to timeout) for the process to exit.
func (s *Supervisor) StopDetached(p *Profile, timeout time.Duration) error {
	if _, err := CommandOnProfile(p, "stop", []string{"0"}); err != nil {
		return err
	}
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if _, err := s.ReadPID(p.Name); err != nil {
			_ = os.Remove(s.PIDFile(p.Name))
			return nil
		}
		time.Sleep(200 * time.Millisecond)
	}
	_ = os.Remove(s.PIDFile(p.Name))
	return nil
}

// QueryControlStatus asks a running server for its `status` answer.
func QueryControlStatus(p *Profile) (StatusInfo, error) {
	info := StatusInfo{Name: p.Name, ControlPort: p.ControlPort}
	if p.ControlPort == 0 {
		return info, fmt.Errorf("profile %q has no control port", p.Name)
	}
	token, err := readToken(ResolveTokenPath(p.WorkDir, p.TokenFile))
	if err != nil {
		return info, err
	}
	c, err := Dial(fmt.Sprintf("127.0.0.1:%d", p.ControlPort), token, 1)
	if err != nil {
		return info, err
	}
	defer c.Close()

	data, err := c.Command(1, "status", nil)
	if err != nil {
		return info, err
	}
	applyStatusData(&info, data)
	info.HealthOK = true
	return info, nil
}

// ProfileStatus builds a status snapshot for a profile without requiring an
// in-process instance: PID and metrics come from the runtime record, live
// state from the control port.
func (s *Supervisor) ProfileStatus(p *Profile) StatusInfo {
	info := StatusInfo{
		Name:        p.Name,
		State:       "stopped",
		ControlPort: p.ControlPort,
	}

	pidInfo, pidErr := s.ReadPID(p.Name)
	if pidErr == nil {
		info.PID = pidInfo.PID
		info.State = "starting"
		if !pidInfo.StartedAt.IsZero() {
			info.Uptime = time.Since(pidInfo.StartedAt).Round(time.Second)
		}
		if mem, cpu := readProcessMetrics(pidInfo.PID); mem > 0 || cpu > 0 {
			info.MemMB = mem
			info.CPUPercent = cpu
		}
	}

	if live, err := QueryControlStatus(p); err == nil {
		live.PID = info.PID
		live.Uptime = info.Uptime
		live.MemMB = info.MemMB
		live.CPUPercent = info.CPUPercent
		live.ControlPort = p.ControlPort
		live.Name = p.Name
		live.HealthOK = true
		return live
	}

	return info
}

// CommandOnProfile runs one control command against a profile's server
// without needing a managed instance. It performs the token hello itself.
func CommandOnProfile(p *Profile, command string, cmdArgs []string) (map[string]interface{}, error) {
	if p.ControlPort == 0 {
		return nil, fmt.Errorf("profile %q has no control port", p.Name)
	}
	token, err := readToken(ResolveTokenPath(p.WorkDir, p.TokenFile))
	if err != nil {
		return nil, fmt.Errorf("reading control token: %w", err)
	}
	args, err := CommandArgs(command, cmdArgs)
	if err != nil {
		return nil, err
	}
	c, err := Dial(fmt.Sprintf("127.0.0.1:%d", p.ControlPort), token, 1)
	if err != nil {
		return nil, err
	}
	defer c.Close()
	return c.Command(1, command, args)
}

// TailLog writes the tail of a background profile's log to w. With follow it
// keeps streaming new output until the process exits or the writer fails.
func (s *Supervisor) TailLog(name string, w io.Writer, follow bool) error {
	path := s.LogFile(name)
	f, err := os.Open(path)
	if err != nil {
		return fmt.Errorf("opening log %q: %w", path, err)
	}
	defer f.Close()

	if st, err := f.Stat(); err == nil && st.Size() > 64*1024 {
		if _, err := f.Seek(-64*1024, io.SeekEnd); err != nil {
			return err
		}
	}

	buf := make([]byte, 32*1024)
	for {
		n, readErr := f.Read(buf)
		if n > 0 {
			if _, err := w.Write(buf[:n]); err != nil {
				return err
			}
		}
		if readErr == io.EOF {
			if !follow {
				return nil
			}
			time.Sleep(300 * time.Millisecond)
			continue
		}
		if readErr != nil {
			return readErr
		}
	}
}
