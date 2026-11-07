package supervisor

import (
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"
)

// writeTokenFile creates a control token file for supervisor tests.
func writeTokenFile(t *testing.T, dir string, token string) string {
	t.Helper()
	path := filepath.Join(dir, "control_token.json")
	if err := os.WriteFile(path, []byte(fmt.Sprintf(`{"token":"%s","created_unix_s":0}`, token)), 0o644); err != nil {
		t.Fatalf("writing token file: %v", err)
	}
	return path
}

func TestSupervisorNewRejectsMissingBinary(t *testing.T) {
	_, err := New("/nonexistent/server", t.TempDir(), false)
	if err == nil {
		t.Error("New with nonexistent binary should fail")
	}
}

func TestSupervisorNewAcceptsEmptyBinary(t *testing.T) {
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New with empty binary should succeed: %v", err)
	}
	if sup == nil {
		t.Fatal("supervisor should not be nil")
	}
}

func TestSupervisorStartStop(t *testing.T) {
	profileDir := t.TempDir()
	tokenPath := writeTokenFile(t, profileDir, "test-token")

	sup, err := New("", profileDir, false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	defer sup.Close()

	// Manually register a running instance (without starting a real process)
	// to test IsRunning and Status.
	inst := &Instance{
		Profile: Profile{
			Name:         "mock",
			BinPath:      "mock-binary",
			ControlPort:  19876,
			TokenFile:    tokenPath,
			CreatedAt:    time.Now(),
		},
		PID:       12345,
		StartTime: time.Now(),
		State:     StateRunning,
	}
	sup.mu.Lock()
	sup.instances["mock"] = inst
	sup.mu.Unlock()

	if !sup.IsRunning("mock") {
		t.Error("profile should be running after manual registration")
	}

	// Stop will try to connect to the control port (19876) which likely has
	// no server, so it will fail at execControl but wait() will hang because
	// Cmd is nil. Instead of testing Stop here, just verify state transitions.
	inst.mu.Lock()
	inst.State = StateStopping
	inst.stopRequested = true
	inst.mu.Unlock()

	if sup.IsRunning("mock") {
		t.Error("profile should not be running after state set to stopping")
	}

	// Remove from map manually for clean state.
	sup.mu.Lock()
	delete(sup.instances, "mock")
	sup.mu.Unlock()
}

func TestSupervisorStatus(t *testing.T) {
	profileDir := t.TempDir()
	tokenPath := writeTokenFile(t, profileDir, "test-token")

	sup, err := New("", profileDir, false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	defer sup.Close()

	// Register an instance without a real process to test Status logic.
	inst := &Instance{
		Profile: Profile{
			Name:         "mock",
			BinPath:      "mock-binary",
			ControlPort:  0, // no port — status should not query control
			TokenFile:    tokenPath,
			CreatedAt:    time.Now(),
		},
		PID:       12345,
		StartTime: time.Now(),
		State:     StateRunning,
	}
	sup.mu.Lock()
	sup.instances["mock"] = inst
	sup.mu.Unlock()

	info, err := sup.Status("mock")
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if info.Name != "mock" {
		t.Errorf("name = %q, want %q", info.Name, "mock")
	}
	if info.PID <= 0 {
		t.Error("pid should be positive")
	}
}

func TestSupervisorStatusAll(t *testing.T) {
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	defer sup.Close()

	all := sup.StatusAll()
	if len(all) != 0 {
		t.Errorf("StatusAll on empty supervisor should return 0, got %d", len(all))
	}
}

func TestSupervisorExecAll(t *testing.T) {
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	defer sup.Close()

	results := sup.ExecAll("status", nil)
	if len(results) != 0 {
		t.Errorf("ExecAll on empty supervisor should return 0, got %d", len(results))
	}
}

func TestClientHealthCheck(t *testing.T) {
	// Health check to a non-existent port should return false.
	ok := ClientHealthCheck("127.0.0.1:1", "token", 1, 100*time.Millisecond)
	if ok {
		t.Error("health check to closed port should fail")
	}
}
