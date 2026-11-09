//go:build !windows

package supervisor

// readWindowsProcessMetrics is a stub on platforms that use /proc instead.
func readWindowsProcessMetrics(pid int) (float64, float64) {
	return 0, 0
}
