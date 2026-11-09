//go:build windows

package supervisor

import (
	"os/exec"
	"syscall"

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
