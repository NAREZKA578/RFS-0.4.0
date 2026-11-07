// Package supervisor manages server profiles stored next to the supervisor.
//
// A profile is a JSON file in the profiles directory describing how to
// launch a server instance: binary path, config, arguments, work dir, and
// the control port it binds to. Profiles survive supervisor restarts.
package supervisor

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// IsZeroTime reports whether t is the zero time.
func IsZeroTime(t time.Time) bool {
	return t.IsZero()
}

// Now returns the current time. Exported for convenience when creating
// profiles from external callers.
func Now() time.Time {
	return time.Now()
}

// Store loads and saves profiles in a directory.
type Store struct {
	dir string
}

// NewStore opens a profile store in dir, creating it if needed.
func NewStore(dir string) (*Store, error) {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, err
	}
	return &Store{dir: dir}, nil
}

// path returns the file path for a profile name.
func (s *Store) path(name string) string {
	return filepath.Join(s.dir, name+".json")
}

// Save writes a profile to disk.
func (s *Store) Save(p Profile) error {
	if p.CreatedAt.IsZero() {
		p.CreatedAt = Now()
	}
	p.UpdatedAt = Now()
	data, err := json.MarshalIndent(p, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(s.path(p.Name), data, 0o644)
}

// Load reads a profile by name.
func (s *Store) Load(name string) (Profile, error) {
	var p Profile
	data, err := os.ReadFile(s.path(name))
	if err != nil {
		return p, err
	}
	if err := json.Unmarshal(data, &p); err != nil {
		return p, err
	}
	return p, nil
}

// List returns all profile names in the store.
func (s *Store) List() ([]string, error) {
	entries, err := os.ReadDir(s.dir)
	if err != nil {
		return nil, err
	}
	var names []string
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		if ext := filepath.Ext(e.Name()); ext == ".json" {
			names = append(names, strings.TrimSuffix(e.Name(), ext))
		}
	}
	return names, nil
}

// Delete removes a profile by name.
func (s *Store) Delete(name string) error {
	return os.Remove(s.path(name))
}

// Exists returns true if a profile with the given name exists.
func (s *Store) Exists(name string) bool {
	_, err := os.Stat(s.path(name))
	return err == nil
}
