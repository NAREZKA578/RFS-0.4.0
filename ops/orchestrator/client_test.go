package supervisor

import (
	"bufio"
	"net"
	"strings"
	"testing"
)

// mockControlServer is a minimal control-port server for tests.
// It accepts one connection, expects a hello with the correct token,
// then answers commands.
type mockControlServer struct {
	ln   net.Listener
	conn net.Conn
	done chan struct{}
}

func newMockControlServer(t *testing.T, token string, proto uint32) *mockControlServer {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	m := &mockControlServer{ln: ln, done: make(chan struct{})}
	go m.acceptLoop(token, proto)
	return m
}

func (m *mockControlServer) acceptLoop(token string, proto uint32) {
	defer close(m.done)
	for {
		conn, err := m.ln.Accept()
		if err != nil {
			return
		}
		m.conn = conn
		go m.handle(token, proto)
	}
}

func (m *mockControlServer) handle(token string, proto uint32) {
	defer m.conn.Close()
	br := bufio.NewReader(m.conn)

	// Read hello and validate token + proto.
	line, err := br.ReadString('\n')
	if err != nil {
		return
	}
	if !strings.Contains(line, token) || !strings.Contains(line, `"proto":1`) {
		// Send unauthorized refusal.
		m.conn.Write([]byte(`{"ok":false,"error":{"code":"unauthorized","text":"bad token"}}` + "\n"))
		return
	}
	// Hello ack.
	ack := `{"hello":"ok","control_proto":1}` + "\n"
	m.conn.Write([]byte(ack))

	// Answer commands.
	for {
		line, err := br.ReadString('\n')
		if err != nil {
			return
		}
		line = strings.TrimSpace(line)
		if strings.Contains(line, `"cmd":"status"`) {
			m.conn.Write([]byte(`{"id":1,"ok":true,"data":{"state":"running","tick":42}}` + "\n"))
		} else if strings.Contains(line, `"cmd":"say"`) {
			m.conn.Write([]byte(`{"id":1,"ok":true,"data":{"queued":true,"bytes":5}}` + "\n"))
		} else if strings.Contains(line, `"cmd":"unknown"`) {
			m.conn.Write([]byte(`{"id":1,"ok":false,"error":{"code":"unknown_command","text":"bad"}}` + "\n"))
		} else {
			m.conn.Write([]byte(`{"id":1,"ok":true,"data":{}}` + "\n"))
		}
	}
}

func (m *mockControlServer) addr() string {
	return m.ln.Addr().String()
}

func (m *mockControlServer) close() {
	m.ln.Close()
	<-m.done
}

func TestClientHelloAndCommand(t *testing.T) {
	mock := newMockControlServer(t, "test-token-123", 1)
	defer mock.close()

	c, err := Dial(mock.addr(), "test-token-123", 1)
	if err != nil {
		t.Fatalf("Dial: %v", err)
	}
	defer c.Close()

	data, err := c.Command(1, "status", nil)
	if err != nil {
		t.Fatalf("Command: %v", err)
	}
	state, _ := data["state"].(string)
	if state != "running" {
		t.Errorf("state = %q, want %q", state, "running")
	}
	tick, _ := data["tick"].(float64)
	if int(tick) != 42 {
		t.Errorf("tick = %v, want 42", tick)
	}
}

func TestClientUnknownCommand(t *testing.T) {
	mock := newMockControlServer(t, "test-token-123", 1)
	defer mock.close()

	c, err := Dial(mock.addr(), "test-token-123", 1)
	if err != nil {
		t.Fatalf("Dial: %v", err)
	}
	defer c.Close()

	if _, err := c.Command(2, "unknown", nil); err == nil {
		t.Error("unknown command should return an error")
	} else if !strings.Contains(err.Error(), "unknown_command") {
		t.Errorf("error = %v, want unknown_command", err)
	}
}

func TestClientWrongToken(t *testing.T) {
	mock := newMockControlServer(t, "correct-token", 1)
	defer mock.close()

	_, err := Dial(mock.addr(), "wrong-token", 1)
	if err == nil {
		t.Error("wrong token should be refused")
	} else if !strings.Contains(err.Error(), "unauthorized") {
		t.Errorf("error = %v, want unauthorized", err)
	}
}
