//! `ctl` — the console utility the plan parks before the window (section 3,
//! Решение №3): it speaks the same newline-JSON the panel would, straight to
//! the control port. Its value is twofold: the operator has eyes today, and
//! the interface itself is testable without any UI.
//!
//! ```text
//! ctl status
//! ctl log --follow
//! ctl perf --interval 100          (poll ten times a second, stage-1 check)
//! ctl say Match restarts in a minute        (stage 2)
//! ctl kick 17 griefing                      (stage 2)
//! ctl stop 30                               (stage 2, announcements at 60/30/10)
//! ctl reload                                (stage 2, re-reads the server config)
//! ctl audit 20                              (stage 2, the action journal's tail)
//! ```
//!
//! The stage-2 commands take positional arguments on the line exactly as
//! typed above; the reads stay flag-driven as stage 1 shaped them. The
//! token and the port come straight from the file the server produced:
//! the config (its `control` section carries the port) and the token file
//! named next to it.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// The one-line envelope the token file holds — the same shape
/// `control/token.rs` writes. A struct, not a bare string: the field is
/// named on both ends and a format change is a compile break, not a silent
/// misread.
#[derive(Debug, Deserialize)]
struct TokenFile {
    token: String,
}

fn usage() -> String {
    "usage: ctl <command> [--config <path>] [--port <n>] [--token-file <path>] \
     [--follow] [--interval <ms>]\n\
     reads:   status perf net players config log watch\n\
     writes:  say <text> | kick <connection-id> [reason…] | stop [delay-seconds] | reload\n\
     journal: audit [tail]"
        .to_string()
}

/// Bug №750 (double-click finding): a console window the operator launched
/// by double-clicking closes the instant the process exits, and the usage
/// line is exactly what such a launch needs to read. The boundary is the
/// argument list alone, so the rule is pure: nothing typed after the
/// program name means double-click, and double-click means pause. Nothing
/// sniffs a tty and nothing else guesses at being interactive.
fn should_pause_on_exit(args_after_program: &[String]) -> bool {
    args_after_program.is_empty()
}

/// The pause. A closed or empty stdin answers `read_line` with `Ok(0)` — a
/// clean end of input, not an error — so a paused process whose console is
/// gone (or whose caller piped an empty stdin) falls straight through and
/// exits instead of hanging. The Enter the operator presses is read and
/// discarded.
fn pause_for_console() {
    println!("press Enter to close...");
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::from)
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// The resolved connection facts: the port from the config's `control`
/// section (overridable), the token from its file (overridable). The binds
/// of `ctl` are loopback-only like the server's — a `--host` flag has no
/// business existing while the plan says the door is this-machine-only.
#[derive(Debug)]
struct Connection {
    port: u16,
    token: String,
}

fn resolve_connection(args: &[String]) -> Result<Connection> {
    let config_path = std::path::PathBuf::from(
        arg_value(args, "--config")
            .unwrap_or_else(|| rfs_server::config::DEFAULT_CONFIG_PATH.to_string()),
    );
    let config_text = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config: rfs_server::config::MatchConfig =
        serde_json::from_str(&config_text).context("parsing the server config")?;
    if !config.control.enabled {
        bail!(
            "the config at {} has control.enabled = false — the control door is off",
            config_path.display()
        );
    }
    let port = arg_value(args, "--port")
        .map(|p| p.parse::<u16>())
        .transpose()
        .context("--port must be a number")?
        .unwrap_or(config.control.port);
    let token_path = match arg_value(args, "--token-file") {
        Some(path) => std::path::PathBuf::from(path),
        None => {
            // The same resolution the server does: the token file sits next
            // to the config. A `control.token_file` name is joined to the
            // config's directory; the default name sits there too.
            match &config.control.token_file {
                Some(name) => config_path
                    .parent()
                    .map(|dir| dir.join(name))
                    .unwrap_or_else(|| std::path::PathBuf::from(name)),
                None => config_path
                    .parent()
                    .map(|dir| dir.join(rfs_server::control::token::DEFAULT_TOKEN_FILE_NAME))
                    .unwrap_or_else(|| {
                        std::path::PathBuf::from(rfs_server::control::token::DEFAULT_TOKEN_FILE_NAME)
                    }),
            }
        }
    };
    let token_text = std::fs::read_to_string(&token_path)
        .with_context(|| format!("reading the control token from {}", token_path.display()))?;
    let token_file: TokenFile = serde_json::from_str(&token_text)
        .with_context(|| format!("parsing {}", token_path.display()))?;
    Ok(Connection { port, token: token_file.token })
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Outbound {
    Hello { hello: HelloBody },
    Command { id: u64, cmd: String, #[serde(skip_serializing_if = "Option::is_none")] args: Option<Value> },
    Watch { id: u64, cmd: String, args: Value },
}

#[derive(Debug, Serialize)]
struct HelloBody {
    token: String,
    proto: u32,
}

/// The inbound side: only what `ctl` needs. Unknown fields ignored, as the
/// protocol promises — a newer server stays readable.
#[derive(Debug, Deserialize)]
struct Inbound {
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    hello: Option<String>,
    #[serde(default)]
    control_proto: Option<u32>,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

/// One human line per JSON answer, the numbers quoted whole so a `ctl` line
/// and a `perf:` line can be compared by eye in the same unit.
fn print_answer(command: &str, answer: &Inbound) {
    if let Some(data) = &answer.data {
        match command {
            "status" => {
                let num = |key: &str| data.get(key).map(pretty_number).unwrap_or_else(|| "-".to_string());
                println!(
                    "state={} match={} addr={} tick={} tick_rate={} players={} ships={} uptime={}s connections={}",
                    data.get("state").and_then(Value::as_str).unwrap_or("-"),
                    data.get("match_id").and_then(Value::as_str).unwrap_or("-"),
                    data.get("addr").and_then(Value::as_str).unwrap_or("-"),
                    num("tick"),
                    num("tick_rate"),
                    num("players"),
                    num("ships"),
                    num("uptime_s"),
                    num("connections"),
                );
            }
            "perf" => {
                let num = |key: &str| data.get(key).map(pretty_number).unwrap_or_else(|| "-".to_string());
                println!(
                    "perf tick={} sim_sync_avg_ms={} sim_sync_max_ms={} loop_avg_ms={} loop_max_ms={} budget_ms={} skipped={} window_rounds={} window_packets={} rounds_per_tick={} packets_per_round={} ws_mb={}",
                    num("ticks_ran"),
                    num("sim_sync_avg_ms"),
                    num("sim_sync_max_ms"),
                    num("loop_avg_ms"),
                    num("loop_max_ms"),
                    num("budget_ms"),
                    num("ticks_skipped_cumulative"),
                    num("window_rounds"),
                    num("window_packets"),
                    num("rounds_per_tick"),
                    num("packets_per_round"),
                    num("working_set_mb"),
                );
            }
            "net" => {
                let num = |key: &str| data.get(key).map(pretty_number).unwrap_or_else(|| "-".to_string());
                println!(
                    "net sent_bps={} received_bps={} sent_packets={} received_packets={} rounds={} first_fulls={} dropped_events={} dropped_outbox={} session_rejects={} pending_challenges={}",
                    num("sent_bps"),
                    num("received_bps"),
                    num("sent_packets_total"),
                    num("received_packets_total"),
                    num("snapshot_rounds_total"),
                    num("first_fulls_total"),
                    num("dropped_events"),
                    num("dropped_outbox"),
                    num("session_rejects"),
                    num("pending_challenges"),
                );
            }
            "players" => {
                println!("players count={}", data.get("count").and_then(Value::as_u64).unwrap_or(0));
                if let Some(list) = data.get("players").and_then(Value::as_array) {
                    for player in list {
                        println!(
                            "  player id={} name={} addr={} ship={} ping={}ms age={}s",
                            player.get("connection_id").and_then(Value::as_u64).unwrap_or(0),
                            player.get("name").and_then(Value::as_str).unwrap_or("-"),
                            player.get("addr").and_then(Value::as_str).unwrap_or("-"),
                            player.get("ship_id").and_then(Value::as_u64).unwrap_or(0),
                            player.get("ping_ms").map(pretty_number).unwrap_or_else(|| "-".to_string()),
                            player.get("age_s").and_then(Value::as_u64).unwrap_or(0),
                        );
                    }
                }
            }
            "config" => {
                println!("config {}", serde_json::to_string(data).unwrap_or_default());
            }
            "log" => {
                if let Some(lines) = data.get("lines").and_then(Value::as_array) {
                    for line in lines {
                        println!(
                            "{} {}",
                            line.get("level").and_then(Value::as_str).unwrap_or("INFO"),
                            line.get("text").and_then(Value::as_str).unwrap_or(""),
                        );
                    }
                }
            }
            // Stage 2: one line per write, the words the door itself said.
            "say" => {
                println!(
                    "say queued bytes={}",
                    data.get("bytes").map(pretty_number).unwrap_or_else(|| "-".to_string())
                );
            }
            "kick" | "stop" => {
                println!(
                    "{command} queued{}",
                    match data.get("delay_s") {
                        Some(delay) => format!(" delay={}s", pretty_number(delay)),
                        None => String::new(),
                    }
                );
            }
            "reload" => {
                println!(
                    "reload applied={} restart_required={} rejected={}",
                    data.get("applied").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0),
                    data.get("restart_required")
                        .and_then(Value::as_array)
                        .map(|a| a.len())
                        .unwrap_or(0),
                    data.get("rejected").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0),
                );
                let fields = |key: &str| {
                    data.get(key)
                        .and_then(Value::as_array)
                        .map(|list| {
                            list.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<&str>>()
                                .join(", ")
                        })
                        .unwrap_or_default()
                };
                let applied = fields("applied");
                let restart = fields("restart_required");
                if !applied.is_empty() {
                    println!("  applied live: {applied}");
                }
                if !restart.is_empty() {
                    println!("  waits for a restart: {restart}");
                }
                if let Some(note) = data.get("note").and_then(Value::as_str) {
                    println!("  note: {note}");
                }
            }
            "audit" => {
                for action in data
                    .get("actions")
                    .and_then(Value::as_array)
                    .unwrap_or(&Vec::new())
                {
                    println!(
                        "{} [{}] {}: {}",
                        action.get("ts").and_then(Value::as_str).unwrap_or("-"),
                        action.get("source").and_then(Value::as_str).unwrap_or("-"),
                        action.get("command").and_then(Value::as_str).unwrap_or("-"),
                        action.get("outcome").and_then(Value::as_str).unwrap_or(""),
                    );
                }
                if data.get("file_broken").and_then(Value::as_bool) == Some(true) {
                    println!(
                        "journal: the file stopped accepting writes; this tail is the ring only"
                    );
                }
            }
            _ => println!("{}", serde_json::to_string(data).unwrap_or_default()),
        }
    } else {
        println!("{}", serde_json::to_string(&data_or_error(answer)).unwrap_or_default());
    }
}

fn pretty_number(value: &Value) -> String {
    match value {
        // f64 fields print at the same precision the `perf:` line uses, so a
        // ctl line and the log line can be compared field by field.
        Value::Number(n) if n.is_f64() => format!("{:.2}", n.as_f64().unwrap_or(0.0)),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn data_or_error(answer: &Inbound) -> Value {
    answer
        .data
        .clone()
        .or_else(|| answer.error.clone())
        .unwrap_or(Value::Null)
}

async fn one_round(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    command: &str,
    args: Option<Value>,
    id: u64,
) -> Result<Inbound> {
    let outgoing = match command {
        "watch" => Outbound::Watch { id, cmd: "watch".into(), args: json!({}) },
        _ => Outbound::Command { id, cmd: command.to_string(), args },
    };
    let line = serde_json::to_string(&outgoing).context("serializing the command")?;
    writer
        .write_all(format!("{line}\n").as_bytes())
        .await
        .context("writing the command")?;
    let mut incoming = String::new();
    reader
        .read_line(&mut incoming)
        .await
        .context("reading the answer")?;
    let parsed: Inbound =
        serde_json::from_str(incoming.trim_end()).context("parsing the answer")?;
    Ok(parsed)
}

/// Flags that take a value; both the command finder and the positional
/// collector skip them. Stage 2 changed none of the names — reads stay
/// flag-driven, writes go positional.
const FLAGS_WITH_VALUE: [&str; 4] = ["--config", "--port", "--token-file", "--interval"];

// Stage 2: the command word and its positional arguments are separated at
// parse time — everything after the command that is neither a flag nor a
// flag's value belongs to the command (`ctl kick 17 griefing` → ["17", …]).
fn positional_after_command(args: &[String], command_index: usize) -> Vec<String> {
    args[command_index + 1..]
        .iter()
        .enumerate()
        .filter(|(rel, a)| {
            let index = command_index + 1 + rel;
            !(a.starts_with("--")
                || (index > 0 && FLAGS_WITH_VALUE.contains(&args[index - 1].as_str())))
        })
        .map(|(_, a)| a.clone())
        .collect()
}

/// The command's args payload, per the stage-2 shapes: `say` joins its
/// positional words into one text (a console is how the operator speaks
/// here), `kick` names the connection id with an optional reason, `stop`
/// takes the optional seconds, `audit` the optional tail.
fn build_command_args(command: &str, positional: &[String]) -> Result<Option<Value>> {
    match command {
        "say" => {
            let text = positional.join(" ");
            if text.trim().is_empty() {
                bail!("say needs the text as positional words — {}", usage());
            }
            Ok(Some(json!({ "text": text })))
        }
        "kick" => {
            if positional.is_empty() {
                bail!("kick needs the connection id first — {}", usage());
            }
            let player: u32 = positional[0]
                .parse()
                .map_err(|_| anyhow::anyhow!("kick's first positional must be the connection id (a number) — {}", usage()))?;
            let reason = positional[1..].join(" ");
            Ok(Some(json!({ "player": player, "reason": reason })))
        }
        "stop" => match positional.first() {
            None => Ok(None),
            Some(raw) => {
                let delay_s: u64 = raw.parse().map_err(|_| {
                    anyhow::anyhow!("stop's positional must be the delay in seconds (a number) — {}", usage())
                })?;
                Ok(Some(json!({ "delay_s": delay_s })))
            }
        },
        "audit" => match positional.first() {
            None => Ok(None),
            Some(raw) => {
                let tail: u64 = raw.parse().map_err(|_| {
                    anyhow::anyhow!("audit's positional must be the tail length (a number) — {}", usage())
                })?;
                Ok(Some(json!({ "tail": tail })))
            }
        },
        _ => Ok(None),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The command is the first word that is neither a flag nor a flag's
    // value: `ctl --token-file t.json status` names the same command as
    // `ctl status --token-file t.json`. Flags that take a value are skipped
    // together with that value; `arg_value` below reads them wherever they
    // sit on the line.
    let command: (String, usize) = {
        let mut found: Option<usize> = None;
        for (index, a) in args.iter().enumerate() {
            if a.starts_with("--") {
                continue; // a flag itself
            }
            if index > 0 && FLAGS_WITH_VALUE.contains(&args[index - 1].as_str()) {
                continue; // the flag's value, not a command
            }
            found = Some(index);
            break;
        }
        match found {
            Some(i) => (args[i].clone(), i),
            None => {
                // Bug №750: the usage a double-clicked window shows closes
                // with its window in the same blink on the bare launch; the
                // pause holds it on screen. A scripted run always carries a
                // command, so its exit is untouched.
                if should_pause_on_exit(&args) {
                    pause_for_console();
                }
                bail!("{} — no command given", usage())
            }
        }
    };
    let command_index = command.1;
    let command = command.0.as_str();
    // The stage-2 writes say what they are about on the same line; the
    // reads stay flag-driven (stage 1 shaped them that way).
    if !matches!(command, "status" | "perf" | "net" | "players" | "config" | "log" | "watch") {
        if arg_value(&args, "--interval").is_some() {
            bail!("`{command}` says something once; --interval is for the polling reads — {}", usage());
        }
        if has_flag(&args, "--follow") {
            bail!("`{command}` says something once; --follow is for the log feed — {}", usage());
        }
        let positional = positional_after_command(&args, command_index);
        let command_args = build_command_args(command, &positional)?;
        let connection = resolve_connection(&args)?;
        let answer = ask_once(&connection, command, command_args).await?;
        if let Some(error) = &answer.error {
            let code = error.get("code").and_then(Value::as_str).unwrap_or("internal");
            let text = error.get("text").and_then(Value::as_str).unwrap_or("");
            bail!("{code}: {text}");
        }
        print_answer(command, &answer);
        return Ok(());
    }
    let connection = resolve_connection(&args)?;
    if !has_flag(&args, "--interval") && !has_flag(&args, "--follow") {
        // plain single-shot: connect, ask, print, go
        let answer = ask_once(&connection, command, None).await?;
        if let Some(error) = &answer.error {
            let code = error.get("code").and_then(Value::as_str).unwrap_or("internal");
            let text = error.get("text").and_then(Value::as_str).unwrap_or("");
            bail!("{code}: {text}");
        }
        print_answer(command, &answer);
        return Ok(());
    }

    // --interval ms: repeat the same command, one connection held whole.
    if let Some(ms) = arg_value(&args, "--interval") {
        let ms: u64 = ms.parse().context("--interval must be milliseconds")?;
        poll_repeatedly(&connection, command, ms).await
    } else {
        // --follow: log snapshot, then the live feed until Ctrl-C.
        follow_log(&connection).await
    }
}

async fn ask_once(
    connection: &Connection,
    command: &str,
    args: Option<Value>,
) -> Result<Inbound> {
    let stream = TcpStream::connect(("127.0.0.1", connection.port))
        .await
        .with_context(|| format!("connecting to 127.0.0.1:{}", connection.port))?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    hello(&mut reader, &mut writer, &connection.token).await?;
    let id = 1;
    let answer = one_round(&mut reader, &mut writer, command, args, id).await?;
    Ok(answer)
}

async fn hello(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    token: &str,
) -> Result<u32> {
    let greeting = serde_json::to_string(&Outbound::Hello {
        hello: HelloBody { token: token.to_string(), proto: 1 },
    })
    .context("serializing the hello")?;
    writer
        .write_all(format!("{greeting}\n").as_bytes())
        .await
        .context("writing the hello")?;
    let mut incoming = String::new();
    reader.read_line(&mut incoming).await.context("reading the hello ack")?;
    let ack: Inbound =
        serde_json::from_str(incoming.trim_end()).context("parsing the hello ack")?;
    if let Some(error) = &ack.error {
        let code = error.get("code").and_then(Value::as_str).unwrap_or("internal");
        let text = error.get("text").and_then(Value::as_str).unwrap_or("");
        bail!("{code}: {text}");
    }
    if ack.hello.as_deref() != Some("ok") {
        bail!("the door did not answer the hello with ok");
    }
    ack.control_proto.context("the ack carries no control_proto")
}

async fn poll_repeatedly(connection: &Connection, command: &str, ms: u64) -> Result<()> {
    let stream = TcpStream::connect(("127.0.0.1", connection.port))
        .await
        .with_context(|| format!("connecting to 127.0.0.1:{}", connection.port))?;
    let (raw_reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(raw_reader);
    hello(&mut reader, &mut writer, &connection.token).await?;
    let mut id: u64 = 1;
    loop {
        let answer = one_round(&mut reader, &mut writer, command, None, id).await?;
        if let Some(error) = &answer.error {
            let code = error.get("code").and_then(Value::as_str).unwrap_or("internal");
            let text = error.get("text").and_then(Value::as_str).unwrap_or("");
            eprintln!("{code}: {text}");
        } else {
            print_answer(command, &answer);
        }
        id += 1;
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

async fn follow_log(connection: &Connection) -> Result<()> {
    let stream = TcpStream::connect(("127.0.0.1", connection.port))
        .await
        .with_context(|| format!("connecting to 127.0.0.1:{}", connection.port))?;
    let (raw_reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(raw_reader);
    hello(&mut reader, &mut writer, &connection.token).await?;
    let snapshot = ask_following(&mut reader, &mut writer, 1).await?;
    if let Some(error) = &snapshot.error {
        let code = error.get("code").and_then(Value::as_str).unwrap_or("internal");
        let text = error.get("text").and_then(Value::as_str).unwrap_or("");
        bail!("{code}: {text}");
    }
    if let Some(data) = &snapshot.data {
        if let Some(lines) = data.get("lines").and_then(Value::as_array) {
            for line in lines {
                println!(
                    "{} {}",
                    line.get("level").and_then(Value::as_str).unwrap_or("INFO"),
                    line.get("text").and_then(Value::as_str).unwrap_or(""),
                );
            }
        }
    }
    // The live feed: watch answers ok, then every event is a line of its own.
    let watch = serde_json::to_string(&Outbound::Watch {
        id: 2,
        cmd: "watch".into(),
        args: json!({}),
    })
    .context("serializing watch")?;
    writer
        .write_all(format!("{watch}\n").as_bytes())
        .await
        .context("writing watch")?;
    let mut incoming = String::new();
    reader.read_line(&mut incoming).await.context("reading the watch ack")?;
    let ack: Inbound =
        serde_json::from_str(incoming.trim_end()).context("parsing the watch ack")?;
    if ack.ok != Some(true) {
        bail!("watch was refused: {}", serde_json::to_string(&ack.error).unwrap_or_default());
    }
    loop {
        incoming.clear();
        let read = reader.read_line(&mut incoming).await.context("reading an event")?;
        if read == 0 {
            bail!("the door closed the feed");
        }
        let event: Inbound =
            serde_json::from_str(incoming.trim_end()).context("parsing an event")?;
        match event.event.as_deref() {
            Some("log") => println!(
                "{} {}",
                event.level.unwrap_or_else(|| "INFO".to_string()),
                event.text.unwrap_or_default(),
            ),
            Some("state") => println!(
                "state {}",
                event
                    .data
                    .as_ref()
                    .and_then(|d| d.get("state"))
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or(event.text)
                    .unwrap_or_default(),
            ),
            Some("lagged") => {
                let missed = event
                    .data
                    .as_ref()
                    .and_then(|d| d.get("missed"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                println!("... missed {missed} event(s) while the feed was full");
            }
            Some(other) => println!("event {other}: {incoming}"),
            None => println!("{incoming}"),
        }
    }
}

async fn ask_following(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    id: u64,
) -> Result<Inbound> {
    let answer = one_round(reader, writer, "log", None, id).await?;
    Ok(answer)
}

#[cfg(test)]
mod exit_pause_tests {
    //! Bug №750 (double-click finding): the usage line a double-clicked
    //! window shows closes with the window in the same blink. The rule lives
    //! in `should_pause_on_exit` — pure, so it is machine-checkable: an
    //! argument list of nothing but the program name is the double-click
    //! shape, and that is the only shape that pauses.

    use super::*;

    #[test]
    fn a_bare_launch_pauses_so_the_usage_survives_the_window() {
        let args: Vec<String> = vec!["ctl.exe".to_string()];
        assert!(
            should_pause_on_exit(&args[1..]),
            "nothing after the program name is the double-click shape"
        );
    }

    #[test]
    fn an_argument_means_a_script_and_no_pause() {
        let scripts = [
            vec!["ctl.exe".to_string(), "status".to_string()],
            vec![
                "ctl.exe".to_string(),
                "--config".to_string(),
                "server_config.json".to_string(),
            ],
            vec![
                "ctl.exe".to_string(),
                "--config".to_string(),
                "server_config.json".to_string(),
                "log".to_string(),
                "--follow".to_string(),
            ],
        ];
        for args in &scripts {
            assert!(
                !should_pause_on_exit(&args[1..]),
                "a scripted launch must exit immediately: {args:?}"
            );
        }
    }
}
