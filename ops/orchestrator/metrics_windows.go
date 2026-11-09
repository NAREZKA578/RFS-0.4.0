//go:build windows

package supervisor

import (
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	psapiDLL             = windows.NewLazySystemDLL("psapi.dll")
	getProcessMemoryInfo = psapiDLL.NewProc("GetProcessMemoryInfo")
)

// processMemoryCounters mirrors PROCESS_MEMORY_COUNTERS from psapi.h.
type processMemoryCounters struct {
	cb                         uint32
	pageFaultCount             uint32
	peakWorkingSetSize         uintptr
	workingSetSize             uintptr
	quotaPeakPagedPoolUsage    uintptr
	quotaPagedPoolUsage        uintptr
	quotaPeakNonPagedPoolUsage uintptr
	quotaNonPagedPoolUsage     uintptr
	pagefileUsage              uintptr
	peakPagefileUsage          uintptr
}

// readWindowsProcessMetrics reads working-set size and average CPU usage
// (total CPU time divided by process age) for a PID.
func readWindowsProcessMetrics(pid int) (float64, float64) {
	if pid <= 0 {
		return 0, 0
	}
	h, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, uint32(pid))
	if err != nil {
		return 0, 0
	}
	defer windows.CloseHandle(h)

	var counters processMemoryCounters
	counters.cb = uint32(unsafe.Sizeof(counters))
	ok, _, _ := getProcessMemoryInfo.Call(
		uintptr(h),
		uintptr(unsafe.Pointer(&counters)),
		uintptr(counters.cb),
	)
	memMB := 0.0
	if ok != 0 {
		memMB = float64(counters.workingSetSize) / (1024 * 1024)
	}

	var creation, exit, kernel, user windows.Filetime
	if err := windows.GetProcessTimes(h, &creation, &exit, &kernel, &user); err != nil {
		return memMB, 0
	}
	elapsed := time.Now().UnixNano() - creation.Nanoseconds()
	if elapsed <= 0 {
		return memMB, 0
	}
	cpuNs := filetimeTicks(kernel) + filetimeTicks(user)
	cpu := float64(cpuNs) / float64(elapsed) * 100
	if cpu < 0 {
		cpu = 0
	}
	if cpu > 100 {
		cpu = 100
	}
	return memMB, cpu
}

// filetimeTicks converts a FILETIME duration (100 ns ticks) to nanoseconds.
// Filetime.Nanoseconds() cannot be used for durations: it subtracts the
// 1601->1970 epoch offset, which is only valid for absolute times.
func filetimeTicks(ft windows.Filetime) int64 {
	return int64(uint64(ft.HighDateTime)<<32|uint64(ft.LowDateTime)) * 100
}
