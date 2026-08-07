//! Wire-level proof of `openai_compat`'s authentication contract.
//!
//! The unit tests assert `api_key.is_none()` on the built struct; these assert
//! what actually crosses the socket, because the claim that matters is about
//! the *request*: a keyless config sends **no `Authorization` header at all**
//! (an empty or garbage bearer would break servers that validate whatever is
//! presented), and a keyed config sends exactly `Bearer <value>`.
//!
//! The mock endpoint is a plain `std::net::TcpListener` on an OS thread — no
//! mock-HTTP dev-dependency — that captures one request verbatim and answers
//! with a canned chat completion.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use orvena_core::config::agent::ProviderSelection;
use orvena_core::provider::{build_chat_provider, ChatRequest, Message};

/// One-shot HTTP server: accepts a single connection, captures the full request
/// (head + body), answers with a minimal valid chat completion, and hands the
/// captured request back. Returns the base_url to point the provider at.
fn one_shot_server() -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        // Read until the header/body split, then drain the declared body.
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

        let body = r#"{"choices":[{"message":{"role":"assistant","content":"pong"}}],"usage":{"prompt_tokens":7,"completion_tokens":2}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).expect("write response");
        tx.send(request).expect("hand back the captured request");
    });

    (format!("http://127.0.0.1:{port}/v1"), rx)
}

fn header_names(request: &str) -> Vec<String> {
    request
        .split("\r\n\r\n")
        .next()
        .unwrap_or("")
        .lines()
        .skip(1)
        .filter_map(|l| l.split_once(':'))
        .map(|(name, _)| name.trim().to_ascii_lowercase())
        .collect()
}

#[tokio::test]
async fn a_keyless_config_sends_no_authorization_header_at_all() {
    let (base_url, rx) = one_shot_server();
    let sel = ProviderSelection {
        kind: "openai_compat".into(),
        model: "test-model".into(),
        base_url: Some(base_url),
        api_key_env: None,
        sampling: None,
    };

    let provider = build_chat_provider(&sel).expect("keyless openai_compat builds");
    let resp = provider
        .chat(ChatRequest { messages: vec![Message::user("ping")], max_tokens: 16 })
        .await
        .expect("the mock endpoint answers");
    assert_eq!(resp.content, "pong", "sanity: the canned completion round-trips");

    let request = rx.recv().expect("the server captured one request");
    assert!(
        !header_names(&request).iter().any(|h| h == "authorization"),
        "a keyless openai_compat must not send ANY Authorization header \
         (empty/garbage bearers break servers that validate what is presented); \
         request head was:\n{}",
        request.split("\r\n\r\n").next().unwrap_or("")
    );
}

#[tokio::test]
async fn a_keyed_config_sends_exactly_the_bearer_it_was_pointed_at() {
    // Var name unique to this test: tests share a process, and the assertion
    // must not race another test's environment.
    const KEY_VAR: &str = "ORVENA_WIRE_TEST_KEY";
    std::env::set_var(KEY_VAR, "sk-wire-test-123");

    let (base_url, rx) = one_shot_server();
    let sel = ProviderSelection {
        kind: "openai_compat".into(),
        model: "test-model".into(),
        base_url: Some(base_url),
        api_key_env: Some(KEY_VAR.into()),
        sampling: None,
    };

    let provider = build_chat_provider(&sel).expect("keyed openai_compat builds");
    provider
        .chat(ChatRequest { messages: vec![Message::user("ping")], max_tokens: 16 })
        .await
        .expect("the mock endpoint answers");

    let request = rx.recv().expect("the server captured one request");
    let auth = request
        .split("\r\n\r\n")
        .next()
        .unwrap_or("")
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("authorization").then(|| value.trim().to_string())
        })
        .expect("a keyed config must authenticate");
    assert_eq!(auth, "Bearer sk-wire-test-123");
}
