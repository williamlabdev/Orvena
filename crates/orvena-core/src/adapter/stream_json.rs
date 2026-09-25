//! Normalise a wrapped agent's captured output before Orvena reads it as text.
//!
//! `claude -p` in its default text mode prints only the final assistant
//! message. The sandbox refusals live in *tool results* (`EPERM: operation not
//! permitted, open '/abs/path'` on the Write tool's temp file), which never
//! reach stdout — so on the Claude leg [`super::run`]'s refusal scan was a
//! structural zero, the same "never measured" shape `refused_path`'s doc
//! comment describes for the 2026-08-03 matrix (issue #38).
//!
//! The Claude profile therefore asks for `--output-format stream-json
//! --verbose`: one JSON event per line. This module turns that stream back into
//! the plain transcript the text scanners expect, keeping what carries evidence
//! (tool results, assistant text, the final `result`) and dropping what does
//! not (the `system` init event with its tool list and session id). Lines that
//! are not stream-json events pass through untouched, so every other profile —
//! and a Claude CLI that prints a plain error before the stream starts — is
//! read exactly as before.
//!
//! Pure: no I/O, no branching on what it finds. Same discipline as
//! `refusal_lines` — this only ever makes evidence *visible*; nothing here
//! decides whether the run completed.

use serde_json::Value;

/// What one invocation's output normalises to.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Normalized {
    /// The text transcript: tool results, assistant text and the final result
    /// from stream-json events, plus every non-event line verbatim.
    pub transcript: String,
    /// The agent's own last words: the `result` event's `result` string, or —
    /// when the run ended without one (turn budget, kill) — the last assistant
    /// text block. `None` when the output carried neither.
    pub final_text: Option<String>,
    /// `num_turns` from the `result` event: the agent's *own* turn count
    /// inside this one invocation, which `RunReport::tool_calls` (invocations)
    /// cannot see.
    pub num_turns: Option<u32>,
    /// `(input, output)` from the `result` event's `usage`, relayed as the
    /// agent reported them. Input counts everything that went in — fresh,
    /// cache-creation and cache-read tokens alike.
    pub usage: Option<(u32, u32)>,
}

/// Rebuild a text transcript from `raw`, treating each line that parses as a
/// stream-json event structurally and passing every other line through.
pub(super) fn normalise(raw: &str) -> Normalized {
    let mut out = Normalized::default();
    let mut last_assistant_text: Option<String> = None;
    let mut result_text: Option<String> = None;

    for line in raw.lines() {
        let Some(event) = parse_event(line) else {
            out.transcript.push_str(line);
            out.transcript.push('\n');
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                for text in message_texts(&event, "text") {
                    push_line(&mut out.transcript, &text);
                    last_assistant_text = Some(text);
                }
            }
            Some("user") => {
                for text in tool_result_texts(&event) {
                    push_line(&mut out.transcript, &text);
                }
            }
            Some("result") => {
                if let Some(text) = event.get("result").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        push_line(&mut out.transcript, text);
                        result_text = Some(text.to_string());
                    }
                }
                if let Some(n) = event.get("num_turns").and_then(Value::as_u64) {
                    out.num_turns = Some(u32::try_from(n).unwrap_or(u32::MAX));
                }
                if let Some(usage) = event.get("usage").and_then(Value::as_object) {
                    let count = |k: &str| {
                        usage
                            .get(k)
                            .and_then(Value::as_u64)
                            .map(|v| u32::try_from(v).unwrap_or(u32::MAX))
                    };
                    let input = count("input_tokens")
                        .unwrap_or(0)
                        .saturating_add(count("cache_creation_input_tokens").unwrap_or(0))
                        .saturating_add(count("cache_read_input_tokens").unwrap_or(0));
                    let output = count("output_tokens").unwrap_or(0);
                    out.usage = Some((input, output));
                }
            }
            // The init event (tool list, session id, cwd) and partial-message
            // events carry no evidence — dropped, so the diagnostic excerpt
            // is the agent's words and not its configuration dump.
            Some("system" | "stream_event") => {}
            // A JSON line Orvena does not recognise is still the agent's
            // output; keep it rather than guess.
            _ => {
                out.transcript.push_str(line);
                out.transcript.push('\n');
            }
        }
    }

    out.final_text = result_text.or(last_assistant_text).filter(|t| !t.trim().is_empty());
    out
}

/// A stream-json event: one JSON object per line with a string `type`.
fn parse_event(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let v: Value = serde_json::from_str(trimmed).ok()?;
    v.get("type").and_then(Value::as_str).map(|_| v.clone())
}

fn push_line(transcript: &mut String, text: &str) {
    let t = text.trim_end_matches('\n');
    if t.is_empty() {
        return;
    }
    transcript.push_str(t);
    transcript.push('\n');
}

/// `message.content[*]` items of `kind` with a string `text`.
fn message_texts(event: &Value, kind: &str) -> Vec<String> {
    content_items(event)
        .filter(|item| item.get("type").and_then(Value::as_str) == Some(kind))
        .filter_map(|item| item.get("text").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// The text of every `tool_result` block in a `user` event. The CLI renders a
/// tool result's `content` either as a plain string or as an array of
/// `{type: "text", text}` blocks; both are read.
fn tool_result_texts(event: &Value) -> Vec<String> {
    content_items(event)
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_result"))
        .filter_map(|item| item.get("content"))
        .filter_map(|content| match content {
            Value::String(s) => Some(s.clone()),
            Value::Array(blocks) => {
                let joined: Vec<&str> = blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect();
                (!joined.is_empty()).then(|| joined.join("\n"))
            }
            _ => None,
        })
        .collect()
}

fn content_items(event: &Value) -> impl Iterator<Item = &Value> {
    event
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .map(|a| a.iter())
        .into_iter()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    const INIT: &str = r#"{"type":"system","subtype":"init","cwd":"/x","session_id":"s","tools":["Bash","Write"],"model":"m"}"#;

    fn assistant(text: &str) -> String {
        format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":{}}}]}}}}"#,
            serde_json::to_string(text).unwrap()
        )
    }

    fn tool_result(content: Value) -> String {
        serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": content}
            ]}
        })
        .to_string()
    }

    #[test]
    fn plain_text_passes_through_unchanged() {
        let raw = "Applied edit to src/a.rs\n[Errno 1] Operation not permitted: '/x/tests/it.rs'\n";
        let n = normalise(raw);
        assert_eq!(n.transcript, raw);
        assert_eq!(n.final_text, None);
        assert_eq!(n.num_turns, None);
        assert_eq!(n.usage, None);
    }

    #[test]
    fn a_tool_result_refusal_reaches_the_transcript_from_a_string_or_a_block_list() {
        let eperm = "EPERM: operation not permitted, open '/x/apps/x/y.go.tmp.123'";
        for content in
            [Value::String(eperm.into()), serde_json::json!([{"type": "text", "text": eperm}])]
        {
            let raw = format!("{INIT}\n{}\n", tool_result(content));
            let n = normalise(&raw);
            assert!(n.transcript.contains(eperm), "{}", n.transcript);
            assert!(!n.transcript.contains("session_id"), "init event is dropped");
        }
    }

    #[test]
    fn the_result_event_is_the_final_text_and_carries_turns_and_usage() {
        let raw = format!(
            "{INIT}\n{}\n{}\n",
            assistant("Now I'll check the module setup."),
            serde_json::json!({
                "type": "result", "subtype": "success", "is_error": false,
                "num_turns": 7, "result": "Done: added the doc comment.",
                "usage": {"input_tokens": 10, "cache_creation_input_tokens": 100,
                          "cache_read_input_tokens": 1000, "output_tokens": 42}
            })
        );
        let n = normalise(&raw);
        assert_eq!(n.final_text.as_deref(), Some("Done: added the doc comment."));
        assert_eq!(n.num_turns, Some(7));
        assert_eq!(n.usage, Some((1110, 42)));
        assert!(n.transcript.contains("Now I'll check the module setup."));
        assert!(n.transcript.ends_with("Done: added the doc comment.\n"));
    }

    /// The first #38 data point: twelve turns burned mid-plan, no `result`
    /// text worth the name. The last assistant block is what the agent said.
    #[test]
    fn without_a_result_string_the_last_assistant_text_is_the_final_text() {
        let raw = format!(
            "{}\n{}\n{}\n",
            assistant("Let me look."),
            assistant("Now I'll check the module setup, then write the test file."),
            serde_json::json!({"type": "result", "subtype": "error_max_turns",
                                "is_error": true, "num_turns": 12})
        );
        let n = normalise(&raw);
        assert_eq!(
            n.final_text.as_deref(),
            Some("Now I'll check the module setup, then write the test file.")
        );
        assert_eq!(n.num_turns, Some(12));
    }

    #[test]
    fn json_lines_orvena_does_not_recognise_are_kept_verbatim() {
        let raw = "{\"type\":\"rate_limit_event\",\"x\":1}\n{\"no_type\":true}\nnot json {\n";
        let n = normalise(raw);
        assert_eq!(n.transcript, raw);
    }

    #[test]
    fn a_mixed_stream_keeps_plain_lines_in_place() {
        // A plain error before the stream starts must not vanish.
        let raw = format!("warning: something\n{}\n", assistant("hi"));
        let n = normalise(&raw);
        assert_eq!(n.transcript, "warning: something\nhi\n");
    }
}
