// Regression tests for the supervisor fix batch (Bugs №764-780).
//
// The package's own test binary doubles as the "server" child process:
// TestSupervisorHelperChild runs in a spawned copy of this binary with a
// mode in RFS_SUPERVISOR_CHILD. That gives the tests a real, killable
// process with controllable output without needing the RFS server.
//
// Race coverage note: `go test -race` needs cgo and there is no C compiler
// on the machines this repository is built on, so these tests pin down the
// fixed behaviour with deterministic overlap and white-box state checks
// instead of the race detector.
package supervisor

import (
	"bufio"
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"
)

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// syncBuffer is a bytes.Buffer usable as supervisor output from several
// goroutines.
type syncBuffer struct {
	mu  sync.Mutex
	buf bytes.Buffer
}

func (b *syncBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *syncBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.String()
}

// waitFor polls cond until it holds or the timeout elapses.
func waitFor(t *testing.T, timeout time.Duration, what string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if cond() {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", what)
}

func fileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}

func fileMissing(path string) bool {
	_, err := os.Stat(path)
	return os.IsNotExist(err)
}

// closedControlPort returns a TCP port on 127.0.0.1 that refuses
// connections (the listener is closed immediately).
func closedControlPort(t *testing.T) uint16 {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	port := ln.Addr().(*net.TCPAddr).Port
	ln.Close()
	return uint16(port)
}

// helperChildArgs makes the spawned test binary run only the helper child.
func helperChildArgs() []string {
	return []string{"-test.run=TestSupervisorHelperChild$", "-test.v=false"}
}

// helperProfile returns a profile whose "server" is this test binary in the
// given child mode. Control port and token file are off: the child speaks
// no protocol.
func helperProfile(t *testing.T, name, mode string) Profile {
	t.Helper()
	exe, err := os.Executable()
	if err != nil {
		t.Fatalf("os.Executable: %v", err)
	}
	p := DefaultProfile(name, exe)
	p.Args = helperChildArgs()
	p.Env = map[string]string{"RFS_SUPERVISOR_CHILD": mode}
	p.ControlPort = 0
	p.TokenFile = ""
	return p
}

// shrinkStopTimeout shortens the per-instance stop wait so Close/Stop/Kill
// on a hanging child takes the kill fallback in milliseconds instead of 30
// seconds. White-box: the field is set under the instance lock.
func shrinkStopTimeout(t *testing.T, sup *Supervisor, name string, d time.Duration) {
	t.Helper()
	sup.mu.RLock()
	inst := sup.instances[name]
	sup.mu.RUnlock()
	if inst == nil {
		t.Fatalf("instance %q is not registered", name)
	}
	inst.mu.Lock()
	inst.stopTimeout = d
	inst.mu.Unlock()
}

// registerMockInstance registers an instance without a real process, the
// way the existing supervisor tests do. Status, Close and Stop on it work;
// the control port (if any) must point at a test server.
func registerMockInstance(sup *Supervisor, name string, p Profile, state InstanceState) *Instance {
	inst := &Instance{
		Profile:     p,
		PID:         os.Getpid(),
		StartTime:   time.Now(),
		State:       state,
		stopTimeout: 30 * time.Second,
	}
	sup.mu.Lock()
	sup.instances[name] = inst
	sup.mu.Unlock()
	return inst
}

// scriptMock is a control-port server with a configurable delay before the
// `status` answer, used for slow-query and deadlock tests.
type scriptMock struct {
	ln   net.Listener
	done chan struct{}
}

func newScriptMock(t *testing.T, token string, statusDelay time.Duration) *scriptMock {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	m := &scriptMock{ln: ln, done: make(chan struct{})}
	go func() {
		defer close(m.done)
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			go m.handle(conn, token, statusDelay)
		}
	}()
	return m
}

func (m *scriptMock) handle(conn net.Conn, token string, statusDelay time.Duration) {
	defer conn.Close()
	br := bufio.NewReader(conn)
	line, err := br.ReadString('\n')
	if err != nil {
		return
	}
	if !strings.Contains(line, token) {
		conn.Write([]byte(`{"ok":false,"error":{"code":"unauthorized","text":"bad token"}}` + "\n"))
		return
	}
	conn.Write([]byte(`{"hello":"ok","control_proto":1}` + "\n"))
	for {
		line, err := br.ReadString('\n')
		if err != nil {
			return
		}
		if strings.Contains(line, `"cmd":"status"`) {
			time.Sleep(statusDelay)
			conn.Write([]byte(`{"id":1,"ok":true,"data":{"state":"running","tick":7}}` + "\n"))
		} else {
			conn.Write([]byte(`{"id":1,"ok":true,"data":{}}` + "\n"))
		}
	}
}

func (m *scriptMock) port() uint16 {
	return uint16(m.ln.Addr().(*net.TCPAddr).Port)
}

func (m *scriptMock) close() {
	m.ln.Close()
	<-m.done
}

// silentMock accepts connections and then says nothing (or only a hello
// ack), to prove client reads are bounded by deadlines.
type silentMock struct {
	ln       net.Listener
	done     chan struct{}
	helloAck bool
}

func newSilentMock(t *testing.T, helloAck bool) *silentMock {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	m := &silentMock{ln: ln, done: make(chan struct{}), helloAck: helloAck}
	go func() {
		defer close(m.done)
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			go m.handle(conn)
		}
	}()
	return m
}

func (m *silentMock) handle(conn net.Conn) {
	defer conn.Close()
	br := bufio.NewReader(conn)
	if m.helloAck {
		if _, err := br.ReadString('\n'); err != nil {
			return
		}
		conn.Write([]byte(`{"hello":"ok","control_proto":1}` + "\n"))
	}
	// ...then silence for longer than any test deadline.
	time.Sleep(3 * time.Second)
}

func (m *silentMock) addr() string {
	return m.ln.Addr().String()
}

func (m *silentMock) close() {
	m.ln.Close()
	<-m.done
}

// eventMock answers the hello and the watch command, then pushes three
// events 400 ms apart — farther apart than any read deadline a test dials
// with — and closes the connection.
type eventMock struct {
	ln   net.Listener
	done chan struct{}
}

func newEventMock(t *testing.T, token string) *eventMock {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	m := &eventMock{ln: ln, done: make(chan struct{})}
	go func() {
		defer close(m.done)
		for {
			conn, err := ln.Accept()
			if err != nil {
				return
			}
			go m.handle(conn, token)
		}
	}()
	return m
}

func (m *eventMock) handle(conn net.Conn, token string) {
	defer conn.Close()
	br := bufio.NewReader(conn)
	line, err := br.ReadString('\n')
	if err != nil {
		return
	}
	if !strings.Contains(line, token) {
		conn.Write([]byte(`{"ok":false,"error":{"code":"unauthorized","text":"bad token"}}` + "\n"))
		return
	}
	conn.Write([]byte(`{"hello":"ok","control_proto":1}` + "\n"))
	for {
		line, err := br.ReadString('\n')
		if err != nil {
			return
		}
		if strings.Contains(line, `"cmd":"watch"`) {
			conn.Write([]byte(`{"id":2,"ok":true,"data":{}}` + "\n"))
			for i := 1; i <= 3; i++ {
				time.Sleep(400 * time.Millisecond)
				conn.Write([]byte(fmt.Sprintf(`{"id":2,"event":{"seq":%d}}`+"\n", i)))
			}
			return
		}
		conn.Write([]byte(`{"id":1,"ok":true,"data":{}}` + "\n"))
	}
}

func (m *eventMock) addr() string {
	return m.ln.Addr().String()
}

func (m *eventMock) close() {
	m.ln.Close()
	<-m.done
}

// ---------------------------------------------------------------------------
// The helper child process
// ---------------------------------------------------------------------------

// TestSupervisorHelperChild is not a test of the supervisor: it is the body
// of the child process the other tests spawn (see helperProfile). The mode
// arrives in RFS_SUPERVISOR_CHILD.
func TestSupervisorHelperChild(t *testing.T) {
	switch mode := os.Getenv("RFS_SUPERVISOR_CHILD"); mode {
	case "":
		t.Skip("helper child without a mode")
	case "hang":
		// Stays alive until the test kills it.
		time.Sleep(60 * time.Second)
	case "exit0":
		// Exits immediately with code 0.
	case "longline":
		// One 200 KB line: bufio.Scanner's 64 KB cap would have dropped it.
		fmt.Println(strings.Repeat("x", 200000))
	case "grandchild":
		// Spawn a grandchild that inherits this process's stdout handle
		// and outlives it. While the handle lives, the supervisor's pipe
		// never sees EOF even though the direct child is dead — the reap
		// must ride on the process death (cmd.Wait), not on pipe EOF
		// (Bug №765).
		fmt.Println("grandchild-starting")
		gc := exec.Command("cmd", "/c", "ping", "-n", "12", "127.0.0.1")
		gc.Stdout = os.Stdout
		gc.Stderr = os.Stderr
		_ = gc.Run()
	case "echo":
		// Three lines, first one delayed so the parent can subscribe a
		// watcher before any output exists.
		time.Sleep(300 * time.Millisecond)
		for _, w := range []string{"alpha", "beta", "gamma"} {
			fmt.Println(w)
			time.Sleep(150 * time.Millisecond)
		}
	default:
		t.Fatalf("unknown child mode %q", mode)
	}
}

// ---------------------------------------------------------------------------
// №765: a server that dies on its own must be reaped
// ---------------------------------------------------------------------------

func TestFix765ExternalKillReaped(t *testing.T) {
	out := &syncBuffer{}
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sup.SetOutput(out)
	t.Cleanup(func() { sup.Close() })

	p := helperProfile(t, "victim", "hang")
	if err := sup.Start("victim", &p); err != nil {
		t.Fatalf("Start: %v", err)
	}
	shrinkStopTimeout(t, sup, "victim", 500*time.Millisecond)

	waitFor(t, 5*time.Second, "victim to come up", func() bool {
		return sup.IsRunning("victim")
	})
	if !fileExists(sup.PIDFile("victim")) {
		t.Fatal("PID file should exist while running")
	}

	// Kill the process from outside the supervisor lifecycle, the way a
	// crashed server dies: no Stop, no Kill, no exit path involved.
	sup.mu.RLock()
	inst := sup.instances["victim"]
	sup.mu.RUnlock()
	if inst == nil || inst.Cmd == nil {
		t.Fatal("victim instance disappeared before the kill")
	}
	if err := inst.Cmd.Process.Kill(); err != nil {
		t.Fatalf("external kill: %v", err)
	}

	waitFor(t, 5*time.Second, "victim reap (not running, PID file removed)", func() bool {
		return !sup.IsRunning("victim") && fileMissing(sup.PIDFile("victim"))
	})
	if s := out.String(); !strings.Contains(s, "exited") {
		t.Errorf("output should report the exit, got:\n%s", s)
	}
}

// №765: the reap must ride on the process death, not on pipe EOF. The old
// exit path only watched the pipes for EOF and never called cmd.Wait: a
// grandchild inheriting the server's stdout handle kept the pipe open, and
// the dead server stayed "running" forever while its output kept flowing.
func TestFix765GrandchildKeepsPipesOpen(t *testing.T) {
	if runtime.GOOS != "windows" {
		t.Skip("the grandchild probe spawns cmd.exe")
	}
	out := &syncBuffer{}
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sup.SetOutput(out)
	t.Cleanup(func() { sup.Close() })

	p := helperProfile(t, "wrapper", "grandchild")
	// The grandchild's working directory must not pin the profile
	// directory, or the test cleanup cannot remove it while ping runs.
	p.WorkDir = os.TempDir()
	if err := sup.Start("wrapper", &p); err != nil {
		t.Fatalf("Start: %v", err)
	}
	shrinkStopTimeout(t, sup, "wrapper", 500*time.Millisecond)

	// Prove the pipe is alive and the grandchild is running through it
	// BEFORE the kill: a ping reply line flows through the pump.
	waitFor(t, 10*time.Second, "grandchild to start and pipe to flow", func() bool {
		return strings.Contains(out.String(), "grandchild-starting") &&
			strings.Contains(out.String(), "TTL")
	})

	sup.mu.RLock()
	inst := sup.instances["wrapper"]
	sup.mu.RUnlock()
	if inst == nil || inst.Cmd == nil {
		t.Fatal("wrapper instance disappeared before the kill")
	}
	if err := inst.Cmd.Process.Kill(); err != nil {
		t.Fatalf("external kill: %v", err)
	}

	// The pipe is still held by the grandchild; the reap must happen
	// anyway, because the monitor owns cmd.Wait and it returns on the
	// process death.
	waitFor(t, 6*time.Second, "wrapper reap despite the held pipe", func() bool {
		sup.mu.RLock()
		defer sup.mu.RUnlock()
		_, ok := sup.instances["wrapper"]
		return !ok
	})
	if !fileMissing(sup.PIDFile("wrapper")) {
		t.Error("PID file of a dead server should be removed despite the held pipe")
	}
	if s := out.String(); !strings.Contains(s, "exited") {
		t.Errorf("output should report the exit, got:\n%s", s)
	}
}

// ---------------------------------------------------------------------------
// №766: stopRequested written under the instance lock
// ---------------------------------------------------------------------------

func TestFix766StopFlagWrittenUnderLock(t *testing.T) {
	// Close, StopAll and KillAll must land the stop flag (written under
	// inst.mu since the fix) without deadlocking or panicking, and the
	// mock instances must come out stopped.
	for _, tc := range []struct {
		name   string
		action func(*Supervisor) error
	}{
		{"Close", func(s *Supervisor) error { return s.Close() }},
		{"StopAll", func(s *Supervisor) error { return s.StopAll() }},
		{"KillAll", func(s *Supervisor) error { return s.KillAll() }},
	} {
		sup, err := New("", t.TempDir(), false)
		if err != nil {
			t.Fatalf("New: %v", err)
		}
		inst := registerMockInstance(sup, "m", Profile{Name: "m"}, StateRunning)

		done := make(chan error, 1)
		go func() { done <- tc.action(sup) }()
		select {
		case err := <-done:
			if err != nil {
				t.Errorf("%s on a mock instance: %v", tc.name, err)
			}
		case <-time.After(3 * time.Second):
			t.Fatalf("%s deadlocked on a mock instance", tc.name)
		}

		inst.mu.RLock()
		flag := inst.stopRequested
		state := inst.State
		inst.mu.RUnlock()
		if !flag {
			t.Errorf("%s did not set stopRequested", tc.name)
		}
		if state != StateStopped {
			t.Errorf("%s left state = %v, want stopped", tc.name, state)
		}
	}
}

// ---------------------------------------------------------------------------
// №767: control-port reads must be bounded by deadlines
// ---------------------------------------------------------------------------

func TestFix767HelloReadTimeout(t *testing.T) {
	mock := newSilentMock(t, false)
	defer mock.close()

	start := time.Now()
	_, err := DialWithTimeout(mock.addr(), "tok", 1, 300*time.Millisecond)
	elapsed := time.Since(start)
	if err == nil {
		t.Fatal("hello against a silent server should time out")
	}
	if !errors.Is(err, os.ErrDeadlineExceeded) && !strings.Contains(err.Error(), "timeout") {
		t.Errorf("error should be a deadline error, got: %v", err)
	}
	if elapsed > 3*time.Second {
		t.Errorf("hello took %v, deadline was 300ms", elapsed)
	}
}

func TestFix767CommandReadTimeout(t *testing.T) {
	mock := newSilentMock(t, true) // answers hello, then goes silent
	defer mock.close()

	c, err := DialWithTimeout(mock.addr(), "tok", 1, 300*time.Millisecond)
	if err != nil {
		t.Fatalf("Dial: %v", err)
	}
	defer c.Close()

	start := time.Now()
	_, err = c.Command(1, "status", nil)
	elapsed := time.Since(start)
	if err == nil {
		t.Fatal("command against a silent server should time out")
	}
	if !errors.Is(err, os.ErrDeadlineExceeded) && !strings.Contains(err.Error(), "timeout") {
		t.Errorf("error should be a deadline error, got: %v", err)
	}
	if elapsed > 3*time.Second {
		t.Errorf("command took %v, deadline was 300ms", elapsed)
	}
}

func TestFix767HealthCheckBounded(t *testing.T) {
	mock := newSilentMock(t, false)
	defer mock.close()

	start := time.Now()
	if HealthCheck(mock.addr(), "tok", 1, 300*time.Millisecond) {
		t.Fatal("health check against a silent server should fail")
	}
	if elapsed := time.Since(start); elapsed > 3*time.Second {
		t.Errorf("health check took %v, deadline was 300ms", elapsed)
	}
}

func TestFix767WatchStreamsLongGaps(t *testing.T) {
	mock := newEventMock(t, "tok-watch")
	defer mock.close()

	// Dial with a 300 ms deadline; the events arrive 400 ms apart, so the
	// stream loop must have dropped the deadline or events 2 and 3 would
	// be lost.
	c, err := DialWithTimeout(mock.addr(), "tok-watch", 1, 300*time.Millisecond)
	if err != nil {
		t.Fatalf("Dial: %v", err)
	}
	defer c.Close()

	var mu sync.Mutex
	var seqs []int
	handler := func(ev map[string]interface{}) {
		mu.Lock()
		defer mu.Unlock()
		if e, ok := ev["event"].(map[string]interface{}); ok {
			if s, ok := e["seq"].(float64); ok {
				seqs = append(seqs, int(s))
			}
		}
	}

	start := time.Now()
	watchErr := make(chan error, 1)
	go func() { watchErr <- c.Watch(2, nil, handler) }()

	select {
	case err := <-watchErr:
		// The mock closes the connection after the third event; any error
		// (EOF) is the expected end of the stream.
		if err == nil {
			t.Log("Watch returned without error (connection closed cleanly)")
		}
	case <-time.After(5 * time.Second):
		t.Fatal("Watch did not return within 5s")
	}
	elapsed := time.Since(start)

	mu.Lock()
	defer mu.Unlock()
	if len(seqs) != 3 || seqs[0] != 1 || seqs[1] != 2 || seqs[2] != 3 {
		t.Errorf("events = %v, want [1 2 3]", seqs)
	}
	if elapsed < 1100*time.Millisecond {
		t.Errorf("stream ended after %v; 400 ms gaps were not tolerated", elapsed)
	}
}

// ---------------------------------------------------------------------------
// №768: Watch subscribes to pump output instead of racing for the pipes
// ---------------------------------------------------------------------------

func TestFix768WatchFanOutFromPumps(t *testing.T) {
	out := &syncBuffer{}
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sup.SetOutput(out)
	t.Cleanup(func() { sup.Close() })

	p := helperProfile(t, "echo-watch", "echo")
	if err := sup.Start("echo-watch", &p); err != nil {
		t.Fatalf("Start: %v", err)
	}
	shrinkStopTimeout(t, sup, "echo-watch", 500*time.Millisecond)

	wbuf := &syncBuffer{}
	watchDone := make(chan error, 1)
	go func() { watchDone <- sup.Watch("echo-watch", wbuf) }()

	waitFor(t, 5*time.Second, "watcher to see gamma", func() bool {
		return strings.Contains(wbuf.String(), "gamma")
	})
	waitFor(t, 5*time.Second, "echo-watch reap", func() bool {
		sup.mu.RLock()
		defer sup.mu.RUnlock()
		_, ok := sup.instances["echo-watch"]
		return !ok
	})
	select {
	case err := <-watchDone:
		if err != nil {
			t.Errorf("Watch: %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("Watch did not return after the process exited")
	}

	// The watcher must have received its own copy of every line, and the
	// pump output must be intact too: two readers on one pipe would have
	// split the bytes between them.
	for _, want := range []string{"alpha", "beta", "gamma"} {
		ws, os_ := wbuf.String(), out.String()
		if n := strings.Count(ws, want+"\n"); n != 1 {
			t.Errorf("watch stream has %d copies of %q, want 1", n, want)
		}
		if n := strings.Count(os_, want+"\n"); n != 1 {
			t.Errorf("pump output has %d copies of %q, want 1", n, want)
		}
	}
}

// ---------------------------------------------------------------------------
// №769: lines longer than 64 KiB must survive the pump
// ---------------------------------------------------------------------------

func TestFix769LongLineSurvives(t *testing.T) {
	out := &syncBuffer{}
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sup.SetOutput(out)
	t.Cleanup(func() { sup.Close() })

	p := helperProfile(t, "longline", "longline")
	if err := sup.Start("longline", &p); err != nil {
		t.Fatalf("Start: %v", err)
	}
	shrinkStopTimeout(t, sup, "longline", 500*time.Millisecond)

	waitFor(t, 5*time.Second, "longline reap", func() bool {
		sup.mu.RLock()
		defer sup.mu.RUnlock()
		_, ok := sup.instances["longline"]
		return !ok
	})

	got := out.String()
	want := strings.Repeat("x", 200000)
	if !strings.Contains(got, want) {
		t.Errorf("the 200 KB line did not arrive in full (%d bytes total output)", len(got))
	}
	if strings.Contains(got, "output read error") {
		t.Error("pump reported a read error on a long line")
	}
}

// ---------------------------------------------------------------------------
// №770: the restart counter is per profile and survives restarts
// ---------------------------------------------------------------------------

func TestFix770RestartCountSurvivesRestart(t *testing.T) {
	out := &syncBuffer{}
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sup.SetOutput(out)
	t.Cleanup(func() { sup.Close() })

	p := helperProfile(t, "phoenix", "hang")
	p.AutoRestart = true
	p.MaxRestarts = 5
	p.RestartDelay = 200 * time.Millisecond
	if err := sup.Start("phoenix", &p); err != nil {
		t.Fatalf("Start: %v", err)
	}
	shrinkStopTimeout(t, sup, "phoenix", 500*time.Millisecond)

	sup.mu.RLock()
	firstPID := sup.instances["phoenix"].PID
	sup.mu.RUnlock()

	// The child is killed to trigger the auto-restart; the new process
	// hangs, so its window is open indefinitely.
	sup.mu.RLock()
	inst := sup.instances["phoenix"]
	sup.mu.RUnlock()
	if err := inst.Cmd.Process.Kill(); err != nil {
		t.Fatalf("kill to trigger restart: %v", err)
	}

	waitFor(t, 8*time.Second, "restarted instance with a new PID", func() bool {
		sup.mu.RLock()
		defer sup.mu.RUnlock()
		cur, ok := sup.instances["phoenix"]
		return ok && cur.PID != firstPID
	})
	shrinkStopTimeout(t, sup, "phoenix", 500*time.Millisecond)

	// The user-visible counter must reflect the restart (the old code read
	// a per-instance field that stayed 0 forever).
	info, err := sup.Status("phoenix")
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if info.RestartCount != 1 {
		t.Errorf("Status.RestartCount = %d, want 1", info.RestartCount)
	}
	sup.mu.RLock()
	total := sup.restartTotals["phoenix"]
	sup.mu.RUnlock()
	if total != 1 {
		t.Errorf("restartTotals = %d, want 1", total)
	}
}

func TestFix770RestartLimitCumulative(t *testing.T) {
	out := &syncBuffer{}
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sup.SetOutput(out)
	t.Cleanup(func() { sup.Close() })

	p := helperProfile(t, "phoenix", "exit0")
	p.AutoRestart = true
	p.MaxRestarts = 2
	p.RestartDelay = 150 * time.Millisecond
	if err := sup.Start("phoenix", &p); err != nil {
		t.Fatalf("Start: %v", err)
	}

	// exit0 children keep crashing; the cumulative counter must stop the
	// cycle at the limit instead of restarting forever. Wait for the limit
	// message: the instance map is empty during the delay window too.
	waitFor(t, 15*time.Second, "restart cycle to exhaust the limit", func() bool {
		return strings.Contains(out.String(), "reached the restart limit")
	})
	waitFor(t, 5*time.Second, "final reap with no instance left", func() bool {
		sup.mu.RLock()
		defer sup.mu.RUnlock()
		return len(sup.instances) == 0
	})

	sup.mu.RLock()
	total := sup.restartTotals["phoenix"]
	sup.mu.RUnlock()
	if total != 2 {
		t.Errorf("restartTotals = %d, want 2 (the limit)", total)
	}
	got := out.String()
	if !strings.Contains(got, `reached the restart limit (2)`) {
		t.Errorf("output should report the limit, got:\n%s", got)
	}
	if !strings.Contains(got, "attempt 2") {
		t.Errorf("output should show attempt 2, got:\n%s", got)
	}
}

// ---------------------------------------------------------------------------
// №771: StopDetached timeout must keep the PID record
// ---------------------------------------------------------------------------

func TestFix771StopDetachedTimeoutKeepsRecord(t *testing.T) {
	dir := t.TempDir()
	mock := newScriptMock(t, "tok-detach", 0)
	defer mock.close()

	sup, err := New("", dir, false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}

	tokenPath := writeTokenFile(t, dir, "tok-detach")
	p := helperProfile(t, "detach-me", "hang")
	p.ControlPort = mock.port()
	p.TokenFile = tokenPath

	info, err := sup.StartDetached("detach-me", &p, "")
	if err != nil {
		t.Fatalf("StartDetached: %v", err)
	}
	if info.PID <= 0 {
		t.Fatalf("StartDetached returned pid %d", info.PID)
	}

	// ReadPID accepts the fresh record (this is also the positive side of
	// №775: the recorded start time matches the process creation time).
	rec, err := sup.ReadPID("detach-me")
	if err != nil {
		t.Fatalf("ReadPID: %v", err)
	}
	if rec.PID != info.PID {
		t.Errorf("ReadPID pid = %d, want %d", rec.PID, info.PID)
	}

	// The mock happily answers `stop`, but the child ignores it and keeps
	// hanging: the timeout must surface as an error, not as success.
	err = sup.StopDetached(&p, 1*time.Second)
	if err == nil {
		t.Fatal("StopDetached should report a still-running server as an error")
	}
	if !strings.Contains(err.Error(), "NOT stopped") {
		t.Errorf("error should say the server was NOT stopped, got: %v", err)
	}
	if !fileExists(sup.PIDFile("detach-me")) {
		t.Error("PID file must survive a failed stop")
	}

	// Cleanup: kill the detached child and check it is really gone.
	if err := sup.KillDetached("detach-me"); err != nil {
		t.Fatalf("KillDetached: %v", err)
	}
	waitFor(t, 3*time.Second, "detached child to die", func() bool {
		_, err := sup.ReadPID("detach-me")
		return err != nil
	})
}

// ---------------------------------------------------------------------------
// №775: PID reuse must not pass as the recorded server
// ---------------------------------------------------------------------------

func TestFix775ReadPIDRejectsMismatchedAndDead(t *testing.T) {
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}

	// A live PID (this test process) recorded with a start time ten
	// minutes ago cannot be the process the record describes.
	stale := PIDInfo{PID: os.Getpid(), StartedAt: time.Now().Add(-10 * time.Minute)}
	data, err := json.Marshal(stale)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if err := os.WriteFile(sup.PIDFile("mine"), data, 0o644); err != nil {
		t.Fatalf("write pid file: %v", err)
	}
	if _, err := sup.ReadPID("mine"); err == nil {
		t.Error("ReadPID should reject a start-time mismatch")
	} else if !strings.Contains(err.Error(), "PID reuse") {
		t.Errorf("error should mention PID reuse, got: %v", err)
	}
	if !fileMissing(sup.PIDFile("mine")) {
		t.Error("rejected PID file should be removed")
	}

	// Legacy records without a start timestamp keep the alive-only check.
	legacy := PIDInfo{PID: os.Getpid()}
	data, err = json.Marshal(legacy)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if err := os.WriteFile(sup.PIDFile("legacy"), data, 0o644); err != nil {
		t.Fatalf("write pid file: %v", err)
	}
	rec, err := sup.ReadPID("legacy")
	if err != nil {
		t.Errorf("legacy record (no StartedAt) should pass the alive check: %v", err)
	} else if rec.PID != os.Getpid() {
		t.Errorf("legacy record pid = %d, want %d", rec.PID, os.Getpid())
	}

	// A dead process is rejected and its record removed.
	p := helperProfile(t, "short", "exit0")
	if _, err := sup.StartDetached("short", &p, ""); err != nil {
		t.Fatalf("StartDetached: %v", err)
	}
	waitFor(t, 5*time.Second, "short child to die and its record to go", func() bool {
		_, err := sup.ReadPID("short")
		return err != nil
	})
	if !fileMissing(sup.PIDFile("short")) {
		t.Error("PID file of a dead process should be removed")
	}
}

// ---------------------------------------------------------------------------
// №772: an unreadable token file is not a server health failure
// ---------------------------------------------------------------------------

func TestFix772TokenErrorNotHealthFailure(t *testing.T) {
	dir := t.TempDir()
	mock := newScriptMock(t, "tok", 0)
	defer mock.close()

	sup, err := New("", dir, false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	t.Cleanup(func() { sup.Close() })

	inst := registerMockInstance(sup, "no-token", Profile{
		Name:        "no-token",
		ControlPort: mock.port(),
		TokenFile:   filepath.Join(dir, "missing_token.json"),
	}, StateRunning)

	info, err := sup.Status("no-token")
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	inst.mu.RLock()
	fails := inst.healthFails
	inst.mu.RUnlock()
	if fails != 0 {
		t.Errorf("healthFails = %d after a token read error, want 0 (config problem, not server health)", fails)
	}
	if info.Tick != 0 {
		t.Errorf("Tick = %d; a status answer should not have been applied without a token", info.Tick)
	}
}

// ---------------------------------------------------------------------------
// №776: health monitoring marks, recovers and restarts
// ---------------------------------------------------------------------------

func TestFix776UnhealthyOnFailedChecks(t *testing.T) {
	dir := t.TempDir()
	port := closedControlPort(t)
	tokenPath := writeTokenFile(t, dir, "tok")

	sup, err := New("", dir, false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	t.Cleanup(func() { sup.Close() })

	inst := registerMockInstance(sup, "sick-mock", Profile{
		Name:                "sick-mock",
		ControlPort:         port,
		TokenFile:           tokenPath,
		HealthCheckFailures: 1,
	}, StateRunning)

	// First failed check crosses the threshold of 1: unhealthy.
	if _, err := sup.Status("sick-mock"); err != nil {
		t.Fatalf("Status: %v", err)
	}
	inst.mu.RLock()
	fails, state := inst.healthFails, inst.State
	inst.mu.RUnlock()
	if fails != 1 {
		t.Errorf("healthFails = %d, want 1", fails)
	}
	if state != StateUnhealthy {
		t.Errorf("state = %v, want unhealthy after crossing the threshold", state)
	}

	// Further failures do not re-fire the transition.
	if _, err := sup.Status("sick-mock"); err != nil {
		t.Fatalf("Status: %v", err)
	}
	inst.mu.RLock()
	fails, state = inst.healthFails, inst.State
	inst.mu.RUnlock()
	if fails != 2 {
		t.Errorf("healthFails = %d, want 2", fails)
	}
	if state != StateUnhealthy {
		t.Errorf("state = %v, want still unhealthy", state)
	}
}

func TestFix776RecoveryOnSuccessfulCheck(t *testing.T) {
	dir := t.TempDir()
	mock := newScriptMock(t, "tok", 0)
	defer mock.close()
	tokenPath := writeTokenFile(t, dir, "tok")

	sup, err := New("", dir, false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	t.Cleanup(func() { sup.Close() })

	inst := registerMockInstance(sup, "recovers", Profile{
		Name:        "recovers",
		ControlPort: mock.port(),
		TokenFile:   tokenPath,
	}, StateUnhealthy)
	inst.mu.Lock()
	inst.healthFails = 5
	inst.mu.Unlock()

	info, err := sup.Status("recovers")
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	inst.mu.RLock()
	fails, state, lastOK := inst.healthFails, inst.State, inst.healthLastOK
	inst.mu.RUnlock()
	if fails != 0 {
		t.Errorf("healthFails = %d after a successful check, want 0", fails)
	}
	if state != StateRunning {
		t.Errorf("state = %v, want running after recovery", state)
	}
	if lastOK.IsZero() {
		t.Error("healthLastOK should be set after a successful check")
	}
	if !info.HealthOK {
		t.Error("info.HealthOK should be true after a successful check")
	}
	if info.State != "running" {
		t.Errorf("info.State = %q, want the live answer %q", info.State, "running")
	}
}

func TestFix776UnhealthyAutoRestartHonorsLimit(t *testing.T) {
	out := &syncBuffer{}
	dir := t.TempDir()
	port := closedControlPort(t)
	tokenPath := writeTokenFile(t, dir, "tok")

	sup, err := New("", dir, false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sup.SetOutput(out)
	t.Cleanup(func() { sup.Close() })

	p := helperProfile(t, "sick", "hang")
	p.ControlPort = port
	p.TokenFile = tokenPath
	p.AutoRestart = true
	p.MaxRestarts = 1
	p.RestartDelay = 100 * time.Millisecond
	p.HealthCheckInterval = 200 * time.Millisecond
	p.HealthCheckFailures = 1

	if err := sup.Start("sick", &p); err != nil {
		t.Fatalf("Start: %v", err)
	}
	shrinkStopTimeout(t, sup, "sick", 500*time.Millisecond)

	sup.mu.RLock()
	firstPID := sup.instances["sick"].PID
	sup.mu.RUnlock()

	// The dead control port fails every health check; after one failure
	// the instance goes unhealthy and is restarted automatically.
	waitFor(t, 12*time.Second, "unhealthy restart to produce a new process", func() bool {
		sup.mu.RLock()
		defer sup.mu.RUnlock()
		cur, ok := sup.instances["sick"]
		return ok && cur.PID != firstPID
	})
	shrinkStopTimeout(t, sup, "sick", 500*time.Millisecond)

	sup.mu.RLock()
	total := sup.restartTotals["sick"]
	sup.mu.RUnlock()
	if total != 1 {
		t.Errorf("restartTotals = %d, want 1", total)
	}

	// The second instance is unhealthy too, but the cumulative limit of 1
	// must stop the cycle instead of restarting forever.
	time.Sleep(1500 * time.Millisecond)
	sup.mu.RLock()
	cur, ok := sup.instances["sick"]
	secondPID := 0
	if ok {
		secondPID = cur.PID
	}
	sup.mu.RUnlock()
	if !ok || secondPID == 0 {
		t.Fatal("the (unhealthy) second instance should still be registered")
	}
	if secondPID == firstPID {
		t.Error("a third process appeared despite MaxRestarts=1")
	}
	got := out.String()
	if !strings.Contains(got, "unhealthy after 1 failed health checks") {
		t.Errorf("output should report the unhealthy restart, got:\n%s", got)
	}
	if !strings.Contains(got, `reached the restart limit (1)`) {
		t.Errorf("output should report the limit, got:\n%s", got)
	}
}

// ---------------------------------------------------------------------------
// №773: Restart on a stopped profile starts it; stop→start race is survived
// ---------------------------------------------------------------------------

func TestFix773RestartStartsStoppedProfile(t *testing.T) {
	out := &syncBuffer{}
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sup.SetOutput(out)
	t.Cleanup(func() { sup.Close() })

	p := helperProfile(t, "rise", "hang")
	// Nothing is running: Restart used to refuse with "not running".
	if err := sup.Restart("rise", &p); err != nil {
		t.Fatalf("Restart of a stopped profile should start it: %v", err)
	}
	shrinkStopTimeout(t, sup, "rise", 500*time.Millisecond)
	if !sup.IsRunning("rise") {
		t.Error("profile should be running after Restart")
	}
}

func TestFix773RestartStopStartRace(t *testing.T) {
	out := &syncBuffer{}
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sup.SetOutput(out)
	t.Cleanup(func() { sup.Close() })

	p := helperProfile(t, "rise", "hang")
	if err := sup.Start("rise", &p); err != nil {
		t.Fatalf("Start: %v", err)
	}
	shrinkStopTimeout(t, sup, "rise", 500*time.Millisecond)
	sup.mu.RLock()
	firstPID := sup.instances["rise"].PID
	sup.mu.RUnlock()

	// Restart stops the child (kill fallback after the stop command does
	// nothing), waits for the exit path to remove the instance, and starts
	// a new one. The old code slept a fixed 500 ms and could race the
	// removal into "already running".
	if err := sup.Restart("rise", &p); err != nil {
		t.Fatalf("Restart: %v", err)
	}
	// The new instance has the production stop timeout; shrink it so the
	// cleanup Close does not wait 30 seconds on the hanging child.
	shrinkStopTimeout(t, sup, "rise", 500*time.Millisecond)

	waitFor(t, 8*time.Second, "restarted instance with a new PID", func() bool {
		sup.mu.RLock()
		defer sup.mu.RUnlock()
		cur, ok := sup.instances["rise"]
		return ok && cur.PID != firstPID
	})
	if !sup.IsRunning("rise") {
		t.Error("profile should be running after Restart")
	}
}

// ---------------------------------------------------------------------------
// №774: Stop and Start cancel a pending auto-restart
// ---------------------------------------------------------------------------

func TestFix774StopCancelsPendingRestart(t *testing.T) {
	out := &syncBuffer{}
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sup.SetOutput(out)
	t.Cleanup(func() { sup.Close() })

	p := helperProfile(t, "ghost", "exit0")
	p.AutoRestart = true
	p.MaxRestarts = 5
	p.RestartDelay = 2 * time.Second
	if err := sup.Start("ghost", &p); err != nil {
		t.Fatalf("Start: %v", err)
	}

	// The child exits at once; the auto-restart waits out its 2 s delay.
	waitFor(t, 5*time.Second, "pending restart to appear", func() bool {
		sup.mu.RLock()
		defer sup.mu.RUnlock()
		return sup.pending["ghost"] != nil
	})

	if err := sup.Stop("ghost"); err != nil {
		t.Fatalf("Stop during the restart window: %v", err)
	}
	waitFor(t, 3*time.Second, "pending restart to be cancelled and removed", func() bool {
		sup.mu.RLock()
		defer sup.mu.RUnlock()
		return sup.pending["ghost"] == nil
	})

	// Wait well past the original 2 s delay: a cancelled restart must not
	// resurrect the server.
	time.Sleep(3200 * time.Millisecond)
	if sup.IsRunning("ghost") {
		t.Error("the server came back despite the cancelled restart")
	}
	sup.mu.RLock()
	total := sup.restartTotals["ghost"]
	sup.mu.RUnlock()
	if total != 0 {
		t.Errorf("restartTotals = %d, want 0", total)
	}
	if s := out.String(); !strings.Contains(s, `pending restart of "ghost" cancelled`) {
		t.Errorf("output should report the cancellation, got:\n%s", s)
	}
}

func TestFix774StartSupersedesPendingRestart(t *testing.T) {
	out := &syncBuffer{}
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sup.SetOutput(out)
	t.Cleanup(func() { sup.Close() })

	p := helperProfile(t, "ghost", "exit0")
	p.AutoRestart = true
	p.MaxRestarts = 5
	p.RestartDelay = 2 * time.Second
	if err := sup.Start("ghost", &p); err != nil {
		t.Fatalf("Start: %v", err)
	}
	waitFor(t, 5*time.Second, "pending restart to appear", func() bool {
		sup.mu.RLock()
		defer sup.mu.RUnlock()
		return sup.pending["ghost"] != nil
	})

	// An explicit start is the newer intent and wins over the sleeping
	// restart.
	p2 := helperProfile(t, "ghost", "hang")
	p2.AutoRestart = false
	if err := sup.Start("ghost", &p2); err != nil {
		t.Fatalf("Start during the restart window: %v", err)
	}
	shrinkStopTimeout(t, sup, "ghost", 500*time.Millisecond)

	waitFor(t, 3*time.Second, "pending restart to be removed", func() bool {
		sup.mu.RLock()
		defer sup.mu.RUnlock()
		return sup.pending["ghost"] == nil
	})
	if !sup.IsRunning("ghost") {
		t.Error("explicitly started profile should be running")
	}
	if s := out.String(); !strings.Contains(s, `pending restart of "ghost" cancelled`) {
		t.Errorf("output should report the cancellation, got:\n%s", s)
	}
}

// ---------------------------------------------------------------------------
// №777: Stop/Kill on an instance without a real process must not panic
// ---------------------------------------------------------------------------

func TestFix777NilProcStopKillNoPanic(t *testing.T) {
	sup, err := New("", t.TempDir(), false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	t.Cleanup(func() { sup.Close() })

	inst := registerMockInstance(sup, "m", Profile{Name: "m"}, StateRunning)
	if err := sup.Stop("m"); err != nil {
		t.Errorf("Stop on a mock instance: %v", err)
	}
	inst.mu.RLock()
	state := inst.State
	inst.mu.RUnlock()
	if state != StateStopped {
		t.Errorf("state after Stop = %v, want stopped", state)
	}

	inst2 := registerMockInstance(sup, "m2", Profile{Name: "m2"}, StateRunning)
	if err := sup.Kill("m2"); err != nil {
		t.Errorf("Kill on a mock instance: %v", err)
	}
	inst2.mu.RLock()
	state = inst2.State
	inst2.mu.RUnlock()
	if state != StateStopped {
		t.Errorf("state after Kill = %v, want stopped", state)
	}
}

// ---------------------------------------------------------------------------
// №780: StatusAll must not deadlock Start
// ---------------------------------------------------------------------------

func TestFix780StatusAllDoesNotDeadlockStart(t *testing.T) {
	dir := t.TempDir()
	slow := newScriptMock(t, "tok-slow", 600*time.Millisecond)
	defer slow.close()
	fast := newScriptMock(t, "tok-fast", 0)
	defer fast.close()

	sup, err := New("", dir, false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	t.Cleanup(func() { sup.Close() })

	registerMockInstance(sup, "slow", Profile{
		Name:        "slow",
		ControlPort: slow.port(),
		TokenFile:   writeTokenFile(t, dir, "tok-slow"),
	}, StateRunning)
	registerMockInstance(sup, "fast", Profile{
		Name:        "fast",
		ControlPort: fast.port(),
		TokenFile:   writeTokenFile(t, dir, "tok-fast"),
	}, StateRunning)

	allCh := make(chan map[string]StatusInfo, 1)
	go func() { allCh <- sup.StatusAll() }()

	// Queue a writer (Start) while StatusAll is inside the slow query.
	time.Sleep(150 * time.Millisecond)
	errCh := make(chan error, 1)
	go func() { errCh <- sup.Start("writer", &Profile{Name: "writer"}) }()

	select {
	case err := <-errCh:
		if err == nil {
			t.Log("writer start unexpectedly succeeded")
		} else if !strings.Contains(err.Error(), "no server binary") {
			t.Errorf("writer error = %v, want the no-binary refusal", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("Start blocked while StatusAll was running: the old deadlock")
	}

	select {
	case all := <-allCh:
		if len(all) != 2 {
			t.Errorf("StatusAll returned %d entries, want 2", len(all))
		}
		if info, ok := all["fast"]; ok && info.State != "running" {
			t.Errorf("fast state = %q, want running", info.State)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("StatusAll did not finish within 5s")
	}
}

// ---------------------------------------------------------------------------
// №789: CLI exec must reach detached profiles
// ---------------------------------------------------------------------------

// captureStdout runs f with os.Stdout redirected to a pipe and returns what
// f printed. Exec prints through fmt.Println, which reads os.Stdout at call
// time, so the swap is observable. The reader runs concurrently because f
// may write more than the pipe buffer holds.
func captureStdout(t *testing.T, f func() error) string {
	t.Helper()
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatalf("os.Pipe: %v", err)
	}
	old := os.Stdout
	os.Stdout = w
	var buf bytes.Buffer
	readDone := make(chan struct{})
	go func() {
		_, _ = io.Copy(&buf, r)
		close(readDone)
	}()
	ferr := f()
	os.Stdout = old
	if err := w.Close(); err != nil {
		t.Fatalf("close stdout pipe: %v", err)
	}
	<-readDone
	if err := r.Close(); err != nil {
		t.Fatalf("close read pipe: %v", err)
	}
	if ferr != nil {
		t.Fatalf("captured call failed: %v", ferr)
	}
	return buf.String()
}

// TestFix789ExecReachesDetachedProfile proves `Supervisor.Exec` — the path
// behind the CLI `exec` command — works for a profile this process never
// managed. A CLI invocation is a one-shot process whose instance map is
// always empty, so the old Exec answered `profile "…" is not running` for
// every detached server, however live it was, while status/health/metrics
// saw it fine through the control-port fallback. The regression pins the
// detached path and keeps the managed path printing the same way.
func TestFix789ExecReachesDetachedProfile(t *testing.T) {
	dir := t.TempDir()
	sup, err := New("", dir, false)
	if err != nil {
		t.Fatalf("New: %v", err)
	}

	mock := newScriptMock(t, "tok789", 0)
	defer mock.close()

	// The profile exists only on disk — the way a detached server leaves it:
	// no managed instance, just the store record and a live control port.
	store, err := NewStore(dir)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	detached := Profile{Name: "det789", ControlPort: mock.port(), TokenFile: writeTokenFile(t, dir, "tok789")}
	if err := store.Save(detached); err != nil {
		t.Fatalf("save profile: %v", err)
	}

	data, err := sup.ExecCommand("det789", "status", nil)
	if err != nil {
		t.Fatalf("ExecCommand on a detached profile: %v", err)
	}
	if state, _ := data["state"].(string); state != "running" {
		t.Errorf("state = %v, want running", data["state"])
	}

	// Exec itself: the old code failed here with `profile "det789" is not
	// running`; now it must print the response.
	if out := captureStdout(t, func() error { return sup.Exec("det789", "status", nil) }); !strings.Contains(out, `"state":"running"`) {
		t.Errorf("detached Exec output = %q, want the status JSON", out)
	}

	// The managed path still works and prints the same way.
	registerMockInstance(sup, "managed789", Profile{
		Name:        "managed789",
		ControlPort: mock.port(),
		TokenFile:   writeTokenFile(t, dir, "tok789"),
	}, StateRunning)
	if out := captureStdout(t, func() error { return sup.Exec("managed789", "status", nil) }); !strings.Contains(out, `"state":"running"`) {
		t.Errorf("managed Exec output = %q, want the status JSON", out)
	}
}
