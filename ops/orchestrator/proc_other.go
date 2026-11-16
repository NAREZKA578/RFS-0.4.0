//go:build !windows

package supervisor

import (
	"os"
	"os/exec"
	"syscall"
)

// detachProcess puts the child in its own session so it survives the
// supervisor exiting.
func detachProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
}

// processAlive reports whether pid names a running process.
func processAlive(pid int) bool {
	if pid <= 0 {
		return false
	}
	p, err := os.FindProcess(pid)
	if err != nil {
		return false
	}
	return p.Signal(syscall.Signal(0)) == nil
}

// processStartTimeMatches reports whether the process behind pid was created
// close to startedAt. There is no portable way to read the creation time
// here, so the PID-reuse check degrades to the alive check only
// (Bug №775 is closed properly on Windows; elsewhere it stays best-effort).
func processStartTimeMatches(pid int, startedAt time.Time) bool {
	_ = pid
	_ = startedAt
	return true
}
