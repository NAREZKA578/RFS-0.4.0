// Plain-text (ANSI) output helpers for the supervisor CLI.
//
// These are the non-interactive counterparts to the tview dashboard in
// ui.go: the same data, printed to a terminal or a pipe. They are used by
// `rfs-orchestrator list|status|metrics|health`.
package supervisor

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"strings"
)

// ANSI escape sequences used by the plain-text UI.
const (
	ColorReset = "\033[0m"
	ColorBold  = "\033[1m"
	ColorDim   = "\033[2m"

	ColorRed     = "\033[31m"
	ColorGreen   = "\033[32m"
	ColorYellow  = "\033[33m"
	ColorBlue    = "\033[34m"
	ColorMagenta = "\033[35m"
	ColorCyan    = "\033[36m"
	ColorWhite   = "\033[37m"

	ColorBgRed  = "\033[41m"
	ColorBgBlue = "\033[44m"
)

// stateColor returns the ANSI color for an instance state.
func stateColor(state string) string {
	switch state {
	case "running":
		return ColorGreen
	case "starting", "stopping":
		return ColorYellow
	case "error", "unhealthy":
		return ColorRed
	case "stopped":
		return ColorDim
	default:
		return ""
	}
}

// colorize wraps s in an ANSI color, if one is given.
func colorize(s, color string) string {
	if color == "" {
		return s
	}
	return color + s + ColorReset
}

// stripANSI removes ANSI CSI escape sequences from s.
func stripANSI(s string) string {
	var b strings.Builder
	b.Grow(len(s))
	for i := 0; i < len(s); i++ {
		if s[i] == 0x1b && i+1 < len(s) && s[i+1] == '[' {
			i += 2
			for i < len(s) {
				c := s[i]
				if (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') {
					break
				}
				i++
			}
			continue
		}
		b.WriteByte(s[i])
	}
	return b.String()
}

// padRight pads s to width, counting only visible characters.
func padRight(s string, width int) string {
	visible := len(stripANSI(s))
	if visible >= width {
		return s
	}
	return s + strings.Repeat(" ", width-visible)
}

// Header prints a section title with an underline.
func Header(w io.Writer, title string) {
	fmt.Fprintf(w, "%s%s%s\n", ColorBold, title, ColorReset)
	fmt.Fprintln(w, strings.Repeat("─", len([]rune(title))))
}

// Separator prints a horizontal rule.
func Separator(w io.Writer) {
	fmt.Fprintln(w, strings.Repeat("─", 60))
}

// Box draws a bordered box around lines.
func Box(w io.Writer, title string, lines []string) {
	width := len([]rune(title))
	for _, line := range lines {
		if l := len([]rune(stripANSI(line))); l > width {
			width = l
		}
	}
	width += 2

	fmt.Fprintf(w, "┌─ %s %s┐\n", title, strings.Repeat("─", max(0, width-len([]rune(title))-3)))
	for _, line := range lines {
		visible := len([]rune(stripANSI(line)))
		fmt.Fprintf(w, "│ %s%s │\n", line, strings.Repeat(" ", max(0, width-1-visible)))
	}
	fmt.Fprintf(w, "└%s┘\n", strings.Repeat("─", width))
}

// Panel prints a titled panel with colorized content.
func Panel(w io.Writer, title string, lines []string, color string) {
	fmt.Fprintf(w, "%s%s%s\n", color, title, ColorReset)
	Box(w, title, lines)
}

// PrintTable prints a table with padded columns. colorFn may return an ANSI
// color for a cell; pass nil for no colors.
func PrintTable(w io.Writer, headers []string, rows [][]string, colorFn func(row, col int) string) {
	widths := make([]int, len(headers))
	for i, h := range headers {
		widths[i] = len([]rune(h))
	}
	for _, row := range rows {
		for i, cell := range row {
			if i >= len(widths) {
				continue
			}
			if l := len([]rune(stripANSI(cell))); l > widths[i] {
				widths[i] = l
			}
		}
	}

	cells := make([]string, len(headers))
	for i, h := range headers {
		cells[i] = padRight(h, widths[i])
	}
	fmt.Fprintln(w, strings.Join(cells, "  "))

	total := 0
	for _, width := range widths {
		total += width + 2
	}
	fmt.Fprintln(w, strings.Repeat("─", max(0, total-2)))

	for r, row := range rows {
		cells = cells[:0]
		for i := range headers {
			cell := ""
			if i < len(row) {
				cell = row[i]
			}
			if colorFn != nil {
				cell = colorize(cell, colorFn(r, i))
			}
			cells = append(cells, padRight(cell, widths[i]))
		}
		fmt.Fprintln(w, strings.Join(cells, "  "))
	}
}

// ProgressBar prints a labeled progress bar.
func ProgressBar(w io.Writer, label string, value, maximum float64, width int) {
	if width <= 0 {
		width = 40
	}
	ratio := 0.0
	if maximum > 0 {
		ratio = value / maximum
	}
	if ratio < 0 {
		ratio = 0
	}
	if ratio > 1 {
		ratio = 1
	}
	filled := int(ratio * float64(width))
	fmt.Fprintf(w, "%-12s %s%s%s%s%s%s %.1f%%  (%.1f/%.1f)\n",
		label,
		ColorGreen, strings.Repeat("█", filled), ColorReset,
		ColorDim, strings.Repeat("░", width-filled), ColorReset,
		ratio*100, value, maximum)
}

// PrintProfiles prints a table of profiles.
func PrintProfiles(w io.Writer, rows []ProfileRow) {
	headers := []string{"NAME", "STATE", "PID", "ADDR", "UPTIME", "MEM", "CPU", "TAGS"}
	data := make([][]string, 0, len(rows))
	for _, p := range rows {
		mem := "-"
		if p.MemMB > 0 {
			mem = fmt.Sprintf("%.1f MB", p.MemMB)
		}
		cpu := "-"
		if p.CPU > 0 {
			cpu = fmt.Sprintf("%.1f%%", p.CPU)
		}
		data = append(data, []string{
			p.Name,
			p.State,
			fmt.Sprintf("%d", p.PID),
			p.Addr,
			p.Uptime,
			mem,
			cpu,
			strings.Join(p.Tags, ","),
		})
	}
	PrintTable(w, headers, data, func(row, col int) string {
		if col == 1 {
			return stateColor(data[row][col])
		}
		return ""
	})
}

// PrintStatus prints one profile's status.
func PrintStatus(w io.Writer, name, state string, pid int, addr, uptime string, memMB, cpu float64) {
	fmt.Fprintf(w, "%sprofile: %s%s\n", ColorBold, name, ColorReset)
	fmt.Fprintf(w, "  state:  %s%s%s\n", stateColor(state), state, ColorReset)
	fmt.Fprintf(w, "  pid:    %d\n", pid)
	fmt.Fprintf(w, "  addr:   %s\n", addr)
	fmt.Fprintf(w, "  uptime: %s\n", uptime)
	fmt.Fprintf(w, "  memory: %.1f MB\n", memMB)
	fmt.Fprintf(w, "  cpu:    %.1f%%\n", cpu)
}

// PrintMetrics prints process metrics for one profile.
func PrintMetrics(w io.Writer, name, state string, pid int, uptime string, memMB, cpu float64, restarts, healthFails int, healthOK bool) {
	PrintStatus(w, name, state, pid, "", uptime, memMB, cpu)
	fmt.Fprintf(w, "  restarts: %d | health fails: %d | health ok=%v\n", restarts, healthFails, healthOK)
}

// PrintHealth prints a health-check result.
func PrintHealth(w io.Writer, name string, ok bool, addr string) {
	if ok {
		fmt.Fprintf(w, "%sprofile %q is healthy%s (%s)\n", ColorGreen, name, ColorReset, addr)
		return
	}
	fmt.Fprintf(w, "%sprofile %q is UNHEALTHY%s (%s not responding)\n", ColorRed, name, ColorReset, addr)
}

// PrintJSON pretty-prints a JSON document, falling back to the raw text if it
// cannot be parsed.
func PrintJSON(w io.Writer, raw string) {
	var buf bytes.Buffer
	if err := json.Indent(&buf, []byte(raw), "", "  "); err == nil {
		fmt.Fprintln(w, buf.String())
		return
	}
	fmt.Fprintln(w, raw)
}

// Dashboard prints the one-shot (non-interactive) dashboard snapshot.
func Dashboard(w io.Writer, rows []ProfileRow, totalMem, totalCPU float64) {
	Header(w, "RFS Server Supervisor")
	fmt.Fprintf(w, "%d profile(s) — memory %.1f MB — cpu %.1f%%\n\n",
		len(rows), totalMem, totalCPU)
	PrintProfiles(w, rows)
}

// PrintUsage prints the supervisor command-line help.
func PrintUsage(w io.Writer) {
	fmt.Fprintf(w, `%sRFS Server Supervisor%s

Usage:
  rfs-orchestrator [flags]                 launch the interactive dashboard
  rfs-orchestrator --ui [flags]            same as above
  rfs-orchestrator <command> [args...]     run one command

Flags:
  --profiles <dir>       profile directory (default "profiles")
  --server <path>        default RFS server binary
  --config <path>        default server config file
  --auto-restart         restart servers after an unexpected exit
  --max-restarts <n>     max auto-restart attempts (0 = unlimited)
  --restart-delay <dur>  delay between restart attempts (default 5s)
  --ui                   launch the interactive dashboard
  -v                     verbose supervisor logging

Process commands:
  start [name]           start a profile (creates "default" from --server)
  stop [name]            graceful stop via the control port
  kill [name]            kill the process immediately
  restart [name]         stop then start
  stop-all               stop every running profile
  kill-all               kill every running profile
  restart-all            restart every known profile
  list                   list profiles and states
  status [name]          detailed status for one profile
  status-all             status for every running profile
  metrics [name]         memory/CPU/uptime/restart metrics
  health [name]          control-port health check
  watch [name]           stream a server's stdout/stderr

Control-port commands (through the supervisor):
  exec [name] <cmd> [args...]   run a control command (status, players, say,
                                kick, ban, unban, banlist, end_match, ...)
  say-all <text>                announce to every running server

Profiles:
  profile-create <name> [bin]    create a profile
  profile-delete <name>          delete a stopped profile
  profile-show <name>            print a profile as JSON
  config-set <name> <key> <value> set an editable profile field

Interactive dashboard keys: F1 help, F2 new profile, F3 start, F4 stop,
F6 command line, F7 restart, F8 kill, F9 delete, F5 refresh, Ctrl+C quit.
`, ColorBold, ColorReset)
}

// max returns the larger of a and b.
func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}
