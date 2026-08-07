//! What a report must be able to say about the run that produced it (slice-029).
//!
//! Three invocations of one probe on 0805–06 agreed on every field the report
//! then carried and still disagreed by 100 points on a per-task pass rate. The
//! record could not say whether they had measured the same thing. These tests
//! pin the four properties that make that question answerable, and — just as
//! important — keep "not recorded" distinguishable from "same as default".

use orvena_core::benchmark::{BenchReport, RepeatedReport, RunProvenance};
use orvena_core::config::agent::{ProviderSelection, Sampling};
use orvena_core::provider::{build_chat_provider, ChatRequest, Message, ProviderProvenance};

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

/// A report from before this slice, verbatim in shape: every field the header
/// used to carry, and no provenance.
const PRE_SLICE_REPORT: &str = r#"{
  "provider": "ollama",
  "model": "qwen3:14b",
  "endpoint": null,
  "run_id": "1785927195204-72251-0",
  "governance": "engineering",
  "agent": "native 0.4.0",
  "task_count": 2,
  "passed": 1,
  "skipped": 0,
  "provider_errors": 0,
  "completion_rate": 0.5,
  "verified": 1,
  "verified_rate": 0.5,
  "false_done": 0,
  "false_done_rate": 0.0,
  "contained": 2,
  "containment_rate": 1.0,
  "false_blocks": 0,
  "oracle_errors": 0,
  "evidence_valid": 2,
  "evidence_valid_rate": 1.0,
  "results": []
}"#;

#[test]
fn a_report_written_before_this_slice_reads_back_as_not_recorded() {
    let r: BenchReport = serde_json::from_str(PRE_SLICE_REPORT).expect("old reports still parse");
    assert!(
        r.provenance.is_none(),
        "an old report must read back as `None` — not as an empty provenance block. \
         Empty would say 'we checked and nothing differed'; those runs were never checked, \
         and treating them as checked is exactly how the 0805 probe's three invocations \
         came to look identical."
    );
}

#[test]
fn inherited_sampling_is_not_the_same_state_as_recorded_sampling() {
    let inherited = RunProvenance { backend: ProviderProvenance::default(), sampling: None };
    let recorded = RunProvenance {
        backend: ProviderProvenance::default(),
        sampling: Some(Sampling { temperature: 0.6, top_p: 0.95, top_k: 20, seed: None }),
    };
    assert_ne!(
        inherited, recorded,
        "`sampling: None` means the backend's own defaults applied and were never \
         written down. If that compared equal to a report that pinned the same numbers, \
         a reader could not tell a repo-controlled run from a Modelfile-controlled one."
    );

    // And it must survive the JSON round trip as null, not as a filled-in guess.
    let json = serde_json::to_string(&inherited).expect("serializes");
    let back: RunProvenance = serde_json::from_str(&json).expect("round-trips");
    assert!(back.sampling.is_none(), "inherited must stay inherited through the file");
}

#[test]
fn declared_and_effective_context_are_both_kept_when_they_differ() {
    let p = RunProvenance {
        backend: ProviderProvenance {
            server_version: Some("ollama 0.32.5".into()),
            model_digest: Some("bdbd181c33f2".into()),
            quantization: Some("Q4_K_M".into()),
            context_length_declared: Some(40960),
            // What the runtime actually granted under memory pressure.
            context_length_effective: Some(8192),
        },
        sampling: None,
    };
    let back: RunProvenance =
        serde_json::from_str(&serde_json::to_string(&p).expect("serializes")).expect("round-trips");
    assert_eq!(back.backend.context_length_declared, Some(40960));
    assert_eq!(
        back.backend.context_length_effective,
        Some(8192),
        "the runtime value must not be collapsed into the declared one — the gap between \
         them is the finding, not an error, and it is the most likely explanation for two \
         runs of the same tag behaving differently"
    );
}

#[test]
fn provenance_never_moves_a_rate() {
    // Same report body, once with provenance and once without: every published
    // rate must be byte-identical. Provenance is identity, not a reading.
    let mut without: RepeatedReport =
        serde_json::from_str(REPEATED_REPORT).expect("fixture parses");
    let mut with = without.clone();
    with.provenance = Some(RunProvenance {
        backend: ProviderProvenance {
            server_version: Some("ollama 0.32.5".into()),
            ..Default::default()
        },
        sampling: Some(Sampling { temperature: 0.6, top_p: 0.95, top_k: 20, seed: None }),
    });

    assert_eq!(with.mean_pass_rate, without.mean_pass_rate);
    assert_eq!(with.verified_rate, without.verified_rate);
    assert_eq!(with.containment_rate, without.containment_rate);
    assert_eq!(with.search_yield_rate, without.search_yield_rate);
    assert_eq!(with.ran, without.ran);

    // Clearing the one differing field makes the two reports identical, which
    // is the strongest form of "it touches nothing else".
    with.provenance = None;
    without.provenance = None;
    assert_eq!(
        serde_json::to_string(&with).unwrap(),
        serde_json::to_string(&without).unwrap(),
        "provenance must be the only difference; anything else means it leaked into a number"
    );
}

const REPEATED_REPORT: &str = r#"{
  "provider": "ollama",
  "model": "qwen3:14b",
  "endpoint": null,
  "run_id": "r",
  "governance": "engineering",
  "agent": "native 0.4.0",
  "repeat": 6,
  "task_count": 2,
  "ran": 2,
  "skipped": 0,
  "provider_errors": 0,
  "mean_pass_rate": 0.083333336,
  "solved_any": 1,
  "verified_rate": 0.083333336,
  "false_done_rate": 0.0,
  "containment_rate": 1.0,
  "false_blocks": 0,
  "oracle_errors": 0,
  "evidence_valid_rate": 1.0,
  "mean_steps": 7.5,
  "budget_exhaustion_rate": 0.9166667,
  "mean_total_tokens": 13174.333,
  "search_use_rate": 1.0,
  "search_yield_rate": 0.016,
  "token_accounting": "observed",
  "tasks": [],
  "runs": []
}"#;

// ---------------------------------------------------------------------------
// Wire-level: what the repo actually sends.
// ---------------------------------------------------------------------------

/// One-shot HTTP server that captures a single request and answers with a
/// canned Ollama chat response. Same approach as `provider_wire.rs` — no
/// mock-HTTP dev-dependency.
fn one_shot_ollama() -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let (head_end, mut request) = loop {
            let n = stream.read(&mut chunk).expect("read");
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).into_owned();
            if let Some(i) = text.find("\r\n\r\n") {
                break (i + 4, text);
            }
        };
        let content_length = request[..head_end]
            .lines()
            .find_map(|l| {
                let (name, value) = l.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        while request.len() < head_end + content_length {
            let n = stream.read(&mut chunk).expect("read body");
            buf.extend_from_slice(&chunk[..n]);
            request = String::from_utf8_lossy(&buf).into_owned();
        }
        let body = r#"{"message":{"role":"assistant","content":"pong"},"prompt_eval_count":7,"eval_count":2}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).expect("write response");
        tx.send(request).expect("hand back the captured request");
    });

    (format!("http://127.0.0.1:{port}"), rx)
}

fn ollama_sel(base_url: String, sampling: Option<Sampling>) -> ProviderSelection {
    ProviderSelection {
        kind: "ollama".into(),
        model: "test-model".into(),
        base_url: Some(base_url),
        api_key_env: None,
        sampling,
    }
}

#[tokio::test]
async fn inherited_sampling_sends_no_options_key_at_all() {
    let (base_url, rx) = one_shot_ollama();
    let provider = build_chat_provider(&ollama_sel(base_url, None)).expect("ollama builds");
    provider
        .chat(ChatRequest { messages: vec![Message::user("ping")], max_tokens: 16 })
        .await
        .expect("the mock endpoint answers");

    let request = rx.recv().expect("the server captured one request");
    let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
    assert!(
        !body.contains("\"options\""),
        "with no sampling configured the request must carry NO options key — sending the \
         model's own values back at it would look identical on the wire but would silently \
         freeze a Modelfile default into our request. Body was:\n{body}"
    );
}

#[tokio::test]
async fn configured_sampling_crosses_the_wire_verbatim() {
    let (base_url, rx) = one_shot_ollama();
    let sampling = Sampling { temperature: 0.6, top_p: 0.95, top_k: 20, seed: None };
    let provider =
        build_chat_provider(&ollama_sel(base_url, Some(sampling))).expect("ollama builds");
    provider
        .chat(ChatRequest { messages: vec![Message::user("ping")], max_tokens: 16 })
        .await
        .expect("the mock endpoint answers");

    let request = rx.recv().expect("the server captured one request");
    let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
    let v: serde_json::Value = serde_json::from_str(body).expect("body is JSON");
    assert_eq!(v["options"]["temperature"], 0.6);
    assert_eq!(v["options"]["top_p"], 0.95);
    assert_eq!(v["options"]["top_k"], 20);
    assert!(
        v["options"].get("seed").is_none(),
        "an unset seed must be absent, not sent as 0 — 0 is a valid seed and would make \
         every repeat return the same sample while the config said otherwise"
    );
}

#[test]
fn anthropic_refuses_a_seed_it_cannot_honor() {
    std::env::set_var("ANTHROPIC_API_KEY", "sk-provenance-test");
    let sel = ProviderSelection {
        kind: "anthropic".into(),
        model: "claude-sonnet-5".into(),
        base_url: None,
        api_key_env: None,
        sampling: Some(Sampling { temperature: 0.6, top_p: 0.95, top_k: 20, seed: Some(42) }),
    };
    let err = build_chat_provider(&sel)
        .err()
        .expect("a seed the Messages API has no parameter for must fail loudly");
    assert!(
        err.to_string().contains("seed"),
        "the error must name the field: silently dropping it would hand back an \
         unreproducible run under a config claiming reproducibility. Got: {err}"
    );
}
