package supervisor

import (
	"bytes"
	"strings"
	"testing"
	"time"
)

func TestPrintProfiles(t *testing.T) {
	var buf bytes.Buffer
	PrintProfiles(&buf, []ProfileRow{
		{Name: "main", State: "running", PID: 1234, Addr: "127.0.0.1:7777", Uptime: "1h2m", MemMB: 256.5, CPU: 12.3, Tags: []string{"prod"}},
		{Name: "beta", State: "stopped", PID: 0, Addr: "-", Uptime: "-", MemMB: 0, CPU: 0, Tags: []string{"dev"}},
		{Name: "gamma", State: "error", PID: 5678, Addr: "127.0.0.1:7779", Uptime: "0s", MemMB: 0, CPU: 0, Tags: []string{}},
	})
	out := buf.String()
	if !strings.Contains(out, "main") {
		t.Error("output should contain profile name 'main'")
	}
	if !strings.Contains(out, "running") {
		t.Error("output should contain state 'running'")
	}
	if !strings.Contains(out, "1234") {
		t.Error("output should contain PID")
	}
	if !strings.Contains(out, "prod") {
		t.Error("output should contain tag 'prod'")
	}
	if !strings.Contains(out, "error") {
		t.Error("output should contain state 'error'")
	}
}

func TestPrintStatus(t *testing.T) {
	var buf bytes.Buffer
	PrintStatus(&buf, "main", "running", 42, "127.0.0.1:7777", "1h2m", 256.5, 12.3)
	out := buf.String()
	if !strings.Contains(out, "profile: main") {
		t.Error("output should contain profile name")
	}
	if !strings.Contains(out, "running") {
		t.Error("output should contain state")
	}
	if !strings.Contains(out, "256.5") {
		t.Error("output should contain memory")
	}
}

func TestPrintUsage(t *testing.T) {
	var buf bytes.Buffer
	PrintUsage(&buf)
	out := buf.String()
	if !strings.Contains(out, "start") {
		t.Error("usage should mention start")
	}
	if !strings.Contains(out, "--server") {
		t.Error("usage should mention --server")
	}
	if !strings.Contains(out, "--auto-restart") {
		t.Error("usage should mention --auto-restart")
	}
	if !strings.Contains(out, "http-addr") {
		t.Error("usage should mention --http-addr")
	}
	if !strings.Contains(out, "say-all") {
		t.Error("usage should mention say-all")
	}
	if !strings.Contains(out, "profile-create") {
		t.Error("usage should mention profile-create")
	}
	if !strings.Contains(out, "config-set") {
		t.Error("usage should mention config-set")
	}
}

func TestBox(t *testing.T) {
	var buf bytes.Buffer
	Box(&buf, "Title", []string{"line one", "line two"})
	out := buf.String()
	if !strings.Contains(out, "Title") {
		t.Error("box should contain title")
	}
	if !strings.Contains(out, "line one") {
		t.Error("box should contain first line")
	}
	if !strings.Contains(out, "line two") {
		t.Error("box should contain second line")
	}
}

func TestPrintHealth(t *testing.T) {
	var buf bytes.Buffer
	PrintHealth(&buf, "main", true, "127.0.0.1:7778")
	out := buf.String()
	if !strings.Contains(out, "healthy") {
		t.Error("healthy output should contain 'healthy'")
	}

	buf.Reset()
	PrintHealth(&buf, "main", false, "127.0.0.1:7778")
	out = buf.String()
	if !strings.Contains(out, "UNHEALTHY") {
		t.Error("unhealthy output should contain 'UNHEALTHY'")
	}
}

func TestPrintMetrics(t *testing.T) {
	var buf bytes.Buffer
	PrintMetrics(&buf, "main", "running", 42, "1h2m", 256.5, 12.3, 2, 0, true)
	out := buf.String()
	if !strings.Contains(out, "256.5") {
		t.Error("metrics should contain memory")
	}
	if !strings.Contains(out, "restarts:") {
		t.Error("metrics should contain restart count")
	}
	if !strings.Contains(out, "ok=true") {
		t.Error("metrics should contain health ok")
	}
}

func TestPrintTable(t *testing.T) {
	tableData := [][]string{
		{"main", "running", "1234"},
		{"beta", "stopped", "0"},
	}
	var buf bytes.Buffer
	PrintTable(&buf,
		[]string{"NAME", "STATE", "PID"},
		tableData,
		func(row, col int) string {
			if col == 1 && tableData[row][col] == "running" {
				return "\033[32m" // green
			}
			return ""
		},
	)
	out := buf.String()
	if !strings.Contains(out, "NAME") {
		t.Error("table should contain header")
	}
	if !strings.Contains(out, "main") {
		t.Error("table should contain row data")
	}
	if !strings.Contains(out, "beta") {
		t.Error("table should contain second row")
	}
}

func TestHeader(t *testing.T) {
	var buf bytes.Buffer
	Header(&buf, "Section Title")
	out := buf.String()
	if !strings.Contains(out, "Section Title") {
		t.Error("header should contain title")
	}
	if !strings.Contains(out, "─────────────") {
		t.Error("header should contain underline")
	}
}

func TestSeparator(t *testing.T) {
	var buf bytes.Buffer
	Separator(&buf)
	out := buf.String()
	expected := strings.Repeat("─", 60)
	if !strings.Contains(out, expected) {
		t.Error("separator should contain 60 dashes")
	}
}

func TestPrintJSON(t *testing.T) {
	var buf bytes.Buffer
	PrintJSON(&buf, `{"name":"main","port":7778}`)
	out := buf.String()
	if !strings.Contains(out, "name") {
		t.Error("json output should contain key")
	}
	if !strings.Contains(out, "main") {
		t.Error("json output should contain value")
	}
	if !strings.Contains(out, "port") {
		t.Error("json output should contain port key")
	}
	if !strings.Contains(out, "7778") {
		t.Error("json output should contain port value")
	}
}

func TestProgressBar(t *testing.T) {
	var buf bytes.Buffer
	ProgressBar(&buf, "memory", 256.5, 1024.0, 40)
	out := buf.String()
	if !strings.Contains(out, "memory") {
		t.Error("progress bar should contain label")
	}
	if !strings.Contains(out, "█") {
		t.Error("progress bar should contain filled blocks")
	}
	if !strings.Contains(out, "░") {
		t.Error("progress bar should contain empty blocks")
	}
}

func TestProgressBarFull(t *testing.T) {
	var buf bytes.Buffer
	ProgressBar(&buf, "cpu", 100.0, 100.0, 20)
	out := buf.String()
	if !strings.Contains(out, "100.0%") {
		t.Error("progress bar should show 100% when full")
	}
}

func TestPanel(t *testing.T) {
	var buf bytes.Buffer
	Panel(&buf, "Test Panel", []string{"line one", "line two"}, ColorBold)
	out := buf.String()
	if !strings.Contains(out, "Test Panel") {
		t.Error("panel should contain title")
	}
	if !strings.Contains(out, "line one") {
		t.Error("panel should contain content line one")
	}
	if !strings.Contains(out, "line two") {
		t.Error("panel should contain content line two")
	}
}

func TestStripANSI(t *testing.T) {
	// stripANSI is unexported, test via colorize output
	colored := colorize("hello", ColorRed)
	plain := stripANSI(colored)
	if plain != "hello" {
		t.Errorf("stripANSI should remove color codes, got %q", plain)
	}
}

func TestDashboard(t *testing.T) {
	var buf bytes.Buffer
	rows := []ProfileRow{
		{Name: "main", State: "running", PID: 1234, Addr: "127.0.0.1:7777", Uptime: "1h2m", MemMB: 256.5, CPU: 12.3, Tags: []string{"prod"}},
		{Name: "beta", State: "stopped", PID: 0, Addr: "-", Uptime: "-", MemMB: 0, CPU: 0, Tags: []string{"dev"}},
	}
	Dashboard(&buf, rows, 256.5, 12.3)
	out := buf.String()
	if !strings.Contains(out, "RFS Server Supervisor") {
		t.Error("dashboard should contain title")
	}
	if !strings.Contains(out, "main") {
		t.Error("dashboard should contain profile name")
	}
	if !strings.Contains(out, "beta") {
		t.Error("dashboard should contain second profile")
	}
}

var _ = time.Second
