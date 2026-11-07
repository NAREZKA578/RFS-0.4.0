package supervisor

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestStoreSaveLoadRoundTrip(t *testing.T) {
	dir := t.TempDir()
	store, err := NewStore(dir)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}

	p := Profile{
		Name:        "main",
		BinPath:     "/usr/bin/rfs-server",
		Config:      "server_config.json",
		Args:        []string{"--ships", "4"},
		ControlPort: 7778,
	}
	if err := store.Save(p); err != nil {
		t.Fatalf("Save: %v", err)
	}

	loaded, err := store.Load("main")
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if loaded.Name != p.Name {
		t.Errorf("name = %q, want %q", loaded.Name, p.Name)
	}
	if loaded.ControlPort != p.ControlPort {
		t.Errorf("control_port = %d, want %d", loaded.ControlPort, p.ControlPort)
	}
	if loaded.CreatedAt.IsZero() {
		t.Error("created_at should be set")
	}
	if loaded.UpdatedAt.IsZero() {
		t.Error("updated_at should be set")
	}
}

func TestStoreList(t *testing.T) {
	dir := t.TempDir()
	store, err := NewStore(dir)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}

	for _, name := range []string{"alpha", "beta", "gamma"} {
		if err := store.Save(Profile{Name: name, BinPath: "/bin/x"}); err != nil {
			t.Fatalf("Save %s: %v", name, err)
		}
	}

	names, err := store.List()
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(names) != 3 {
		t.Errorf("List() = %d profiles, want 3", len(names))
	}
}

func TestStoreDelete(t *testing.T) {
	dir := t.TempDir()
	store, err := NewStore(dir)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}

	if err := store.Save(Profile{Name: "tmp", BinPath: "/bin/x"}); err != nil {
		t.Fatalf("Save: %v", err)
	}
	if err := store.Delete("tmp"); err != nil {
		t.Fatalf("Delete: %v", err)
	}
	if _, err := os.Stat(filepath.Join(dir, "tmp.json")); err == nil {
		t.Error("deleted profile file still exists")
	}
}

func TestStoreLoadMissing(t *testing.T) {
	dir := t.TempDir()
	store, err := NewStore(dir)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	if _, err := store.Load("nonexistent"); err == nil {
		t.Error("Load missing profile should fail")
	}
}

func TestStoreExists(t *testing.T) {
	dir := t.TempDir()
	store, err := NewStore(dir)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	if store.Exists("foo") {
		t.Error("Exists should be false for missing profile")
	}
	if err := store.Save(Profile{Name: "foo", BinPath: "/bin/x"}); err != nil {
		t.Fatalf("Save: %v", err)
	}
	if !store.Exists("foo") {
		t.Error("Exists should be true for saved profile")
	}
}

func TestDefaultProfile(t *testing.T) {
	p := DefaultProfile("main", "/usr/bin/rfs-server")
	if p.Name != "main" {
		t.Errorf("name = %q, want %q", p.Name, "main")
	}
	if p.BinPath != "/usr/bin/rfs-server" {
		t.Errorf("bin_path = %q, want %q", p.BinPath, "/usr/bin/rfs-server")
	}
	if p.ControlPort != 7778 {
		t.Errorf("control_port = %d, want 7778", p.ControlPort)
	}
	if p.AutoRestart {
		t.Error("auto_restart should default to false")
	}
	if p.MaxRestarts != 3 {
		t.Errorf("max_restarts = %d, want 3", p.MaxRestarts)
	}
	if p.RestartDelay != 5*time.Second {
		t.Errorf("restart_delay = %v, want 5s", p.RestartDelay)
	}
	if p.HealthCheckInterval != 10*time.Second {
		t.Errorf("health_check_interval = %v, want 10s", p.HealthCheckInterval)
	}
	if p.HealthCheckFailures != 3 {
		t.Errorf("health_check_failures = %d, want 3", p.HealthCheckFailures)
	}
	if p.Env == nil {
		t.Error("env should be initialized")
	}
	if p.CreatedAt.IsZero() {
		t.Error("created_at should be set")
	}
	if p.UpdatedAt.IsZero() {
		t.Error("updated_at should be set")
	}
}

var _ = time.Now
