//! Round-trip test of the search protocol: the agent SEARCHes on step 1, the
//! driver feeds the hits back as evidence, and on step 2 the (scripted) model
//! WRITEs content derived from those hits. This proves the loop can
//! "search → use the results to change a file", not just call a tool.

use async_trait::async_trait;
use orvena_core::config::agent::{AgentConfig, ProviderSelection, Tier};
use orvena_core::config::commands::Commands;
use orvena_core::config::context_budget::ContextBudgets;
use orvena_core::config::gates::{Gate, Gatekeeper, Gates};
use orvena_core::config::roles::{Role, Roles};
use orvena_core::config::Config;
use orvena_core::provider::{ChatRequest, ChatResponse, Provider};
use orvena_core::{Agent, Result, Task};
use std::sync::atomic::{AtomicUsize, Ordering};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("orvena-test-{tag}-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A deterministic two-step script: first search the repo, then write a file
/// whose content is *extracted from the search hits fed back in the prompt*.
struct Scripted {
    calls: AtomicUsize,
}

#[async_trait]
impl Provider for Scripted {
    fn id(&self) -> &str {
        "scripted"
    }

    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let prompt: String =
            req.messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n");

        let content = match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => "<<<SEARCH TODO\n>>>".to_string(),
            _ => {
                // Read the greeting out of the search hit the driver fed back
                // (e.g. "  notes.txt:1: TODO: greet politely").
                let greeting = prompt
                    .lines()
                    .find(|l| l.contains("notes.txt:"))
                    .and_then(|l| l.split("TODO: ").nth(1))
                    .unwrap_or("SEARCH RESULTS WERE NOT FED BACK");
                format!("<<<WRITE hello.txt\n{greeting}\n>>>")
            }
        };

        Ok(ChatResponse { content, input_tokens: 0, output_tokens: 0 })
    }
}

fn config() -> Config {
    Config {
        agent: AgentConfig {
            provider: ProviderSelection {
                kind: "offline".into(),
                model: "scripted".into(),
                base_url: None,
                api_key_env: None,
            },
            tier: Tier::Engineering,
            default_role: "developer".into(),
            max_steps: 3,
            sandbox: Default::default(),
        },
        roles: Roles {
            roles: vec![Role {
                name: "developer".into(),
                allowed_tools: vec!["fs.read".into(), "fs.write".into(), "grep.search".into()],
                forbidden_tools: vec![],
                knowledge_scope: vec![],
            }],
        },
        gates: Gates {
            gates: vec![Gate {
                name: "greeting-from-search".into(),
                condition: "hello.txt contains the greeting found via search".into(),
                verify: Some("grep -q 'greet politely' hello.txt".into()),
                gatekeeper: Gatekeeper::Automated,
                timeout_secs: None,
            }],
        },
        budgets: ContextBudgets::default(),
        commands: Commands::default(),
    }
}

#[tokio::test]
async fn search_results_drive_the_next_write() {
    let root = temp_dir("search-roundtrip");
    std::fs::write(root.join("notes.txt"), "TODO: greet politely\n").unwrap();

    let agent =
        Agent::with_provider(config(), &root, Box::new(Scripted { calls: AtomicUsize::new(0) }));
    let task = Task::new("Find the TODO and write the greeting", vec!["hello.txt".into()]);
    let report = agent.run(task).await.unwrap();

    assert!(report.completed, "gate should pass; blockers: {:?}", report.blockers);
    assert_eq!(report.steps, 2, "step 1 searches, step 2 writes");
    assert!(report.tool_calls >= 2, "one search + one write");
    // Per-action attribution (slice-026): `tool_calls` alone cannot answer
    // "did the loop search?", which is the question the ruler keeps asking.
    let counts = report.action_counts.expect("the native loop attributes its own actions");
    assert_eq!(counts.search, 1, "one SEARCH emitted");
    assert_eq!(counts.write, 1, "one WRITE emitted");
    assert_eq!(counts.read, 0);

    let written = std::fs::read_to_string(root.join("hello.txt")).unwrap();
    assert_eq!(written.trim(), "greet politely", "content must come from the search hit");

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn invalid_regex_is_a_blocker_but_does_not_stop_the_loop() {
    let root = temp_dir("search-badregex");

    struct BadRegex;
    #[async_trait]
    impl Provider for BadRegex {
        fn id(&self) -> &str {
            "scripted"
        }
        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse> {
            Ok(ChatResponse {
                content: "<<<SEARCH (unclosed\n>>>".into(),
                input_tokens: 0,
                output_tokens: 0,
            })
        }
    }

    let agent = Agent::with_provider(config(), &root, Box::new(BadRegex));
    let task = Task::new("Search with a broken pattern", vec!["hello.txt".into()]);
    let report = agent.run(task).await.unwrap();

    assert!(!report.completed, "the gate never passes");
    assert_eq!(report.steps, 3, "a bad regex must not abort the bounded loop");
    assert!(
        report.blockers.iter().any(|b| b.contains("invalid search pattern")),
        "the regex failure is recorded as a blocker: {:?}",
        report.blockers
    );

    let _ = std::fs::remove_dir_all(&root);
}
