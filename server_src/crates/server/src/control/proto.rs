//! The control-port wire protocol: newline-delimited JSON, one message per
//! line, the first message always the token hello. See
//! `.DOC/new/server-interface-plan.md` (Приложение А, А.4) — the forms there
//! are the contract:
//!
//! ```text
//! → {"hello":{"token":"…","proto":1}}
//! ← {"hello":"ok","control_proto":1}
//! → {"id":1,"cmd":"status"}
//! ← {"id":1,"ok":true,"data":{…}}
//! ← {"id":2,"ok":false,"error":{"code":"bad_arguments","text":"…"}}
//! ```
//!
//! Reading rules from the plan: a reader ignores fields it does not know (so
//! adding a field never breaks an older `ctl`); removing or re-meaning a field
//! is a `CONTROL_PROTO` bump a note next to it has to justify.

use serde::Deserialize;

/// The protocol revision this server speaks. Where a value 1 is expected, a
/// bigger or different one is refused with a text that names the difference —
/// silently accepting a stranger schema is how a client and a server stop
/// meaning the same words by "status".
pub const CONTROL_PROTO: u32 = 1;

/// The hello the connection opens with. The token is the file token the
/// server generates on first start; it is never logged, never echoed back.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Hello {
    pub token: String,
    #[serde(default)]
    pub proto: u32,
}

/// A command after the hello. `id` rides on the response unchanged; a request
/// without one is refused — a stream where responses cannot be matched to
/// requests made `--follow`-style clients guess.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Command {
    pub id: u64,
    pub cmd: String,
    ///free-form per-command arguments; each command names what it accepts and
    /// everything else is ignored, the way the plan's reading rules promise.
    #[serde(default)]
    pub args: Option<serde_json::Value>,
}

/// A parsed inbound line. Parsing never trusts the shape further than this:
/// the dispatch below owns the "unknown name" and "unknown argument" answers.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Hello(Hello),
    Command(Command),
}

/// Why a line could not become a request the protocol understands.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// Not JSON at all — a broken client, not a client speaking v1 wrong.
    NotJson,
    /// JSON of the right dialect but the wrong shape; the string is serde's
    /// own complaint ("missing field `token`"), which is exactly the named
    /// refusal the negative checks ask for.
    Malformed(String),
    /// A bare newline / whitespace line: a dead keep-alive, not a request.
    Empty,
}

/// Parses one inbound line (the trailing newline already stripped by the
/// reader). Anything that is not a hello and not a command is an error the
/// caller reports as `bad_arguments` — the plan's codes cover the whole
/// refusal space, and a protocol-framing miss is a caller-side badness, not
/// an unknown *command*.
pub fn parse_line(line: &str) -> Result<Incoming, ParseError> {
    if line.trim().is_empty() {
        // A bare newline is a dead client's keep-alive, not a request: report
        // it as empty so the caller closes instead of answering.
        return Err(ParseError::Empty);
    }
    let value: serde_json::Value =
        serde_json::from_str(line).map_err(|_| ParseError::NotJson)?;
    // A "hello" key makes the line a hello attempt: whatever else it carries
    // cannot rescue it into a command (the auth gate must see it first). The
    // Hello struct describes the *inner* object — the token lives inside it,
    // not on the envelope.
    if let Some(hello_value) = value.get("hello") {
        let hello: Hello =
            serde_json::from_value(hello_value.clone())
                .map_err(|e| ParseError::Malformed(e.to_string()))?;
        return Ok(Incoming::Hello(hello));
    }
    let command: Command =
        serde_json::from_value(value).map_err(|e| ParseError::Malformed(e.to_string()))?;
    Ok(Incoming::Command(command))
}

/// `true` when two plain-text reads produce the same incoming line for the
/// same bytes — whitespace between fields is the sender's business, not the
/// protocol's. Locked by a test so a future strict format change announces
/// itself as one.
pub fn whitespace_between_fields_is_insignificant() -> bool {
    let a = parse_line(r#"{"id":1,"cmd":"status"}"#);
    let b = parse_line(r#"{"id":1, "cmd" : "status"}"#);
    a == b
}

#[cfg(test)]
mod proto_tests {
    use super::*;

    fn hello(line: &str) -> Hello {
        match parse_line(line).expect("the line parses") {
            Incoming::Hello(h) => h,
            other => panic!("expected a hello, got {other:?}"),
        }
    }

    fn command(line: &str) -> Command {
        match parse_line(line).expect("the line parses") {
            Incoming::Command(c) => c,
            other => panic!("expected a command, got {other:?}"),
        }
    }

    #[test]
    fn a_hello_line_parses_with_its_token_and_proto() {
        let h = hello(r#"{"hello":{"token":"abc","proto":1}}"#);
        assert_eq!(h.token, "abc");
        assert_eq!(h.proto, 1);
        // A hello with no proto rides as 0 — the comparison against
        // `CONTROL_PROTO` then refuses it with a text naming the versions.
        let no_proto = hello(r#"{"hello":{"token":"abc"}}"#);
        assert_eq!(no_proto.proto, 0);
    }

    #[test]
    fn a_command_line_parses_with_its_id_cmd_and_optional_args() {
        let c = command(r#"{"id":7,"cmd":"log","args":{"tail":20}}"#);
        assert_eq!(c.id, 7);
        assert_eq!(c.cmd, "log");
        assert_eq!(
            c.args.expect("args are carried"),
            serde_json::json!({"tail": 20})
        );
        let bare = command(r#"{"id":7,"cmd":"status"}"#);
        assert!(bare.args.is_none(), "args stay optional");
    }

    #[test]
    fn a_command_with_unknown_fields_still_parses_a_known_one() {
        // The reading rule: unknown fields are ignored, so a newer ctl sending
        // an extra field to an older server is not a rejection.
        let c = command(r#"{"id":1,"cmd":"status","future_field":{"x":1}}"#);
        assert_eq!(c.cmd, "status");
    }

    #[test]
    fn anything_else_is_a_parse_error_not_a_panic() {
        assert_eq!(parse_line("not json"), Err(ParseError::NotJson));
        assert_eq!(parse_line("\r\n"), Err(ParseError::Empty));
        assert_eq!(parse_line("   "), Err(ParseError::Empty));
        // Pronounced shape leftovers carry serde's complaint — the reason a
        // "token not transmitted" question receives the field name as its
        // response.
        match parse_line("[1,2,3]") {
            Err(ParseError::Malformed(text)) => assert!(text.contains("type"), "{text}"),
            other => panic!("expected Malformed, got {other:?}"),
        }
        match parse_line(r#"{"cmd":"status"}"#) {
            Err(ParseError::Malformed(text)) => assert!(text.contains("id"), "{text}"),
            other => panic!("expected Malformed naming the id, got {other:?}"),
        }
        match parse_line(r#"{"id":1}"#) {
            Err(ParseError::Malformed(text)) => assert!(text.contains("cmd"), "{text}"),
            other => panic!("expected Malformed naming the cmd, got {other:?}"),
        }
        match parse_line(r#"{"hello":{"proto":1}}"#) {
            Err(ParseError::Malformed(text)) => assert!(text.contains("token"), "{text}"),
            other => panic!("expected Malformed naming the token, got {other:?}"),
        }
    }

    #[test]
    fn whitespace_between_fields_does_not_change_the_parse() {
        assert!(whitespace_between_fields_is_insignificant());
    }

    /// The hello wins over command-shaped leftovers: a line that carries both
    /// is processed as the hello (the auth gate must see it, whatever else a
    /// buggy client bundled with it).
    #[test]
    fn a_line_with_both_hello_and_command_is_framed_as_hello() {
        let mixed = parse_line(r#"{"hello":{"token":"a","proto":1},"id":1,"cmd":"status"}"#);
        assert!(matches!(mixed, Ok(Incoming::Hello(_))), "got {mixed:?}");
    }
}
