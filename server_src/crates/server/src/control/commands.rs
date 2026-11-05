//! The command layer the plan demands as "чистые функции «состояние + запрос
//! → ответ»" (section 10.1): every accepted read answers from the snapshot
//! the loop published — no command recomputes anything, no command touches a
//! socket. The listener (server.rs) is the only place with I/O; so this
//! whole file unit-tests with fixtures and no tokio.
//!
//! Stage 2 lives here as the pure half of the mutating commands: parsing
//! (`say`'s caps, `kick`'s address), the state and target gates, and the
//! `reload` decision are functions of (state, request, config) — the I/O
//! side (queueing an order into the tick loop, touching no file but the
//! journal the listener calls) sits in server.rs, and the loop's own
//! execution sits in main.rs. The split follows А.2 item 1: the queue
//! belongs to the loop, the door only decides.

use crate::config::{ControlConfig, MatchConfig, VoteKickConfig};
use super::journal::{ActionJournal, Entry};
use super::ring::{level_rank, LogEvent, LogRing};
use super::status::{PlayerStatus, ServerStatus};
use serde::Serialize;
use serde_json::{json, Map, Value};

/// Every command the port accepts. A name outside this list is
/// refused with `unknown_command` — the refusal names what IS accepted, so a
/// typo'd `ctl statuss` is diagnosable from its own error line. Stage 2
/// appends the mutating set and the `audit` read (plan section 6.1: the
/// journal's own command, etapa 2 row).
pub const COMMANDS: &[&str] = &[
    "status", "perf", "net", "players", "config", "log", "watch",
    // stage 2
    "say", "kick", "stop", "reload", "audit",
];

/// The topics a `watch` subscription may name. The journal (the ring exists)
/// and the connection feed (the hub is published to from the loop) are
/// stage 1; the state feed is stage 2's stop machinery publishing through
/// the same hub (`stopping`, and the loop's own state changes).
pub const WATCH_TOPICS: &[&str] = &["log", "conn", "state"];

/// The cap on a `say` text and on a `kick` reason — both ride the same
/// `Announcement` event. The plan fixes "пустой текст, слишком длинный ...
/// слишком часто" as the refusal cases but leaves the number to stage 2;
/// 256 bytes is a screen's worth of text, sits inside MAX_PACKET_SIZE with
/// the whole event overhead many times over, and is byte-length (UTF-8),
/// because the wire and the client know bytes, not chars. "Слишком часто"
/// is the door's own rate limiter (the token bucket every command pays).
pub const SAY_MAX_BYTES: usize = 256;

/// The server's effective configuration, less its secrets: `auth_token`
/// becomes a boolean, never a value, and the control token does not appear
/// here at all (it lives only in its file). Everything else is what the
/// operator would see editing the file anyway.
#[derive(Debug, Clone, Serialize)]
pub struct RedactedConfig {
    pub auth_token_set: bool,
    pub allow_unsigned: bool,
    pub vote_kick: VoteKickConfig,
    pub control: ControlConfig,
}

/// What a command produces. A read produces data; `watch` produces data
/// first and then a live feed the listener wires up.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Data(Value),
    Watch { topics: Vec<String> },
}

/// The refusal pair every bad path answers with: a fixed code from the
/// plan's list and a text that says what was wrong — free-form, operator
/// facing, never containing echo'd tokens or secret material.
pub type Refusal = (&'static str, String);

fn bad_arguments(text: impl Into<String>) -> Refusal {
    ("bad_arguments", text.into())
}

/// The parsed request: one happy path through `parse_request`, the dispatch
/// comes next. Tests drive both halves separately (parsing an arbitrary
/// request; executing a known one) — a command answer never depends on how
/// it was framed on the wire.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Status,
    Perf,
    Net,
    Players,
    Config,
    Log { tail: usize, min_rank: Option<u8> },
    Watch { topics: Vec<String> },
    // stage 2
    Say { text: String },
    Kick { player: u32, reason: String },
    Stop { delay_s: u64 },
    Reload,
    Audit { tail: usize },
}

/// Turns the wire's `cmd` + free-form `args` into a request, or the refusal
/// the connection should print. Strict where the plan says strict: a name
/// outside the list is unknown, values that cannot parse are bad arguments
/// with the field named.
pub fn parse_request(cmd: &str, args: Option<&Value>) -> Result<Request, Refusal> {
    match cmd {
        "status" => Ok(Request::Status),
        "perf" => Ok(Request::Perf),
        "net" => Ok(Request::Net),
        "players" => Ok(Request::Players),
        "config" => Ok(Request::Config),
        "log" => parse_log(args),
        "watch" => parse_watch(args),
        "say" => parse_say(args),
        "kick" => parse_kick(args),
        "stop" => parse_stop(args),
        "reload" => Ok(Request::Reload),
        "audit" => parse_audit(args),
        _ => Err((
            "unknown_command",
            format!(
                "unknown command {cmd:?}; the port accepts: {}",
                COMMANDS.join(", ")
            ),
        )),
    }
}

fn args_map(args: Option<&Value>) -> Result<Option<&Map<String, Value>>, Refusal> {
    match args {
        None => Ok(None),
        Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(other) => Err(bad_arguments(format!(
            "args must be an object, got {other}"
        ))),
    }
}

fn parse_log(args: Option<&Value>) -> Result<Request, Refusal> {
    let map = args_map(args)?;
    // Default tail: 100 — the plan's `log` shows "последние N строк", and an
    // operator printing a command without a tail wants the tail, not the
    // whole ring.
    let mut tail = 100usize;
    let mut min_rank = None;
    if let Some(map) = map {
        if let Some(tail_value) = map.get("tail") {
            let raw = tail_value
                .as_u64()
                .ok_or_else(|| bad_arguments("log.tail must be a non-negative integer"))?;
            // Zero lines is a request for nothing; refusing it names the
            // difference between "no such field" (fine) and "nonsense value"
            // (not fine) — the plan's refactor of silent reconfiguration.
            if raw == 0 {
                return Err(bad_arguments("log.tail of 0 asks for nothing; omit the field or ask for 1"));
            }
            tail = raw as usize;
        }
        if let Some(level_value) = map.get("level") {
            let raw = level_value
                .as_str()
                .ok_or_else(|| bad_arguments("log.level must be a string level name"))?
                .to_uppercase();
            min_rank = Some(
                level_rank(&raw)
                    .ok_or_else(|| bad_arguments(format!("log.level {raw:?} is not a level name (TRACE..ERROR)")))?,
            );
        }
    }
    Ok(Request::Log { tail, min_rank })
}

fn parse_watch(args: Option<&Value>) -> Result<Request, Refusal> {
    let map = args_map(args)?;
    let mut topics: Vec<String> = WATCH_TOPICS.iter().map(|t| t.to_string()).collect();
    if let Some(map) = map {
        if let Some(topics_value) = map.get("topics") {
            let list = topics_value
                .as_array()
                .ok_or_else(|| bad_arguments("watch.topics must be an array of topic names"))?;
            if list.is_empty() {
                return Err(bad_arguments("watch.topics of [] subscribes to nothing; omit the field for all topics"));
            }
            topics = list
                .iter()
                .map(|v| v.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| bad_arguments("watch.topics entries must be strings"))?;
            for topic in &topics {
                if !WATCH_TOPICS.contains(&topic.as_str()) {
                    return Err(bad_arguments(format!(
                        "unknown watch topic {topic:?}; known: {}",
                        WATCH_TOPICS.join(", ")
                    )));
                }
            }
        }
    }
    Ok(Request::Watch { topics })
}

/// `say` needs args with a `text` string at all — a request without one is
/// nonsense, not a silent announcement of "". Empty (white-space-only) and
/// over-cap texts are refused before anything is queued: the plan's
/// "пустой текст, слишком длинный" case, decided at the door where the
/// snapshot rules already live.
fn parse_say(args: Option<&Value>) -> Result<Request, Refusal> {
    let map = args_map(args)?;
    let map = map.ok_or_else(|| bad_arguments("say needs args {\"text\": \"…\"}"))?;
    let raw = map
        .get("text")
        .ok_or_else(|| bad_arguments("say needs a \"text\" field with the announcement"))?;
    let text = raw
        .as_str()
        .ok_or_else(|| bad_arguments("say \"text\" must be a string"))?;
    if text.trim().is_empty() {
        return Err(bad_arguments(
            "say: the announcement text is empty; there is nothing to announce",
        ));
    }
    if text.len() > SAY_MAX_BYTES {
        return Err(bad_arguments(format!(
            "say: the announcement is {} bytes; the cap is {SAY_MAX_BYTES}",
            text.len()
        )));
    }
    Ok(Request::Say { text: text.to_string() })
}

/// `kick` addresses the connection number (`player`, the id the `players`
/// listing speaks — the А.4 sample), with an optional reason. The reason
/// rides the same Announcement cap as `say`: the event has one budget, and
/// a reason the wire refuses is a refusal at the door, not a truncated
/// boot.
fn parse_kick(args: Option<&Value>) -> Result<Request, Refusal> {
    let map = args_map(args)?;
    let map = map.ok_or_else(|| bad_arguments("kick needs args {\"player\": 17, \"reason\": \"…\"}"))?;
    let player = map
        .get("player")
        .ok_or_else(|| bad_arguments("kick needs the \"player\" connection id"))?;
    let player = player
        .as_u64()
        .ok_or_else(|| bad_arguments("kick \"player\" must be the connection number (an integer)"))?
        .try_into()
        .map_err(|_| bad_arguments("kick \"player\" must fit the connection id range (u32)"))?;
    let reason = match map.get("reason") {
        None | Some(Value::Null) => String::new(),
        Some(raw) => {
            let reason = raw
                .as_str()
                .ok_or_else(|| bad_arguments("kick \"reason\" must be a string"))?;
            if reason.len() > SAY_MAX_BYTES {
                return Err(bad_arguments(format!(
                    "kick: the reason is {} bytes; the cap is {SAY_MAX_BYTES}",
                    reason.len()
                )));
            }
            reason.to_string()
        }
    };
    Ok(Request::Kick { player, reason })
}

/// `stop` takes an optional `delay_s`: 0 (or no field) — the immediate stop;
/// N — the soft-stop with its 60/30/10 s announcements. No upper cap: the
/// wait is the operator's stated intent and the journal records it — a
/// delay the operator asked for is not a defect the door should silently
/// shorten.
fn parse_stop(args: Option<&Value>) -> Result<Request, Refusal> {
    let map = args_map(args)?;
    let mut delay_s = 0u64;
    if let Some(map) = map {
        if let Some(raw) = map.get("delay_s") {
            delay_s = raw
                .as_u64()
                .ok_or_else(|| bad_arguments("stop \"delay_s\" must be a non-negative integer (seconds)"))?;
        }
    }
    Ok(Request::Stop { delay_s })
}

/// `audit` reads the action journal's tail; the same "tail of 0 asks for
/// nothing" rule `log` runs, and the same default-tail idea — recent
/// actions, not the whole ring.
fn parse_audit(args: Option<&Value>) -> Result<Request, Refusal> {
    let map = args_map(args)?;
    let mut tail = 50usize;
    if let Some(map) = map {
        if let Some(tail_value) = map.get("tail") {
            let raw = tail_value
                .as_u64()
                .ok_or_else(|| bad_arguments("audit.tail must be a non-negative integer"))?;
            if raw == 0 {
                return Err(bad_arguments("audit.tail of 0 asks for nothing; omit the field or ask for 1"));
            }
            tail = raw as usize;
        }
    }
    Ok(Request::Audit { tail })
}

/// Reads execute here — the pure "snapshot + request → answer" half. The
/// mutating stage-2 commands never reach this fn: the listener routes them
/// through the gates and the queue first (А.2 item 1), so an arm for them
/// below is a routing defect, refused rather than silently answered.
pub fn dispatch(
    request: &Request,
    status: &ServerStatus,
    ring: &LogRing,
    config: &RedactedConfig,
    actions: &ActionJournal,
) -> Result<Reply, Refusal> {
    match request {
        Request::Status => Ok(Reply::Data(status_data(status))),
        Request::Perf => Ok(Reply::Data(perf_data(status))),
        Request::Net => Ok(Reply::Data(net_data(status))),
        Request::Players => Ok(Reply::Data(players_data(status))),
        Request::Config => Ok(Reply::Data(config_data(config))),
        Request::Log { tail, min_rank } => Ok(Reply::Data(log_data(ring, *tail, *min_rank))),
        Request::Watch { topics } => Ok(Reply::Watch {
            topics: topics.clone(),
        }),
        Request::Audit { tail } => Ok(Reply::Data(audit_data(actions, *tail))),
        Request::Say { .. } | Request::Kick { .. } | Request::Stop { .. } | Request::Reload => {
            Err(bad_arguments(
                "a mutating command reached the read path; the listener must route it first",
            ))
        }
    }
}

/// The `status` answer: the plan's sample keys (`state`, `tick`, `tick_rate`,
/// `players`, `ships`, `uptime_s`) plus the fields a panel needs next
/// (identity, address, connection count). A reader ignores what it does not
/// know (the plan's reading rule), so additions here are safe.
pub fn status_data(status: &ServerStatus) -> Value {
    json!({
        "state": status.state,
        "match_id": status.match_id,
        "addr": status.addr,
        "port": status.port,
        "tick": status.tick,
        "tick_rate": status.tick_rate,
        "tick_budget_ms": status.tick_budget_ms,
        "uptime_s": status.uptime_s,
        "players": status.players,
        "ships": status.ships,
        "connections": status.connections,
        "entities": status.entities,
    })
}

/// The `perf` answer: the same window the `perf:` line printed, as data. The
/// serde struct is the answer — no second formatting of the same numbers
/// that could drift from the log line they mirror.
pub fn perf_data(status: &ServerStatus) -> Value {
    serde_json::to_value(&status.perf).unwrap_or_else(|_| json!({"error": "perf serialization"}))
}

pub fn net_data(status: &ServerStatus) -> Value {
    serde_json::to_value(&status.net).unwrap_or_else(|_| json!({"error": "net serialization"}))
}

/// The `players` answer: the loop's sorted listing plus its count. It is the
/// loop's bookkeeping as of the last ~1 s refresh — the point of the plan's
/// cadence rule is exactly that this is a snapshot, not a live query.
pub fn players_data(status: &ServerStatus) -> Value {
    json!({
        "count": status.player_list.len(),
        "players": status.player_list,
    })
}

/// The `config` answer: the effective configuration with secrets removed.
pub fn config_data(config: &RedactedConfig) -> Value {
    serde_json::to_value(config).unwrap_or_else(|_| json!({"error": "config serialization"}))
}

/// The `log` answer: the ring's tail, oldest first, with the level filter
/// applied. `returned` is counted after the filter — an ALL-WARN request
/// returning 2 of 5000 says something real with that number.
pub fn log_data(ring: &LogRing, tail: usize, min_rank: Option<u8>) -> Value {
    let lines: Vec<LogEvent> = ring.snapshot_tail(tail, min_rank);
    json!({
        "returned": lines.len(),
        "lines": lines,
    })
}

/// The `audit` answer: the journal's tail, oldest first, and the honest
/// note about the file — if the disk stopped taking lines, the tail in
/// memory is all the user is looking at, and they get told so.
pub fn audit_data(journal: &ActionJournal, tail: usize) -> Value {
    let actions: Vec<Entry> = journal.snapshot_tail(tail);
    json!({
        "returned": actions.len(),
        "actions": actions,
        "file_broken": journal.file_failed(),
    })
}

/// The state gate every mutating stage-2 command runs at the door (the А.4
/// code set, section 6.3 "commands rest in the queue" restated): only a
/// server the loop has marked `running` accepts changes; `stopping` refuses
/// with the reserved code, anything the loop has not got to yet refuses as
/// an unreachable state — the queue is not being served on those words
/// anyway.
pub fn gate_state(state: &str) -> Result<(), Refusal> {
    match state {
        "running" => Ok(()),
        "stopping" => Err((
            "shutting_down",
            "the server is stopping; commands that change it are refused".into(),
        )),
        other => Err((
            "not_allowed_in_state",
            format!("the server is {other}; commands wait for the loop to mark it running"),
        )),
    }
}

/// The `kick` address gate: the connection id must be someone the players
/// snapshot (the last second's refresh) serves — the А.4 sample's
/// `unknown_player`. The loop re-checks against the live map anyway: a
/// player may leave (or a connection id may be reused) between the snapshot
/// and the tick, and the loop journals that race as the kick outcome. This
/// gate is the fast, honest answer for the id that is clearly not there.
pub fn gate_kick_target(player: u32, player_list: &[PlayerStatus]) -> Result<(), Refusal> {
    if player_list.iter().any(|p| p.connection_id == player) {
        Ok(())
    } else {
        Err((
            "unknown_player",
            format!("no player with connection id {player}"),
        ))
    }
}

/// What a `reload` says (А.2 item 5): applied — adopted between ticks;
/// restart_required — differs from the running arrangement and needs a
/// restart to matter; rejected — the shape stays because the answer's form
/// names three lists, and today, with the whole file validated as one unit
/// before any decision, the rejected list is always empty. A file that
/// cannot parse refuses the reload outright — the server keeps running on
/// the old settings, the plan-worded trade.
#[derive(Debug, Clone, PartialEq)]
pub struct ReloadDecision {
    pub applied: Vec<&'static str>,
    pub restart_required: Vec<&'static str>,
    /// `Some` — a `vote_kick` section that differs in at least one field,
    /// the section the loop adopts between ticks.
    pub vote_kick: Option<VoteKickConfig>,
}

/// The diff. The running arrangement comes in two pieces: the redacted
/// everything (`allow_unsigned`, `vote_kick`, `control`) and the №700 key
/// compared in place — the key itself is never echoed, so an answer
/// listing `auth_token` as restart-required cannot leak the value (only
/// that it differs).
///
/// `vote_kick` differences are per-field, named for the file field they
/// came from (`vote_kick.min_votes`), so a ctl answer reads like a diff.
pub fn reload_decide(
    running: &RedactedConfig,
    running_auth_token: &Option<String>,
    file: &MatchConfig,
) -> ReloadDecision {
    let mut applied: Vec<&'static str> = Vec::new();
    let mut restart_required: Vec<&'static str> = Vec::new();

    let mut vote_kick = None;
    let file_vk = &file.vote_kick;
    if file_vk.enabled != running.vote_kick.enabled {
        applied.push("vote_kick.enabled");
    }
    if file_vk.min_votes != running.vote_kick.min_votes {
        applied.push("vote_kick.min_votes");
    }
    if (file_vk.majority - running.vote_kick.majority).abs() > f32::EPSILON {
        applied.push("vote_kick.majority");
    }
    if file_vk.window_ticks != running.vote_kick.window_ticks {
        applied.push("vote_kick.window_ticks");
    }
    if !applied.is_empty() {
        vote_kick = Some(file_vk.clone());
    }

    // The №700 key: the same normalization the start-up path ran
    // (trim, blank → None), then compared. A different form of the same key
    // is a different key — the player side hashes exactly these bytes.
    let file_auth = file
        .auth_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from);
    if &file_auth != running_auth_token {
        restart_required.push("auth_token");
    }

    if file.allow_unsigned != running.allow_unsigned {
        restart_required.push("allow_unsigned");
    }
    let rc = &running.control;
    let fc = &file.control;
    if fc.enabled != rc.enabled {
        restart_required.push("control.enabled");
    }
    if fc.port != rc.port {
        restart_required.push("control.port");
    }
    if fc.token_file != rc.token_file {
        restart_required.push("control.token_file");
    }
    if fc.max_connections != rc.max_connections {
        restart_required.push("control.max_connections");
    }
    if fc.max_line_bytes != rc.max_line_bytes {
        restart_required.push("control.max_line_bytes");
    }
    if fc.commands_per_second != rc.commands_per_second {
        restart_required.push("control.commands_per_second");
    }
    if fc.max_bad_tokens != rc.max_bad_tokens {
        restart_required.push("control.max_bad_tokens");
    }
    if fc.bad_token_cooldown_s != rc.bad_token_cooldown_s {
        restart_required.push("control.bad_token_cooldown_s");
    }
    if fc.action_log_file != rc.action_log_file {
        restart_required.push("control.action_log_file");
    }

    ReloadDecision {
        applied,
        restart_required,
        vote_kick,
    }
}

#[cfg(test)]
mod commands_tests {
    use super::*;
    use crate::control::status::PlayerStatus;
    use crate::control::ring::LogRing;

    fn fixture_status() -> ServerStatus {
        ServerStatus {
            state: "running".into(),
            match_id: "0d3c-...".into(),
            addr: "0.0.0.0:7777".into(),
            port: 7777,
            uptime_s: 615,
            tick: 18450,
            tick_rate: 30,
            tick_budget_ms: 33.33,
            ships: 4,
            players: 2,
            connections: 2,
            player_list: vec![
                PlayerStatus { connection_id: 2, name: "player".into(), addr: "127.0.0.1:11".into(), ship_id: 1001, ping_ms: 41.0, age_s: 5 },
                PlayerStatus { connection_id: 7, name: "player".into(), addr: "127.0.0.1:12".into(), ship_id: 1002, ping_ms: 180.0, age_s: 60 },
            ],
            ..ServerStatus::default()
        }
    }

    fn redacted() -> RedactedConfig {
        RedactedConfig {
            auth_token_set: true,
            allow_unsigned: false,
            vote_kick: VoteKickConfig::default(),
            control: ControlConfig::default(),
        }
    }

    /// A stage-2 journal under this run's own temp dir: an open()-able path
    /// every test gets fresh, so a ring leak between tests cannot pass for
    /// a rule.
    fn stage2_journal() -> ActionJournal {
        let path = std::env::temp_dir().join(format!(
            "rfs_server_commands_tests_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock is before epoch")
                .as_nanos()
        ));
        ActionJournal::open(&path).expect("the journal opens")
    }

    #[test]
    fn status_data_carries_the_sample_keys_with_their_snapshot_numbers() {
        let status = fixture_status();
        let data = status_data(&status);
        assert_eq!(data["state"], "running");
        assert_eq!(data["tick"], 18450);
        assert_eq!(data["tick_rate"], 30);
        assert_eq!(data["players"], 2);
        assert_eq!(data["ships"], 4);
        assert_eq!(data["uptime_s"], 615);
        assert_eq!(data["match_id"], "0d3c-...");
    }

    #[test]
    fn perf_and_net_data_are_the_structs_themselves_no_second_format() {
        let mut status = fixture_status();
        status.perf.loop_avg_ms = 9.7;
        status.perf.window_rounds = 5;
        status.net.received_bytes_total = 42;
        assert_eq!(perf_data(&status)["loop_avg_ms"], 9.7);
        assert_eq!(perf_data(&status)["window_rounds"], 5);
        assert_eq!(net_data(&status)["received_bytes_total"], 42);
    }

    #[test]
    fn players_data_copies_the_sorted_listing_and_counts_it() {
        let data = players_data(&fixture_status());
        assert_eq!(data["count"], 2);
        let ids: Vec<u32> = data["players"]
            .as_array()
            .expect("players is an array")
            .iter()
            .map(|p| p["connection_id"].as_u64().unwrap() as u32)
            .collect();
        assert_eq!(ids, vec![2, 7], "the snapshot must already be sorted");
    }

    #[test]
    fn config_data_redacts_the_secret_but_names_whether_it_is_set() {
        let data = config_data(&redacted());
        assert_eq!(data["auth_token_set"], true);
        assert_eq!(data["control"]["port"], ControlConfig::default().port);
        // The text search the plan demands (section 10.5): the whole config
        // answer holds no secret material.
        let whole = data.to_string();
        assert!(!whole.contains("station-secret"), "no secret in {whole}");
    }

    #[test]
    fn every_command_dispatches_and_the_dispatcher_is_total() {
        let status = fixture_status();
        let ring = LogRing::new(10);
        let config = redacted();
        let journal = stage2_journal();
        for cmd in ["status", "perf", "net", "players", "config"] {
            let request = parse_request(cmd, None).unwrap_or_else(|e| panic!("{cmd}: {e:?}"));
            dispatch(&request, &status, &ring, &config, &journal)
                .unwrap_or_else(|e| panic!("{cmd}: {e:?}"));
        }
        let request = parse_request("audit", None).expect("audit reads the journal");
        dispatch(&request, &status, &ring, &config, &journal)
            .expect("audit answers from the journal handle");
        assert!(parse_request("status", None).is_ok());
        assert!(parse_request("log", None).is_ok());
        assert!(parse_request("watch", None).is_ok());
    }

    #[test]
    fn an_unknown_command_names_the_accepted_set_in_its_refusal() {
        let (code, text) = parse_request("statuss", None).expect_err("typo must refuse");
        assert_eq!(code, "unknown_command");
        assert!(text.contains("status"), "the refusal lists the set: {text}");
    }

    #[test]
    fn log_defaults_to_the_tail_of_100_and_takes_tail_and_level() {
        let request = parse_request("log", None).expect("no args is fine");
        assert_eq!(request, Request::Log { tail: 100, min_rank: None });

        let request = parse_request("log", Some(&json!({"tail": 7, "level": "warn"})))
            .expect("both fields parse");
        assert_eq!(request, Request::Log { tail: 7, min_rank: Some(3) });

        let (code, text) =
            parse_request("log", Some(&json!({"tail": 0}))).expect_err("zero tail refuses");
        assert_eq!(code, "bad_arguments");
        assert!(text.contains("tail"), "the refusal names the field: {text}");

        assert!(parse_request("log", Some(&json!({"tail": -1}))).is_err());
        assert!(parse_request("log", Some(&json!({"tail": "big"}))).is_err());
        assert!(parse_request("log", Some(&json!({"level": "PANIC"}))).is_err());
    }

    #[test]
    fn watch_defaults_to_every_topic_and_refuses_unknown_and_empty_lists() {
        let request = parse_request("watch", None).expect("no args means all topics");
        assert_eq!(
            request,
            Request::Watch { topics: vec!["log".into(), "conn".into(), "state".into()] }
        );

        let request = parse_request("watch", Some(&json!({"topics": ["log"]})))
            .expect("an explicit known topic is fine");
        assert_eq!(request, Request::Watch { topics: vec!["log".into()] });

        let single = parse_request("watch", Some(&json!({"topics": ["conn"]})))
            .expect("the connection feed is a stage-1 topic");
        assert_eq!(single, Request::Watch { topics: vec!["conn".into()] });

        let state_feed = parse_request("watch", Some(&json!({"topics": ["state"]})))
            .expect("the state feed is stage 2's own topic");
        assert_eq!(state_feed, Request::Watch { topics: vec!["state".into()] });

        let (code, text) = parse_request("watch", Some(&json!({"topics": ["players"]})))
            .expect_err("an unknown topic refuses");
        assert_eq!(code, "bad_arguments");
        assert!(text.contains("players"), "the refusal names the topic: {text}");

        assert!(parse_request("watch", Some(&json!({"topics": []}))).is_err());
        assert!(parse_request("watch", Some(&json!({"topics": "log"}))).is_err());
    }

    #[test]
    fn non_object_args_are_bad_arguments_not_a_panic() {
        assert!(parse_request("log", Some(&json!([1, 2]))).is_err());
        assert!(parse_request("log", Some(&json!("tail=10"))).is_err());
        // args:null means "no args" — the reader-must-tolerate rule.
        assert!(parse_request("log", Some(&Value::Null)).is_ok());
    }

    #[test]
    fn log_data_counts_after_the_filter_and_serves_oldest_first() {
        let ring = LogRing::new(10);
        ring.record(&"INFO".parse().unwrap(), "one");
        ring.record(&"INFO".parse().unwrap(), "two");
        ring.record(&"WARN".parse().unwrap(), "hot");
        let data = log_data(&ring, 2, None);
        assert_eq!(data["returned"], 2, "the tail bounds the answer");
        let texts: Vec<&str> = data["lines"]
            .as_array()
            .expect("lines is an array")
            .iter()
            .map(|l| l["text"].as_str().expect("text is a string"))
            .collect();
        assert_eq!(texts, vec!["two", "hot"]);

        // The filtered path: only the WARN line survives a WARN-or-higher
        // ask, and the count says so.
        let data = log_data(&ring, 10, level_rank("WARN"));
        assert_eq!(data["returned"], 1);
    }

    // ---- stage 2 ----

    #[test]
    fn say_parses_the_text_and_enforces_the_byte_cap() {
        let request = parse_request("say", Some(&json!({"text": "hello"})))
            .expect("a plain text parses");
        assert_eq!(request, Request::Say { text: "hello".into() });

        // The cap is BYTES (UTF-8), not chars (the wire counts bytes).
        let exactly = "a".repeat(SAY_MAX_BYTES);
        assert!(parse_request("say", Some(&json!({"text": exactly}))).is_ok());
        let over = "a".repeat(SAY_MAX_BYTES + 1);
        let (code, text) = parse_request("say", Some(&json!({"text": over})))
            .expect_err("one byte over the cap refuses");
        assert_eq!(code, "bad_arguments");
        assert!(text.contains("cap"), "the refusal names the cap: {text}");

        let (code, _) = parse_request("say", Some(&json!({"text": "   "})))
            .expect_err("a blank announcement is the plan's empty-text case");
        assert_eq!(code, "bad_arguments");

        let (code, _) = parse_request("say", Some(&json!({"text": 5})))
            .expect_err("a non-string text refuses");
        assert_eq!(code, "bad_arguments");
        assert!(parse_request("say", None).is_err(), "no text no announcement");
    }

    #[test]
    fn kick_parses_the_connection_id_and_the_optional_reason() {
        let request = parse_request("kick", Some(&json!({"player": 7})))
            .expect("the id alone is the А.4 shape");
        assert_eq!(request, Request::Kick { player: 7, reason: String::new() });

        let request = parse_request("kick", Some(&json!({"player": 7, "reason": "spam"})))
            .expect("a reason rides along");
        assert_eq!(request, Request::Kick { player: 7, reason: "spam".into() });

        // The reason shares say's budget: one event, one cap.
        let over = "a".repeat(SAY_MAX_BYTES + 1);
        let (code, _) = parse_request("kick", Some(&json!({"player": 7, "reason": over})))
            .expect_err("an over-cap reason refuses");
        assert_eq!(code, "bad_arguments");

        assert!(parse_request("kick", None).is_err(), "no id no kick");
        assert!(
            parse_request("kick", Some(&json!({"player": -1}))).is_err(),
            "a negative id cannot be a connection number"
        );
        assert!(
            parse_request("kick", Some(&json!({"player": "7"}))).is_err(),
            "the id is a number, a string is bad arguments"
        );
    }

    #[test]
    fn kick_refuses_an_id_the_players_list_does_not_name() {
        let status = fixture_status();
        assert!(gate_kick_target(2, &status.player_list).is_ok());
        assert!(gate_kick_target(7, &status.player_list).is_ok());

        let (code, text) = gate_kick_target(9, &status.player_list)
            .expect_err("an id absent from the listing refuses");
        assert_eq!(code, "unknown_player");
        assert!(text.contains('9'), "the refusal names the id: {text}");
    }

    #[test]
    fn the_state_gate_only_runs_a_running_server() {
        assert!(gate_state("running").is_ok());

        // stopping → the reserved А.4 code, not a generic refusal.
        let (code, _) = gate_state("stopping").expect_err("stopping refuses changes");
        assert_eq!(code, "shutting_down");

        // Anything else — including the pre-loop `starting` — refuses with
        // the not-allowed code and names the word it saw.
        let (code, text) = gate_state("starting").expect_err("starting refuses changes");
        assert_eq!(code, "not_allowed_in_state");
        assert!(text.contains("starting"), "the refusal names the state: {text}");
    }

    #[test]
    fn stop_takes_the_optional_delay_and_nothing_else_matters() {
        assert_eq!(parse_request("stop", None), Ok(Request::Stop { delay_s: 0 }));
        assert_eq!(
            parse_request("stop", Some(&json!({}))),
            Ok(Request::Stop { delay_s: 0 })
        );
        assert_eq!(
            parse_request("stop", Some(&json!({"delay_s": 0}))),
            Ok(Request::Stop { delay_s: 0 })
        );
        assert_eq!(
            parse_request("stop", Some(&json!({"delay_s": 30}))),
            Ok(Request::Stop { delay_s: 30 })
        );
        assert!(parse_request("stop", Some(&json!({"delay_s": -5}))).is_err());
        assert!(parse_request("stop", Some(&json!({"delay_s": "30"}))).is_err());
    }

    #[test]
    fn audit_takes_the_optional_tail_and_refuses_zero() {
        assert_eq!(parse_request("audit", None), Ok(Request::Audit { tail: 50 }));
        assert_eq!(
            parse_request("audit", Some(&json!({"tail": 3}))),
            Ok(Request::Audit { tail: 3 })
        );
        let (code, text) = parse_request("audit", Some(&json!({"tail": 0})))
            .expect_err("a tail of zero asks for nothing");
        assert_eq!(code, "bad_arguments");
        assert!(text.contains("tail"), "the refusal names the field: {text}");
        assert!(parse_request("audit", Some(&json!({"tail": -1}))).is_err());
    }

    #[test]
    fn reload_decide_differences_by_group() {
        // Same file, same running arrangement → nothing anywhere.
        let decision = reload_decide(
            &redacted(),
            &Some("key".into()),
            &file_with(None, false, "key", ControlConfig::default()),
        );
        assert_eq!(
            decision,
            ReloadDecision {
                applied: vec![],
                restart_required: vec![],
                vote_kick: None
            },
            "an identical file reloads to a no-op"
        );

        // vote_kick differs → applied, the whole section queued.
        let mut running = redacted();
        running.vote_kick.min_votes = 3;
        let decision = reload_decide(
            &running,
            &Some("key".into()),
            &file_with(Some(VoteKickConfig::default()), false, "key", ControlConfig::default()),
        );
        assert_eq!(decision.applied, vec!["vote_kick.min_votes"]);
        assert_eq!(
            decision.vote_kick,
            Some(VoteKickConfig::default()),
            "the section exactly as the file named it is queued, not a hand-made one"
        );
        assert!(decision.restart_required.is_empty());

        // The №700 key and the door shape → restart_required, never applied.
        let control = ControlConfig { port: 9999, ..ControlConfig::default() };
        let decision = reload_decide(
            &redacted(),
            &Some("key".into()),
            &file_with(None, true, "other-key", control),
        );
        assert!(decision.applied.is_empty(), "the door is frozen at boot");
        assert_eq!(
            decision.restart_required,
            vec!["auth_token", "allow_unsigned", "control.port"]
        );
    }

    #[test]
    fn reload_decide_normalizes_the_key_like_start_up_did() {
        // The same key with padding whitespace is the SAME key: the
        // start-up path did trim + blank→None, reload compares the same way.
        let decision = reload_decide(
            &redacted(),
            &None,
            &file_with(None, false, "   ", ControlConfig::default()),
        );
        assert!(
            decision.restart_required.is_empty(),
            "a blank key normalizes to None, not to a difference"
        );
    }

    /// A file-shaped MatchConfig for the diff tests: only the groups the
    /// decision reads (vote_kick, auth_token, allow_unsigned, control).
    fn file_with(
        vote_kick: Option<VoteKickConfig>,
        allow_unsigned: bool,
        auth_token: &str,
        control: ControlConfig,
    ) -> MatchConfig {
        MatchConfig {
            vote_kick: vote_kick.unwrap_or_default(),
            allow_unsigned,
            auth_token: Some(auth_token.into()),
            control,
        }
    }

    #[test]
    fn audit_data_serves_the_tail_and_says_whether_the_file_is_alive() {
        let journal = stage2_journal();
        journal.record("door", "say \"hi\"", "queued");
        journal.record("loop", "say \"hi\"", "announced");

        let data = audit_data(&journal, 50);
        assert_eq!(data["returned"], 2);
        let sources: Vec<&str> = data["actions"]
            .as_array()
            .expect("actions is an array")
            .iter()
            .map(|a| a["source"].as_str().expect("source is a string"))
            .collect();
        assert_eq!(sources, vec!["door", "loop"], "oldest first");
        assert_eq!(data["file_broken"], false);

        let data = audit_data(&journal, 1);
        assert_eq!(data["returned"], 1, "the tail bounds the answer");
    }
}
