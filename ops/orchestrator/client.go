// Package client implements a Go client for the RFS server control port.
//
// It speaks the same newline-delimited JSON protocol as `ctl` (stage 1,
// variant Б): a token hello followed by one command per line. Used by the
// supervisor to forward commands and data to the panel window.
package supervisor

import (
	"bufio"
	"encoding/json"
	"fmt"
	"net"
	"strings"
	"time"
)

// Client is a connection to a server's control port.
type Client struct {
	conn   net.Conn
	reader *bufio.Reader
	token  string
	proto  uint32
}

// Dial connects to the control port and performs the token hello.
func Dial(addr, token string, proto uint32) (*Client, error) {
	conn, err := net.DialTimeout("tcp", addr, 2*time.Second)
	if err != nil {
		return nil, fmt.Errorf("connecting to control port %s: %w", addr, err)
	}
	c := &Client{
		conn:   conn,
		reader: bufio.NewReader(conn),
		token:  token,
		proto:  proto,
	}
	if err := c.hello(); err != nil {
		conn.Close()
		return nil, fmt.Errorf("hello: %w", err)
	}
	return c, nil
}

// Close closes the connection.
func (c *Client) Close() error {
	return c.conn.Close()
}

// hello sends the token hello and reads the ack.
func (c *Client) hello() error {
	req := fmt.Sprintf(`{"hello":{"token":"%s","proto":%d}}`+"\n", c.token, c.proto)
	if _, err := c.conn.Write([]byte(req)); err != nil {
		return err
	}
	line, err := c.reader.ReadString('\n')
	if err != nil {
		return err
	}
	var ack struct {
		OK    bool   `json:"ok"`
		Hello string `json:"hello"`
		Error struct {
			Code string `json:"code"`
			Text string `json:"text"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(line), &ack); err != nil {
		return fmt.Errorf("parsing hello ack: %w", err)
	}
	if ack.Hello != "ok" {
		return fmt.Errorf("hello refused: %s %s", ack.Error.Code, ack.Error.Text)
	}
	return nil
}

// Command sends a command and returns the parsed response.
func (c *Client) Command(id uint64, cmd string, args map[string]interface{}) (map[string]interface{}, error) {
	envelope := map[string]interface{}{"id": id, "cmd": cmd}
	if args != nil {
		envelope["args"] = args
	}
	data, err := json.Marshal(envelope)
	if err != nil {
		return nil, err
	}
	data = append(data, '\n')
	if _, err := c.conn.Write(data); err != nil {
		return nil, err
	}
	line, err := c.reader.ReadString('\n')
	if err != nil {
		return nil, err
	}
	return parseResponse(line)
}

// CommandRaw sends a raw command line and returns the raw response line.
func (c *Client) CommandRaw(line string) (string, error) {
	if _, err := c.conn.Write([]byte(line + "\n")); err != nil {
		return "", err
	}
	resp, err := c.reader.ReadString('\n')
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(resp), nil
}

// Status sends the `status` command and returns the parsed data.
func (c *Client) Status(id uint64) (map[string]interface{}, error) {
	return c.Command(id, "status", nil)
}

// Perf sends the `perf` command and returns the parsed data.
func (c *Client) Perf(id uint64) (map[string]interface{}, error) {
	return c.Command(id, "perf", nil)
}

// Net sends the `net` command and returns the parsed data.
func (c *Client) Net(id uint64) (map[string]interface{}, error) {
	return c.Command(id, "net", nil)
}

// Players sends the `players` command and returns the parsed data.
func (c *Client) Players(id uint64) (map[string]interface{}, error) {
	return c.Command(id, "players", nil)
}

// Config sends the `config` command and returns the parsed data.
func (c *Client) Config(id uint64) (map[string]interface{}, error) {
	return c.Command(id, "config", nil)
}

// Log sends the `log` command with optional tail and level.
func (c *Client) Log(id uint64, tail int, level string) (map[string]interface{}, error) {
	args := map[string]interface{}{}
	if tail > 0 {
		args["tail"] = tail
	}
	if level != "" {
		args["level"] = level
	}
	return c.Command(id, "log", args)
}

// Watch subscribes to the `watch` command with topics and starts reading
// events. Each event line is sent to handler until the connection closes.
func (c *Client) Watch(id uint64, topics []string, handler func(map[string]interface{})) error {
	args := map[string]interface{}{}
	if len(topics) > 0 {
		args["topics"] = topics
	}
	if _, err := c.Command(id, "watch", args); err != nil {
		return err
	}
	for {
		line, err := c.reader.ReadString('\n')
		if err != nil {
			return err
		}
		event, err := parseEvent(line)
		if err != nil {
			continue
		}
		handler(event)
	}
}

// Say sends the `say` command (announcement to all players).
func (c *Client) Say(id uint64, text string) (map[string]interface{}, error) {
	return c.Command(id, "say", map[string]interface{}{"text": text})
}

// Kick sends the `kick` command.
func (c *Client) Kick(id uint64, player uint32, reason string) (map[string]interface{}, error) {
	return c.Command(id, "kick", map[string]interface{}{
		"player": player,
		"reason": reason,
	})
}

// Stop sends the `stop` command with optional delay seconds.
func (c *Client) Stop(id uint64, delayS uint64) (map[string]interface{}, error) {
	args := map[string]interface{}{"delay_s": delayS}
	return c.Command(id, "stop", args)
}

// Reload sends the `reload` command.
func (c *Client) Reload(id uint64) (map[string]interface{}, error) {
	return c.Command(id, "reload", nil)
}

// Audit sends the `audit` command with optional tail.
func (c *Client) Audit(id uint64, tail int) (map[string]interface{}, error) {
	args := map[string]interface{}{}
	if tail > 0 {
		args["tail"] = tail
	}
	return c.Command(id, "audit", args)
}

// Ban sends the `ban` command (stage 3).
func (c *Client) Ban(id uint64, target, reason string) (map[string]interface{}, error) {
	args := map[string]interface{}{"target": target}
	if reason != "" {
		args["reason"] = reason
	}
	return c.Command(id, "ban", args)
}

// Unban sends the `unban` command (stage 3).
func (c *Client) Unban(id uint64, target string) (map[string]interface{}, error) {
	return c.Command(id, "unban", map[string]interface{}{"target": target})
}

// BanList sends the `banlist` command (stage 3).
func (c *Client) BanList(id uint64, tail int) (map[string]interface{}, error) {
	args := map[string]interface{}{}
	if tail > 0 {
		args["tail"] = tail
	}
	return c.Command(id, "banlist", args)
}

// EndMatch sends the `end_match` command (stage 4).
func (c *Client) EndMatch(id uint64) (map[string]interface{}, error) {
	return c.Command(id, "end_match", nil)
}

// StartMatch sends the `start_match` command (stage 4).
func (c *Client) StartMatch(id uint64) (map[string]interface{}, error) {
	return c.Command(id, "start_match", nil)
}

// RestartMatch sends the `restart_match` command (stage 4).
func (c *Client) RestartMatch(id uint64) (map[string]interface{}, error) {
	return c.Command(id, "restart_match", nil)
}

// parseResponse parses a single control-port response line.
func parseResponse(line string) (map[string]interface{}, error) {
	line = strings.TrimSpace(line)
	var resp struct {
		OK    bool                   `json:"ok"`
		ID    uint64                 `json:"id"`
		Data  map[string]interface{} `json:"data"`
		Error struct {
			Code string `json:"code"`
			Text string `json:"text"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(line), &resp); err != nil {
		return nil, fmt.Errorf("parsing response: %w", err)
	}
	if !resp.OK {
		return nil, fmt.Errorf("command refused: %s: %s", resp.Error.Code, resp.Error.Text)
	}
	return resp.Data, nil
}

// parseEvent parses a watch event line.
func parseEvent(line string) (map[string]interface{}, error) {
	line = strings.TrimSpace(line)
	var ev map[string]interface{}
	if err := json.Unmarshal([]byte(line), &ev); err != nil {
		return nil, err
	}
	return ev, nil
}

// HealthCheck performs a lightweight health check: connects, sends hello,
// sends `status`, and returns true if the server answers OK.
func HealthCheck(addr, token string, proto uint32, timeout time.Duration) bool {
	conn, err := net.DialTimeout("tcp", addr, timeout)
	if err != nil {
		return false
	}
	defer conn.Close()
	br := bufio.NewReader(conn)
	hello := fmt.Sprintf(`{"hello":{"token":"%s","proto":%d}}`+"\n", token, proto)
	if _, err := conn.Write([]byte(hello)); err != nil {
		return false
	}
	if _, err := br.ReadString('\n'); err != nil {
		return false
	}
	req := `{"id":1,"cmd":"status"}` + "\n"
	if _, err := conn.Write([]byte(req)); err != nil {
		return false
	}
	line, err := br.ReadString('\n')
	if err != nil {
		return false
	}
	var resp struct {
		OK bool `json:"ok"`
	}
	if err := json.Unmarshal([]byte(line), &resp); err != nil {
		return false
	}
	return resp.OK
}
