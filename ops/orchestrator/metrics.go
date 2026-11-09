package supervisor

import (
	"os"
	"strconv"
	"strings"
	"time"
)

// readLinuxProcMetrics reads /proc/<pid>/stat and /proc/<pid>/status on Linux.
func readLinuxProcMetrics(pid int) (float64, float64) {
	statPath := "/proc/" + strconv.Itoa(pid) + "/stat"
	data, err := os.ReadFile(statPath)
	if err != nil {
		return 0, 0
	}
	// /proc/pid/stat fields are space-separated, but comm can contain spaces
	// and parentheses. The format is:
	// pid (comm) state ppid pgrp session tty_nr tpgid flags minflt cminflt majflt cmajflt
	// utime stime cutime cstime priority nice num_threads itrealvalue starttime vsize rss ...
	// We parse by finding the closing parenthesis of comm.
	s := string(data)
	closeParen := strings.LastIndex(s, ")")
	if closeParen < 0 {
		return 0, 0
	}
	rest := strings.TrimSpace(s[closeParen+1:])
	fields := strings.Fields(rest)
	// fields after comm: state(0) ppid(1) pgrp(2) session(3) tty_nr(4) tpgid(5)
	// flags(6) minflt(7) cminflt(8) majflt(9) cmajflt(10) utime(11) stime(12)
	// cutime(13) cstime(14) priority(15) nice(16) num_threads(17) itrealvalue(18)
	// starttime(19) vsize(20) rss(21)
	if len(fields) < 22 {
		return 0, 0
	}
	rss, err := strconv.ParseInt(fields[21], 10, 64)
	if err != nil {
		return 0, 0
	}
	// RSS is in pages; multiply by page size.
	memMB := float64(rss) * 4.0 / 1024.0 / 1024.0 // assume 4KB pages

	// CPU percent: need two readings; single reading gives cumulative time.
	// We return 0 on first call; caller should call twice.
	startTime, err := strconv.ParseInt(fields[19], 10, 64)
	if err != nil {
		return memMB, 0
	}
	utime, _ := strconv.ParseInt(fields[11], 10, 64)
	stime, _ := strconv.ParseInt(fields[12], 10, 64)
	totalTime := utime + stime

	bootTime := readLinuxBootTime()
	if bootTime == 0 {
		return memMB, 0
	}
	seconds := time.Now().Unix() - (startTime / sysconfCLKTCK()) - bootTime
	if seconds <= 0 {
		return memMB, 0
	}
	cpuPercent := float64(totalTime) / float64(sysconfCLKTCK()) / float64(seconds) * 100.0
	if cpuPercent > 100 {
		cpuPercent = 100
	}
	return memMB, cpuPercent
}

func readLinuxBootTime() int64 {
	data, err := os.ReadFile("/proc/stat")
	if err != nil {
		return 0
	}
	for _, line := range strings.Split(string(data), "\n") {
		if strings.HasPrefix(line, "btime ") {
			parts := strings.Fields(line)
			if len(parts) == 2 {
				if v, err := strconv.ParseInt(parts[1], 10, 64); err == nil {
					return v
				}
			}
		}
	}
	return 0
}

func sysconfCLKTCK() int64 {
	// On Linux, CLK_TCK is typically 100.
	return 100
}

// readDarwinProcessMetrics reads /proc on macOS (not available) — return 0.
func readDarwinProcessMetrics(pid int) (float64, float64) {
	_ = pid
	return 0, 0
}

