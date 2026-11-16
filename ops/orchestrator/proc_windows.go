//go:build windows

package supervisor

import (
	"os/exec"
	"syscall"
	"time"

	"golang.org/x/sys/windows"
)

// Windows process creation flags used for background servers.
const (
	detachedProcess       = 0x00000008 // DETACHED_PROCESS
	createNewProcessGroup = 0x00000200 // CREATE_NEW_PROCESS_GROUP
)

// detachProcess starts the child without a console attached to the supervisor
// so it survives the supervisor exiting.
func detachProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{
		CreationFlags: detachedProcess | createNewProcessGroup,
	}
}

// processAlive reports whether pid names a running process.
func processAlive(pid int) bool {
	if pid <= 0 {
		return false
	}
	h, err := windows.OpenProcess(windows.SYNCHRONIZE|windows.PROCESS_QUERY_LIMITED_INFORMATION, false, uint32(pid))
	if err != nil {
		return false
	}
	defer windows.CloseHandle(h)
	event, err := windows.WaitForSingleObject(h, 0)
	if err != nil {
		return false
	}
	return event == uint32(windows.WAIT_TIMEOUT)
}

// processStartTimeMatches reports whether the process behind pid was created
// close to startedAt. PID reuse hands an old record a live but unrelated
// process; the creation time is what tells them apart (Bug №775).
// startedAt is recorded just after the process was created, so the creation
// time is at or slightly before it; the comparison allows a small skew.
func processStartTimeMatches(pid int, startedAt time.Time) bool {
	if pid <= 0 || startedAt.IsZero() {
		return false
	}
	h, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, uint32(pid))
	if err != nil {
		return false
	}
	defer windows.CloseHandle(h)
	var creation, exit, kernel, user windows.Filetime
	if err := windows.GetProcessTimes(h, &creation, &exit, &kernel, &user); err != nil {
		return false
	}
	created := time.Unix(0, creation.Nanoseconds())
	diff := created.Sub(startedAt)
	return diff >= -2*time.Second && diff <= 2*time.Second
}
