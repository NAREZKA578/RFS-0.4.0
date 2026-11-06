//! The control door itself: a tokio TCP listener bound to the loopback,
//! opened by a one-line token hello and kept under four caps the plan fixes
//! (section 5): simultaneous connections, one line's bytes, commands per
//! second per connection, and consecutive wrong tokens before authentication
//! pauses.
//!
//! Stage boundary: this file never mutates the match. Every command it
//! serves is a read from the snapshot the loop published (commands.rs) — the
//! mutating commands are stage 2 and come with an action journal of their
//! own.

use super::commands::{self, RedactedConfig, Reply, Request};
use super::journal::ActionJournal;
use super::proto::{self, ParseError};
use super::ring::LogRing;
use super::status::StatusHandle;
use anyhow::{bail, Context, Result};
use crate::config::{MatchConfig, VoteKickConfig};
use parking_lot::{Mutex, RwLock};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, Semaphore};
use tracing::{info, warn};

/// The plan's first wall (section 5, item 1): the door exists on this
/// machine, nowhere else. Binding to this literal and asserting the bind's
/// loopback are the same wall said twice — a future parameter must defeat
/// the assertion, not silently replace it.
pub const LISTEN_ADDR: &str = "127.0.0.1";

/// The stage-2 orders the door queues for the tick loop (А.2 item 1). The
/// loop drains them at the start of a tick — the same "commands execute
/// between ticks" order section 6.3 fixes for player commands. The door
/// gates against its snapshot; the loop is the only mutator and re-checks
/// its live state, so a snapshot-raced duplicate (`stop` twice) resolves
/// idempotently there.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlOrder {
    /// `say`: the text was gated at the door; the loop fans it out as one
    /// `GameEvent::Announcement` per connection-boundary rule.
    Say { text: String },
    /// `kick`: the loop delivers the reason event first, then
    /// `Disconnect(Kicked)` — the order leaves the socket in enqueue order
    /// (the delivery itself is best effort, №272: events ride the
    /// unreliable channel).
    Kick { connection_id: u32, reason: String },
    /// `stop`: `0` — immediate; N — the soft-stop, 60/30/10 s announcements,
    /// a stop on an empty server runs immediately whatever the delay.
    Stop { delay_s: u64 },
    /// `reload`'s live-applyable difference: the door validated the whole
    /// file and answered; the loop adopts the section between ticks and
    /// may keep a standing vote's tally.
    ReloadVoteKick { vote_kick: VoteKickConfig },
}

/// How many orders may wait between ticks. The door pushes at most
/// `commands_per_second` per connection, the loop drains once per tick —
/// a full one means the loop has not ticked for that long, which is the
/// operator's real story. №755: 256 — seconds of storm headroom and a
/// margin over the old 64 without changing the drain loop.
pub const QUEUE_CAP: usize = 256;

impl ControlOrder {
    /// The journal's `command` field for whatever the loop does with this
    /// order — including ignoring one when a stop is already in the air: a
    /// snapshot-raced duplicate is real history too.
    pub fn journal_line(&self) -> String {
        match self {
            ControlOrder::Say { text } => format!("say {text:?}"),
            ControlOrder::Kick {
                connection_id,
                reason,
            } => format!("kick {connection_id} {reason:?}"),
            ControlOrder::Stop { delay_s } => format!("stop {delay_s}"),
            ControlOrder::ReloadVoteKick { .. } => "reload (vote_kick)".into(),
        }
    }
}

/// The wiring the port serves. The command layer (commands.rs) consumes
/// exactly these; a future stage adds its own handle without touching this
/// struct's existing fields.
#[derive(Clone)]
pub struct ControlEnv {
    pub status: StatusHandle,
    pub ring: LogRing,
    /// Behind the lock so the loop can fold a queued reload in: a `config`
    /// answer then reads what RUNS, not what the file said at start. The
    /// wrap is one `Arc<RwLock>` — reads copy at dispatch, the rare write
    /// is one scoped `write()`.
    pub config: Arc<RwLock<RedactedConfig>>,
    /// The №700 running key, for the reload diff only: compared, never
    /// echoed, never serialized by any answer (same exposure class the
    /// hello token already carries inside every door task).
    pub auth_token: Option<String>,
    /// The queue to the tick loop. A full queue and a closed one both land
    /// in the command's answer — a control port that pretends to have
    /// queued nothing is what journal entries are for.
    pub queue: mpsc::Sender<ControlOrder>,
    /// The action journal (А.2 item 6): the door records what it decides,
    /// the loop records what it executes, `audit` reads the tail.
    pub actions: ActionJournal,
    /// The config file path, for `reload`'s whole-file re-read. The reload
    /// decision happens here — synchronously — so the answer can name
    /// field-level truth before anything is queued.
    pub config_path: PathBuf,
}

/// The door's settings, split from the config file's `ControlConfig` so tests
/// can name their own caps (and port 0, an OS-picked port on the wire the
/// config's validator refuses for `ctl`-discovery reasons).
#[derive(Debug, Clone)]
pub struct ControlSettings {
    pub port: u16,
    pub max_connections: usize,
    pub max_line_bytes: usize,
    pub commands_per_second: u32,
    pub max_bad_tokens: u32,
    pub bad_token_cooldown_s: u64,
}

impl From<&crate::config::ControlConfig> for ControlSettings {
    fn from(c: &crate::config::ControlConfig) -> Self {
        Self {
            port: c.port,
            max_connections: c.max_connections,
            max_line_bytes: c.max_line_bytes,
            commands_per_second: c.commands_per_second,
            max_bad_tokens: c.max_bad_tokens,
            bad_token_cooldown_s: c.bad_token_cooldown_s,
        }
    }
}

/// The non-log events (connections, state changes) the main loop publishes
/// and a `watch` subscription forwards. A published value carries its own
/// `"event"` field — that field is the topic the forwarder filters by, so
/// the wire and the subscription list speak the same names.
#[derive(Clone)]
pub struct EventHub {
    tx: broadcast::Sender<Value>,
}

impl EventHub {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(256).0,
        }
    }

    /// Publishes to whoever is watching; nobody watching is free, not an
    /// error, and the caller's flow must not care.
    pub fn publish(&self, event: Value) {
        let _ = self.tx.send(event);
    }

    fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.tx.subscribe()
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

/// What `spawn` leaves the process holding: the bound address (an OS-picked
/// port 0 resolves here) and the event hub the main loop pushes to.
pub struct ControlDoor {
    pub events: EventHub,
    pub local_addr: SocketAddr,
}

/// The wrong-token counter behind the whole door (plan section 5, item 3):
/// `max_bad_tokens` consecutive failures pause authentication for the
/// cooldown. Counted door-wide, not per connection — a reconnected spray
/// must count on, or a patient script walks under the counter forever.
pub struct AuthGate {
    state: Mutex<GateState>,
    max_bad_tokens: u32,
    cooldown: Duration,
}

#[derive(Default)]
struct GateState {
    consecutive_failures: u32,
    /// `Some` while paused; the expiry is checked lazily — a paused gate
    /// unpauses itself the first time anything asks, no timer task and no
    /// drift.
    locked_until: Option<Instant>,
}

impl AuthGate {
    pub fn new(max_bad_tokens: u32, cooldown_s: u64) -> Self {
        Self {
            state: Mutex::new(GateState::default()),
            max_bad_tokens,
            cooldown: Duration::from_secs(cooldown_s),
        }
    }

    /// `true` while paused. The expiry check clears the flag when it notices
    /// time has passed — the same lock, reapplied from nothing.
    pub fn is_locked(&self) -> bool {
        let mut state = self.state.lock();
        if let Some(until) = state.locked_until {
            if Instant::now() < until {
                return true;
            }
            state.locked_until = None;
        }
        false
    }

    /// Records one evaluated hello. A success forgets the streak; a failure
    /// extends it, and tripping the cap starts the pause and zeroes the
    /// streak — the next window is judged on its own behaviour.
    pub fn admitted(&self, ok: bool) {
        let mut state = self.state.lock();
        if ok {
            state.consecutive_failures = 0;
            state.locked_until = None;
            return;
        }
        state.consecutive_failures += 1;
        if state.consecutive_failures >= self.max_bad_tokens {
            state.locked_until = Some(Instant::now() + self.cooldown);
            state.consecutive_failures = 0;
        }
    }
}

/// The command allowance (plan section 5, item 4): a bucket that refills at
/// `commands_per_second` continuously. A burst may spend the whole allowance
/// at once without ever making the server answer faster than the loop can
/// read — the cap exists to bound cost, not to schedule a metronome.
pub struct TokenBucket {
    tokens: f64,
    capacity: f64,
    last_refill: Instant,
}

impl TokenBucket {
    /// The bucket starts full: the first burst is the allowance the plan
    /// grants per second, not an extra privilege above it.
    pub fn new(commands_per_second: u32) -> Self {
        let capacity = f64::from(commands_per_second);
        Self {
            tokens: capacity,
            capacity,
            last_refill: Instant::now(),
        }
    }

    pub fn try_take(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;
        // The refill rate is the capacity per second — this struct exists
        // because the plan's "N commands per second" is a rate, and a rate is
        // what a bucket refills with.
        self.tokens = (self.tokens + elapsed * self.capacity).min(self.capacity);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Binds and serves the door. Runs for the process's lifetime — on exit the
/// runtime drops, which closes the listener. The event hub is built by the
/// caller and handed in: the main loop publishes connection events whether
/// or not the door was enabled (publishing to nobody is free), and the door
/// merely owns the subscribe end.
pub async fn spawn(
    token: String,
    settings: ControlSettings,
    env: ControlEnv,
    hub: EventHub,
) -> Result<ControlDoor> {
    let listener = TcpListener::bind((LISTEN_ADDR, settings.port))
        .await
        .with_context(|| {
            format!("binding the control listener on {LISTEN_ADDR}:{}", settings.port)
        })?;
    let local_addr = listener.local_addr().context("reading the control listener's address")?;
    if !local_addr.ip().is_loopback() {
        bail!(
            "the control listener bound {local_addr}, which is not the loopback — \
             the plan's first wall is the bind address and this run would be breaking it"
        );
    }
    info!("control: listening on {local_addr} ({} connection(s) cap)", settings.max_connections);

    let door = ControlDoor {
        events: hub.clone(),
        local_addr,
    };
    let gate = Arc::new(AuthGate::new(settings.max_bad_tokens, settings.bad_token_cooldown_s));
    let permits = Arc::new(Semaphore::new(settings.max_connections));

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    match permits.clone().try_acquire_owned() {
                        Ok(permit) => {
                            tokio::spawn(handle_connection(
                                stream,
                                peer,
                                token.clone(),
                                settings.clone(),
                                gate.clone(),
                                permit,
                                env.clone(),
                                hub.clone(),
                            ));
                        }
                        Err(_exhausted) => {
                            // The one line and the close: a fifth connection
                            // held open while three wait would let a client
                            // park on the door for nothing.
                            let text = json!({
                                "error": {
                                    "code": "rate_limited",
                                    "text": format!(
                                        "the connection allowance ({}) is exhausted; try again shortly",
                                        settings.max_connections
                                    ),
                                }
                            })
                            .to_string();
                            let answer = format!("{text}\n");
                            let mut stream = stream;
                            let _ = stream.write_all(answer.as_bytes()).await;
                            let _ = stream.shutdown().await;
                            warn!("control: refused a connection over the allowance from {peer}");
                        }
                    }
                }
                Err(error) => {
                    // An accept error is logged and survived: binding failed
                    // once, and the door that already bound should not quit
                    // because one accept call tripped.
                    warn!("control: accept failed: {error}");
                }
            }
        }
    });

    Ok(door)
}

/// What reading a line can hit. `TooLong` is answered and then closed — the
/// connection that showed it cannot follow the framing has earned nothing
/// more than the refusal.
enum LineError {
    Io,
    TooLong,
}

/// Reads one newline-terminated line, bounded by `max_line_bytes`. There is
/// no unbounded read anywhere in this file: a read that outgrows the cap is
/// an error, never a growing buffer.
async fn read_line_capped(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    max_line_bytes: usize,
) -> Result<Option<String>, LineError> {
    let mut buffer: Vec<u8> = Vec::with_capacity(128);
    let mut chunk = [0u8; 4096];
    loop {
        let read = reader
            .read(&mut chunk)
            .await
            .map_err(|_| LineError::Io)?;
        if read == 0 {
            // EOF: a clean close is None; anything accumulated arrives as an
            // unterminated line and is parsed downstream (a malformed remnant
            // then answers bad_arguments — the "обрыв посреди строки" case).
            return Ok(if buffer.is_empty() {
                None
            } else {
                Some(String::from_utf8_lossy(&buffer).into_owned())
            });
        }
        if let Some(newline) = chunk[..read].iter().position(|byte| *byte == b'\n') {
            buffer.extend_from_slice(&chunk[..newline]);
            if buffer.len() > max_line_bytes {
                return Err(LineError::TooLong);
            }
            return Ok(Some(String::from_utf8_lossy(&buffer).into_owned()));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > max_line_bytes {
            return Err(LineError::TooLong);
        }
    }
}

/// One connection, from hello to close. The permit stays held for the whole
/// connection — the crate above counts *served* connections, not authed ones.
#[allow(clippy::too_many_arguments)]
async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    token: String,
    settings: ControlSettings,
    gate: Arc<AuthGate>,
    permit: tokio::sync::OwnedSemaphorePermit,
    env: ControlEnv,
    hub: EventHub,
) {
    let _permit = permit; // lives for the connection
    let (mut reader, writer) = stream.into_split();
    let (line_tx, line_rx) = mpsc::channel::<String>(256);
    let writer_task = tokio::spawn(writer_loop(writer, line_rx));

    macro_rules! refuse {
        ($code:expr, $text:expr) => {{
            let line = json!({ "error": { "code": $code, "text": $text } }).to_string();
            let _ = line_tx.send(line + "\n").await;
        }};
    }

    // ---- the hello ----
    let first = match read_line_capped(&mut reader, settings.max_line_bytes).await {
        Ok(Some(line)) => line,
        Ok(None) => return finish(writer_task, line_tx).await,
        Err(LineError::TooLong) => {
            refuse!(
                "bad_arguments",
                format!(
                    "the hello line exceeds {} bytes; nothing larger is read on this door",
                    settings.max_line_bytes
                )
            );
            return finish(writer_task, line_tx).await;
        }
        Err(LineError::Io) => return finish(writer_task, line_tx).await,
    };
    match proto::parse_line(&first) {
        Ok(proto::Incoming::Hello(hello)) => {
            if gate.is_locked() {
                refuse!(
                    "unauthorized",
                    format!(
                        "authentication is paused after {max} wrong token(s); \
                         try again within {s} s",
                        max = settings.max_bad_tokens,
                        s = settings.bad_token_cooldown_s
                    )
                );
                return finish(writer_task, line_tx).await;
            }
            if hello.proto != proto::CONTROL_PROTO {
                // Version mismatch is its own refusal: an operator pointing a
                // v2 tool at a v1 door deserves the words, not a generic no.
                refuse!(
                    "unauthorized",
                    format!(
                        "the connection speaks control proto {}; this server speaks {}",
                        hello.proto,
                        proto::CONTROL_PROTO
                    )
                );
                gate.admitted(false);
                return finish(writer_task, line_tx).await;
            }
            // The compare itself: a straightforward `==` — the door is bound
            // to the loopback and rate-paused after repeats, so the timing
            // side channel the fancy compares defend against has no seat
            // here. If the plan ever opens the door past the loopback, this
            // is the line that has to change with it.
            if hello.token == token {
                gate.admitted(true);
                info!("control: {peer} authenticated");
                let ack = json!({ "hello": "ok", "control_proto": proto::CONTROL_PROTO });
                let _ = line_tx.send(ack.to_string() + "\n").await;
            } else {
                // The text names the door, never the attempted token.
                refuse!(
                    "unauthorized",
                    "invalid control token — the token lives in the file the \
                     server's log named at first start"
                );
                gate.admitted(false);
                return finish(writer_task, line_tx).await;
            }
        }
        Ok(proto::Incoming::Command(_)) => {
            refuse!(
                "unauthorized",
                "the connection must open with the hello before any command"
            );
            return finish(writer_task, line_tx).await;
        }
        Err(ParseError::Malformed(detail)) => {
            refuse!(
                "bad_arguments",
                format!("the hello is not a v1 hello: {detail}")
            );
            return finish(writer_task, line_tx).await;
        }
        Err(ParseError::NotJson) => {
            refuse!("bad_arguments", "the hello is not JSON");
            return finish(writer_task, line_tx).await;
        }
        Err(ParseError::Empty) => {
            refuse!("bad_arguments", "the hello is an empty line");
            return finish(writer_task, line_tx).await;
        }
    }

    // ---- the command loop ----
    let mut bucket = TokenBucket::new(settings.commands_per_second);
    let mut watchers: Vec<tokio::task::AbortHandle> = Vec::new();
    loop {
        let line = match read_line_capped(&mut reader, settings.max_line_bytes).await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(LineError::TooLong) => {
                refuse!(
                    "bad_arguments",
                    format!(
                        "the line exceeds {} bytes; the connection is closed",
                        settings.max_line_bytes
                    )
                );
                break;
            }
            Err(LineError::Io) => break,
        };
        // The rate limiter walks in front of every parsed command: a storm of
        // garbage lines is exactly the shape the cap must stop, and a storm
        // of valid ones should not get a cheaper ride through parsing.
        if !bucket.try_take() {
            let response = json!({
                "error": {
                    "code": "rate_limited",
                    "text": format!(
                        "past the {} command(s)/s allowance; slow down",
                        settings.commands_per_second
                    ),
                }
            });
            if line_tx.send(response.to_string() + "\n").await.is_err() {
                break;
            }
            continue;
        }
        let response = match proto::parse_line(&line) {
            Err(ParseError::NotJson) => {
                json!({
                    "error": { "code": "bad_arguments", "text": "the line is not JSON" }
                })
            }
            Err(ParseError::Malformed(detail)) => {
                json!({
                    "error": { "code": "bad_arguments", "text": format!("the command is not a v1 command: {detail}") }
                })
            }
            Err(ParseError::Empty) => continue,
            Ok(proto::Incoming::Hello(_)) => {
                json!({
                    "error": { "code": "bad_arguments", "text": "the connection is already authenticated; hello once per connection" }
                })
            }
            Ok(proto::Incoming::Command(command)) => {
                answer_command(command, &env, &hub, &mut watchers, &line_tx).await
            }
        };
        if line_tx.send(response.to_string() + "\n").await.is_err() {
            break;
        }
    }
    for watcher in watchers.drain(..) {
        watcher.abort();
    }
    finish(writer_task, line_tx).await;
}

/// Wraps a command body's answer so the loop above stays flat. Mutating
/// nothing, it is the plan's read path; `watch` is the one reply that also
/// starts a feed (its `ok` rides the same ordering as every other response:
/// FIFO through the one writer channel — the `ok` cannot overtake or lag a
/// queued event behind its own connection).
async fn answer_command(
    command: proto::Command,
    env: &ControlEnv,
    hub: &EventHub,
    watchers: &mut Vec<tokio::task::AbortHandle>,
    line_tx: &mpsc::Sender<String>,
) -> Value {
    let request = match commands::parse_request(&command.cmd, command.args.as_ref()) {
        Ok(request) => request,
        Err((code, text)) => return refusal_body(command.id, code, &text),
    };
    match request {
        // Stage 2's mutating half: gates against the door's snapshot, the
        // reload's whole-file read, then one order into the queue. The
        // answer names what the door did itself, not what the loop will do.
        Request::Say { .. } | Request::Kick { .. } | Request::Stop { .. } | Request::Reload => {
            mutating_answer(command.id, request, env).await
        }
        _ => match commands::dispatch(
            &request,
            &env.status.snapshot(),
            &env.ring,
            &env.config.read().clone(),
            &env.actions,
        ) {
            Err((code, text)) => refusal_body(command.id, code, &text),
            Ok(Reply::Data(data)) => json!({
                "id": command.id,
                "ok": true,
                "data": data,
            }),
            Ok(Reply::Watch { topics }) => {
                watchers.push(spawn_watcher(&topics, &env.ring, hub, line_tx.clone()));
                json!({
                    "id": command.id,
                    "ok": true,
                    "data": { "topics": topics },
                })
            }
        },
    }
}

/// The stage-2 mutating path. Gates first (the snapshot's state word, the
/// players listing for a kick), the reload's file decision next, and one
/// journal line per attempt — a refused attempt is as real an entry in the
/// action history as a queued one. The order into the queue is the LAST
/// step: an order the journal never named leaves no trail anyone could
/// read back.
async fn mutating_answer(id: u64, request: Request, env: &ControlEnv) -> Value {
    let journal = |command: &str, outcome: &str| {
        env.actions.record("door", command, outcome);
    };

    // The state gate: only the loop's `running` accepts changes.
    let snapshot = env.status.snapshot();
    if let Err((code, text)) = commands::gate_state(&snapshot.state) {
        journal(&describe(&request), &format!("refused: {code}"));
        return refusal_body(id, code, &text);
    }

    match request {
        Request::Say { text } => {
            // The queue is bounded; a send waits, so a wedged loop surfaces
            // as a slow door, not as a lie about what got queued.
            match env.queue.send(ControlOrder::Say { text: text.clone() }).await {
                Ok(()) => {
                    journal(&format!("say {}", quote_text(&text)), "queued");
                    json!({ "id": id, "ok": true, "data": { "queued": true, "bytes": text.len() } })
                }
                Err(_) => {
                    journal(&describe(&Request::Say { text }), "refused: the queue is not served");
                    refusal_body(id, "internal", "the command queue is not being served; the match is not ticking")
                }
            }
        }
        Request::Kick { player, reason } => {
            if let Err((code, text)) = commands::gate_kick_target(player, &snapshot.player_list) {
                journal(&describe(&Request::Kick { player, reason }), &format!("refused: {code}"));
                return refusal_body(id, code, &text);
            }
            match env
                .queue
                .send(ControlOrder::Kick { connection_id: player, reason: reason.clone() })
                .await
            {
                Ok(()) => {
                    journal(
                        &format!("kick {player} {}", quote_text(&reason)),
                        "queued",
                    );
                    json!({ "id": id, "ok": true, "data": { "queued": true } })
                }
                Err(_) => {
                    journal(&format!("kick {player}"), "refused: the queue is not served");
                    refusal_body(id, "internal", "the command queue is not being served; the match is not ticking")
                }
            }
        }
        Request::Stop { delay_s } => {
            match env.queue.send(ControlOrder::Stop { delay_s }).await {
                Ok(()) => {
                    journal(
                        &format!("stop {delay_s}"),
                        if delay_s == 0 { "queued; the loop stops immediately" } else { "queued; the loop announces 60/30/10 s" },
                    );
                    json!({ "id": id, "ok": true, "data": { "queued": true, "delay_s": delay_s } })
                }
                Err(_) => {
                    journal("stop", "refused: the queue is not served");
                    refusal_body(id, "internal", "the command queue is not being served; the match is not ticking")
                }
            }
        }
        Request::Reload => {
            // The whole-file decision (А.2 item 5): missing file — refuse
            // (reload never creates one; `MatchConfig::load` would), broken
            // file — refuse with the field the validator named, valid file —
            // diff and queue the live-applyable difference.
            if !env.config_path.exists() {
                journal("reload", "refused: the config file is missing");
                return refusal_body(
                    id,
                    "bad_arguments",
                    &format!(
                        "the config file {} is missing; reload refuses to create one in its place",
                        env.config_path.display()
                    ),
                );
            }
            let file = match MatchConfig::load(&env.config_path) {
                Ok(file) => file,
                Err(error) => {
                    journal("reload", &format!("refused: {error:#}"));
                    return refusal_body(
                        id,
                        "bad_arguments",
                        &format!(
                            "reload refused the file; the server keeps running on the old settings: {error:#}"
                        ),
                    );
                }
            };
            let running = env.config.read().clone();
            let decision = commands::reload_decide(&running, &env.auth_token, &file);
            if let Some(vote_kick) = &decision.vote_kick {
                if let Err(error) = env.queue.send(ControlOrder::ReloadVoteKick { vote_kick: vote_kick.clone() }).await {
                    journal("reload", &format!("refused: the queue is not served: {error}"));
                    return refusal_body(id, "internal", "the command queue is not being served; the match is not ticking");
                }
            }
            let summary = format!(
                "applied={} restart_required={} rejected=0",
                decision.applied.len(),
                decision.restart_required.len()
            );
            journal("reload", &summary);
            json!({
                "id": id,
                "ok": true,
                "data": {
                    "applied": decision.applied,
                    "restart_required": decision.restart_required,
                    "rejected": [],
                    "note": if decision.vote_kick.is_some() { "vote_kick applies at the next tick; the rest waits for a restart" } else { "nothing live-applyable; nothing queued" },
                },
            })
        }
        // The read commands cannot arrive here — the mutating variants were
        // all destructured above, so `_` here serves exactly the reads.
        _ => refusal_body(id, "internal", "a read command reached the mutating path"),
    }
}

/// The journal's `command` field keeps the arguments the operator typed —
/// visible arguments only; nothing here is a secret, everything here is
/// what a scan needs to reconstruct the decision.
fn quote_text(text: &str) -> String {
    format!("{text:?}")
}

/// The same one-line description for the paths that refused before quoting
/// anything.
fn describe(request: &Request) -> String {
    match request {
        Request::Say { text } => format!("say {}", quote_text(text)),
        Request::Kick { player, reason } => format!("kick {player} {}", quote_text(reason)),
        Request::Stop { delay_s } => format!("stop {delay_s}"),
        Request::Reload => "reload".into(),
        other => format!("{other:?}"),
    }
}

/// The one refusal shape, so the three refusal paths cannot drift apart: the
/// id rides when a command named one, `ok` is `false`, the error carries the
/// fixed code and its text.
fn refusal_body(id: u64, code: &str, text: &str) -> Value {
    json!({
        "id": id,
        "ok": false,
        "error": { "code": code, "text": text },
    })
}

/// The per-connection event forwarder: ring and hub receipts fall into one
/// selector, filtered by the subscribed topics. A lagged subscriber is told
/// how much it missed — the gap is a line of its own, never silence.
fn spawn_watcher(
    topics: &[String],
    ring: &LogRing,
    hub: &EventHub,
    line_tx: mpsc::Sender<String>,
) -> tokio::task::AbortHandle {
    let topics: HashSet<String> = topics.iter().cloned().collect();
    let mut ring_rx = ring.subscribe();
    let mut hub_rx = hub.subscribe();
    tokio::spawn(async move {
        // A nested fn, not a closure: the send is awaited at the call site,
        // which keeps the future's borrows local and the loop's arms flat.
        async fn push(
            tx: &mpsc::Sender<String>,
            value: &Value,
        ) -> Result<(), mpsc::error::SendError<String>> {
            let text = value.to_string() + "\n";
            tx.send(text).await
        }
        loop {
            tokio::select! {
                log = ring_rx.recv() => {
                    match log {
                        Ok(event) => {
                            if !topics.contains("log") {
                                continue;
                            }
                            let wire = json!({
                                "event": "log",
                                "level": event.level,
                                "text": event.text,
                            });
                            if push(&line_tx, &wire).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(missed)) => {
                            let wire = json!({ "event": "lagged", "missed": missed });
                            if push(&line_tx, &wire).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                event = hub_rx.recv() => {
                    match event {
                        Ok(value) => {
                            let topic = value.get("event").and_then(Value::as_str);
                            if topic.is_some_and(|t| topics.contains(t))
                                && push(&line_tx, &value).await.is_err()
                            {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(missed)) => {
                            let wire = json!({ "event": "lagged", "missed": missed });
                            if push(&line_tx, &wire).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    })
    .abort_handle()
}

/// Runs the writer to its end and joins it. Every early-exit path in
/// `handle_connection` funnels here, so a connection can neither leak a
/// writer task nor double-shutdown the stream.
async fn finish(writer_task: tokio::task::JoinHandle<()>, line_tx: mpsc::Sender<String>) {
    drop(line_tx);
    let _ = writer_task.await;
}

/// The write half's own loop: one line, one write, flushed per line — the
/// panel reads line by line and an unflushed queue would look like a hang.
async fn writer_loop(
    mut writer: tokio::net::tcp::OwnedWriteHalf,
    mut line_rx: mpsc::Receiver<String>,
) {
    while let Some(line) = line_rx.recv().await {
        if writer.write_all(line.as_bytes()).await.is_err() {
            break;
        }
        let _ = writer.flush().await;
    }
    let _ = writer.shutdown().await;
}

#[cfg(test)]
mod door_tests {
    use super::*;
    use crate::config::{ControlConfig, VoteKickConfig};
    use crate::control::ring::DEFAULT_RING_CAP;

    /// The shared token the tests use: 64 hex chars, the length the
    /// generator itself would have picked.
    const TEST_TOKEN: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";

    fn env() -> (
        StatusHandle,
        LogRing,
        ControlEnv,
        tokio::sync::mpsc::Receiver<ControlOrder>,
    ) {
        let status = StatusHandle::new(fixture_status());
        let (layer, ring) = crate::control::ring::RingLayer::new(DEFAULT_RING_CAP);
        // The layer exists so ring.record's caller surface (the tracing path)
        // is wired the same way production wires it: the same handle the
        // layer pushes to is the one tests record through.
        let _ = layer;
        let config = RedactedConfig {
            auth_token_set: true,
            allow_unsigned: false,
            vote_kick: VoteKickConfig::default(),
            control: ControlConfig::default(),
        };
        let actions_path = temp_journal_path();
        let actions = ActionJournal::open(&actions_path).expect("the journal opens");
        let (control_tx, control_rx) = tokio::sync::mpsc::channel(super::QUEUE_CAP);
        (
            status.clone(),
            ring.clone(),
            ControlEnv {
                status,
                ring,
                config: Arc::new(RwLock::new(config)),
                auth_token: Some("fixture-secret".into()),
                queue: control_tx,
                actions,
                config_path: fixture_config_path(),
            },
            control_rx,
        )
    }

    /// A journal under this test run's own temp dir: every env() gets a
    /// fresh file, and a ring leak between tests cannot pass for a rule.
    fn temp_journal_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rfs_server_door_tests_journal_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock is before epoch")
                .as_nanos()
        ))
    }

    /// A reload target that does not exist: the reload tests that want a
    /// missing/broken file point a fresh ControlEnv at a real path; the
    /// default here only needs to be non-existent-tolerant.
    fn fixture_config_path() -> PathBuf {
        PathBuf::from("untested-reload-target.json")
    }

    fn fixture_status() -> crate::control::status::ServerStatus {
        let mut status = crate::control::status::ServerStatus::starting(
            "fixture-match".into(),
            "127.0.0.1:7777".into(),
            7777,
        );
        status.tick = 18450;
        status.uptime_s = 615;
        status.players = 2;
        status.ships = 4;
        status
    }

    async fn spawn_door(
        settings: ControlSettings,
    ) -> (ControlDoor, LogRing, StatusHandle) {
        let (status, ring, env, _control_rx) = env();
        let door = spawn(
            TEST_TOKEN.to_string(),
            settings,
            env,
            EventHub::new(),
        )
        .await
        .expect("the door binds");
        (door, ring, status)
    }

    async fn connect(port: u16) -> TcpStream {
        TcpStream::connect((LISTEN_ADDR, port))
            .await
            .expect("the door accepts")
    }

    async fn send_line(stream: &mut TcpStream, line: &str) {
        stream
            .write_all(format!("{line}\n").as_bytes())
            .await
            .expect("the line is written");
        stream.flush().await.expect("the line is flushed");
    }

    async fn read_line(stream: &mut TcpStream) -> Option<String> {
        let mut buffer = Vec::with_capacity(256);
        let mut chunk = [0u8; 4096];
        loop {
            let read = stream.read(&mut chunk).await.expect("the read works");
            if read == 0 {
                return if buffer.is_empty() {
                    None
                } else {
                    Some(String::from_utf8_lossy(&buffer).into_owned())
                };
            }
            if let Some(newline) = chunk[..read].iter().position(|byte| *byte == b'\n') {
                buffer.extend_from_slice(&chunk[..newline]);
                return Some(String::from_utf8_lossy(&buffer).into_owned());
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
    }

    async fn send_command(stream: &mut TcpStream, body: serde_json::Value) {
        let line = body.to_string();
        send_line(stream, &line).await;
    }

    /// A port the OS just proved free: bind :0 on the loopback, read the
    /// number, drop the listener. Two tests cannot collide on it unless the
    /// OS itself hands the same ephemeral port twice inside one release —
    /// which is the soundest base available.
    fn free_port() -> u16 {
        std::net::TcpListener::bind((LISTEN_ADDR, 0))
            .expect("an ephemeral local port binds")
            .local_addr()
            .expect("the listener knows its address")
            .port()
    }

    fn hello_line() -> String {
        format!(r#"{{"hello":{{"token":"{TEST_TOKEN}","proto":1}}}}"#)
    }

    async fn auth(port: u16) -> TcpStream {
        let mut stream = connect(port).await;
        send_line(&mut stream, &hello_line()).await;
        let ack = read_line(&mut stream).await.expect("the hello answers");
        let ack: Value = serde_json::from_str(&ack).expect("the hello ack is JSON");
        assert_eq!(ack["hello"], "ok", "the hello must ack: {ack}");
        assert_eq!(ack["control_proto"], proto::CONTROL_PROTO);
        stream
    }

    fn default_test_settings(port: u16) -> ControlSettings {
        ControlSettings {
            port,
            max_connections: 3,
            max_line_bytes: 64 * 1024,
            commands_per_second: 20,
            max_bad_tokens: 5,
            bad_token_cooldown_s: 60,
        }
    }

    // ---- the real-socket test the plan demands once (section 10.2) ----

    #[tokio::test]
    async fn a_hello_a_command_an_answer_and_a_close() {
        let port = free_port();
        let (door, _ring, status) = spawn_door(default_test_settings(port)).await;
        assert!(door.local_addr.ip().is_loopback(), "the bind is the loopback's");

        let mut stream = auth(port).await;
        send_command(
            &mut stream,
            json!({"id":1,"cmd":"status"}),
        )
        .await;
        let answer = read_line(&mut stream).await.expect("the status answers");
        let answer: Value = serde_json::from_str(&answer).expect("the answer is JSON");
        assert_eq!(answer["id"], 1);
        assert_eq!(answer["ok"], true);
        assert_eq!(answer["data"]["players"], status.snapshot().players);
        assert_eq!(answer["data"]["tick"], 18450);
        drop(stream);
    }

    // ---- the negative set (section 10.3) ----

    #[tokio::test]
    async fn a_foreign_token_is_refused_and_the_door_survives() {
        let port = free_port();
        let (door, _ring, _status) = spawn_door(default_test_settings(port)).await;
        let _ = door; // the door's survival is what the later conn proves

        let mut stream = connect(port).await;
        send_line(
            &mut stream,
            r#"{"hello":{"token":"00000000000000000000000000000000","proto":1}}"#,
        )
        .await;
        let refusal = read_line(&mut stream).await.expect("the refusal answers");
        let refusal: Value = serde_json::from_str(&refusal).expect("the refusal is JSON");
        assert_eq!(refusal["error"]["code"], "unauthorized", "{refusal}");
        assert!(
            !refusal.to_string().contains(TEST_TOKEN),
            "the refusal must not carry the real token: {refusal}"
        );
        // The wrong token must not come back either — the echo rule.
        assert!(
            !refusal.to_string().contains("0000000000000000"),
            "the refusal must not echo the attempted token: {refusal}"
        );
        // The server closes after the refusal; the socket's read ends.
        assert_eq!(
            read_line(&mut stream).await,
            None,
            "a refused connection is closed by the server"
        );

        // A fresh connection with the right token still works — one wrong
        // hello is not the day's quota spent.
        let mut stream = auth(port).await;
        send_command(&mut stream, json!({"id":2,"cmd":"status"})).await;
        let answer = read_line(&mut stream).await.expect("the status answers");
        assert!(answer.contains("\"ok\":true"), "{answer}");
    }

    #[tokio::test]
    async fn a_hello_without_the_token_is_refused_with_the_field_named() {
        let port = free_port();
        let (door, _ring, _status) = spawn_door(default_test_settings(port)).await;
        let _ = door;

        let mut stream = connect(port).await;
        send_line(&mut stream, r#"{"hello":{"proto":1}}"#).await;
        let refusal = read_line(&mut stream).await.expect("the refusal answers");
        assert!(
            refusal.contains("token"),
            "the refusal must name the missing field: {refusal}"
        );
        assert!(refusal.contains("bad_arguments"), "{refusal}");
    }

    #[tokio::test]
    async fn repeated_wrong_tokens_pause_the_door_until_the_cooldown_passes() {
        let port = free_port();
        let (door, _ring, _status) = spawn_door(ControlSettings {
            max_bad_tokens: 3,
            bad_token_cooldown_s: 1,
            ..default_test_settings(port)
        })
        .await;
        let _ = door;

        for _ in 0..3 {
            let mut stream = connect(port).await;
            send_line(
                &mut stream,
                r#"{"hello":{"token":"00000000000000000000000000000000","proto":1}}"#,
            )
            .await;
            let refusal = read_line(&mut stream).await.expect("each failure answers");
            assert!(refusal.contains("unauthorized"), "{refusal}");
        }
        // Three failures later the door pauses: even the CORRECT token is
        // turned away, and the text says why.
        let mut stream = connect(port).await;
        send_line(&mut stream, &hello_line()).await;
        let refusal = read_line(&mut stream).await.expect("the paused door answers");
        let refusal: Value = serde_json::from_str(&refusal).expect("the refusal is JSON");
        assert_eq!(refusal["error"]["code"], "unauthorized", "{refusal}");
        assert!(
            refusal["error"]["text"]
                .as_str()
                .expect("text is present")
                .contains("paused"),
            "the pause must be explained: {refusal}"
        );

        // The pause lifts after its cooldown, without a timer task: the next
        // ask sees the clock move.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let mut stream = connect(port).await;
        send_line(&mut stream, &hello_line()).await;
        let ack = read_line(&mut stream).await.expect("the hello answers");
        assert!(
            ack.contains("\"hello\":\"ok\""),
            "after the cooldown a correct hello works: {ack}"
        );
    }

    #[tokio::test]
    async fn an_overlong_line_is_refused_without_killing_the_door() {
        let port = free_port();
        let (door, _ring, _status) = spawn_door(ControlSettings {
            max_line_bytes: 256,
            ..default_test_settings(port)
        })
        .await;
        let _ = door;

        let mut stream = connect(port).await;
        let long = format!(r#"not json "{}""#, "x".repeat(600));
        send_line(&mut stream, &long).await;
        let refusal = read_line(&mut stream).await.expect("the cap answers");
        assert!(refusal.contains("bad_arguments"), "{refusal}");
        assert!(refusal.contains("256"), "the refusal names the cap: {refusal}");
        assert_eq!(
            read_line(&mut stream).await,
            None,
            "the capped connection is closed"
        );

        // The door itself is alive: a fresh, small hello works.
        let mut stream = auth(port).await;
        send_command(&mut stream, json!({"id":5,"cmd":"status"})).await;
        let answer = read_line(&mut stream).await.expect("the status answers");
        assert!(answer.contains("\"ok\":true"), "{answer}");
    }

    #[tokio::test]
    async fn a_line_over_the_cap_during_commands_closes_only_that_connection() {
        let port = free_port();
        let (door, _ring, _status) = spawn_door(ControlSettings {
            max_line_bytes: 128,
            ..default_test_settings(port)
        })
        .await;
        let _ = door;

        let mut stream = auth(port).await;
        let long = format!(r#"{{"id":9,"cmd":"log","args":{{"tail":{} }}}}"#, "9".repeat(200));
        send_line(&mut stream, &long).await;
        let refusal = read_line(&mut stream).await.expect("the cap answers");
        assert!(refusal.contains("bad_arguments"), "{refusal}");
        assert_eq!(read_line(&mut stream).await, None, "the connection closes");
    }

    #[tokio::test]
    async fn the_connection_allowance_is_held_and_released() {
        let port = free_port();
        let (door, _ring, _status) = spawn_door(ControlSettings {
            max_connections: 2,
            ..default_test_settings(port)
        })
        .await;
        let _ = door;

        let first = auth(port).await;
        let second = auth(port).await;
        let mut third = connect(port).await;
        send_line(&mut third, &hello_line()).await;
        let busy = read_line(&mut third).await.expect("the busy line answers");
        let busy: Value = serde_json::from_str(&busy).expect("the busy line is JSON");
        assert_eq!(busy["error"]["code"], "rate_limited", "{busy}");
        assert_eq!(
            read_line(&mut third).await,
            None,
            "the excess connection is closed"
        );

        // Free one and the door serves again: the allowance is a count of
        // live connections, not of the day's attempts.
        drop(first);
        // The real retry loop: give the server's read loop a beat to notice
        // the closed socket, then authenticate a new connection. A missed
        // release would show up as this poll never succeeding.
        let mut stream = None;
        for attempt in 0..40 {
            let mut candidate = connect(port).await;
            send_line(&mut candidate, &hello_line()).await;
            if let Some(ack) = read_line(&mut candidate).await {
                if ack.contains("\"hello\":\"ok\"") {
                    stream = Some(candidate);
                    break;
                }
            }
            drop(candidate);
            if attempt == 39 {
                panic!("the freed permit never served a new connection");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let mut stream = stream.expect("a freed permit serves a new connection");
        send_command(&mut stream, json!({"id":7,"cmd":"status"})).await;
        let answer = read_line(&mut stream).await.expect("the status answers");
        assert!(answer.contains("\"ok\":true"), "{answer}");
        drop(second);
    }

    #[tokio::test]
    async fn the_command_rate_is_capped_per_connection() {
        let port = free_port();
        let (door, _ring, _status) = spawn_door(ControlSettings {
            commands_per_second: 2,
            ..default_test_settings(port)
        })
        .await;
        let _ = door;

        let mut stream = auth(port).await;
        // Two commands, one right after the other: the bucket holds exactly
        // the allowance the plan grants, and it starts full.
        for id in 1..=2 {
            send_command(&mut stream, json!({"id":id,"cmd":"status"})).await;
            let answer = read_line(&mut stream).await.expect("the command answers");
            assert!(answer.contains("\"ok\":true"), "cmd {id}: {answer}");
        }
        // The third immediately: refused, and the connection lives.
        send_command(&mut stream, json!({"id":3,"cmd":"status"})).await;
        let answer = read_line(&mut stream).await.expect("the third answers");
        let answer: Value = serde_json::from_str(&answer).expect("the answer is JSON");
        assert_eq!(answer["error"]["code"], "rate_limited", "{answer}");

        // A pause refills the bucket: the same connection answers again.
        tokio::time::sleep(Duration::from_millis(700)).await;
        send_command(&mut stream, json!({"id":4,"cmd":"status"})).await;
        let answer = read_line(&mut stream).await.expect("the fourth answers");
        assert!(answer.contains("\"ok\":true"), "{answer}");
    }

    #[tokio::test]
    async fn unknown_commands_and_bad_json_refuse_but_the_connection_survives() {
        let port = free_port();
        let (door, _ring, _status) = spawn_door(default_test_settings(port)).await;
        let _ = door;

        let mut stream = auth(port).await;
        send_command(&mut stream, json!({"id":9,"cmd":"explode"})).await;
        let refusal = read_line(&mut stream).await.expect("the unknown answers");
        let refusal: Value = serde_json::from_str(&refusal).expect("the refusal is JSON");
        assert_eq!(refusal["error"]["code"], "unknown_command", "{refusal}");
        assert!(refusal["error"]["text"].as_str().unwrap_or("").contains("status"));
        assert_eq!(refusal["id"], 9, "the id rides refusals too: {refusal}");

        // Malformed JSON: bad_arguments, and the loop keeps reading.
        send_line(&mut stream, "definitely not json").await;
        let refusal = read_line(&mut stream).await.expect("the malformed answers");
        assert!(refusal.contains("bad_arguments"), "{refusal}");

        // A second real command then works on the same connection.
        send_command(&mut stream, json!({"id":10,"cmd":"status"})).await;
        let answer = read_line(&mut stream).await.expect("the status answers");
        assert!(answer.contains("\"ok\":true"), "{answer}");
    }

    // ---- watch ----

    #[tokio::test]
    async fn a_watch_subscription_streams_the_ring_forward() {
        let port = free_port();
        let (door, ring, _status) = spawn_door(default_test_settings(port)).await;

        let mut stream = auth(port).await;
        send_command(&mut stream, json!({"id":11,"cmd":"watch"})).await;
        let ack = read_line(&mut stream).await.expect("the watch acks");
        assert!(ack.contains("\"ok\":true"), "{ack}");
        assert!(ack.contains("topics"), "{ack}");

        // An event recorded into the SAME ring the door serves reaches the
        // subscription — the tracing layer's own path in production, called
        // from the test through its handle.
        ring.record(&tracing::Level::INFO, "a line for the watcher");
        let event = read_line(&mut stream).await.expect("the event arrives");
        let event: Value = serde_json::from_str(&event).expect("the event is JSON");
        assert_eq!(event["event"], "log", "{event}");
        assert_eq!(event["level"], "INFO");
        assert_eq!(event["text"], "a line for the watcher");
        // And the refusal set's shape survived: the door still serves.
        send_command(&mut stream, json!({"id":12,"cmd":"status"})).await;
        let answer = read_line(&mut stream).await.expect("the status answers");
        assert!(answer.contains("\"ok\":true"), "{answer}");
        let _ = door;
    }

    #[tokio::test]
    async fn an_unknown_watch_topic_refuses_with_the_list_named() {
        let port = free_port();
        let (door, _ring, _status) = spawn_door(default_test_settings(port)).await;
        let _ = door;

        let mut stream = auth(port).await;
        send_command(
            &mut stream,
            json!({"id":13,"cmd":"watch","args":{"topics":["battle"]}}),
        )
        .await;
        let refusal = read_line(&mut stream).await.expect("the topic refuses");
        let refusal: Value = serde_json::from_str(&refusal).expect("the refusal is JSON");
        assert_eq!(refusal["error"]["code"], "bad_arguments", "{refusal}");
        assert!(
            refusal["error"]["text"]
                .as_str()
                .unwrap_or("")
                .contains("log"),
            "the refusal names the known topics: {refusal}"
        );
    }

    // ---- the token never resurfaces ----

    #[tokio::test]
    async fn the_token_never_appears_in_answers_or_in_the_ring() {
        let port = free_port();
        let (door, ring, _status) = spawn_door(default_test_settings(port)).await;
        let _ = door;

        let mut stream = auth(port).await;
        for (id, cmd) in [(20, "status"), (21, "perf"), (22, "net"), (23, "config")] {
            send_command(&mut stream, json!({"id":id,"cmd":cmd})).await;
            let answer = read_line(&mut stream).await.expect("the command answers");
            assert!(
                !answer.contains(TEST_TOKEN),
                "command {cmd}'s answer carries the token: {answer}"
            );
        }
        // Every journal line the ring holds: the door's own logs included.
        let held: Vec<String> = ring
            .snapshot_tail(10_000, None)
            .iter()
            .map(|event| event.text.clone())
            .collect();
        for text in held {
            assert!(
                !text.contains(TEST_TOKEN),
                "the ring carries the token: {text}"
            );
        }
    }

    // ---- the gates, without sockets ----

    #[test]
    fn the_auth_gate_pauses_after_the_cap_and_reopens_after_the_cooldown() {
        let gate = AuthGate::new(3, 1);
        gate.admitted(false);
        gate.admitted(false);
        assert!(!gate.is_locked(), "two failures do not pause");
        gate.admitted(false);
        assert!(gate.is_locked(), "the cap's third failure pauses");
        std::thread::sleep(Duration::from_millis(1100));
        assert!(!gate.is_locked(), "the cooldown unpauses without a timer");
        // A success cleans the counter outright.
        gate.admitted(false);
        gate.admitted(false);
        gate.admitted(true);
        gate.admitted(false);
        assert!(!gate.is_locked(), "one failure after a success is a fresh streak");
    }

    #[test]
    fn the_bucket_grants_a_burst_of_the_allowance_then_refills() {
        let mut bucket = TokenBucket::new(2);
        assert!(bucket.try_take(), "a full bucket spends the first");
        assert!(bucket.try_take(), "and the second");
        assert!(!bucket.try_take(), "a third immediate command is past the rate");
        std::thread::sleep(Duration::from_millis(700));
        assert!(bucket.try_take(), "700 ms at 2/s refills the fourth");
        assert!(!bucket.try_take(), "and the fifth hits the same wall");
    }

    #[test]
    fn spawn_settings_come_from_the_config_untouched() {
        // The one piece of glue between the config file and the door: a
        // default ControlConfig becomes the same numbers at the door. A knob
        // translating itself silently is how a port change becomes a mystery.
        let config = ControlConfig::default();
        let settings = ControlSettings::from(&config);
        assert_eq!(settings.port, config.port);
        assert_eq!(settings.max_connections, config.max_connections);
        assert_eq!(settings.max_line_bytes, config.max_line_bytes);
        assert_eq!(settings.commands_per_second, config.commands_per_second);
        assert_eq!(settings.max_bad_tokens, config.max_bad_tokens);
        assert_eq!(settings.bad_token_cooldown_s, config.bad_token_cooldown_s);
    }
}

#[cfg(test)]
mod buffer_expansion_tests {
    /// №755: the control-plane queues expanded with the rest of the order.
    #[test]
    fn the_control_buffer_budget_is_pinned() {
        assert_eq!(super::QUEUE_CAP, 256);
        assert_eq!(crate::control::ring::SUBSCRIBER_QUEUE, 4096);
        assert_eq!(crate::control::journal::RING_CAP, 2048);
    }
}
