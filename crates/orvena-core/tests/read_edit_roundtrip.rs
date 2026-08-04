//! Round-trip test of the READ/EDIT protocol (slice-020): the agent READs a
//! file on step 1, the driver feeds the content back as evidence, and on step 2
//! the (scripted) model EDITs anchored on *text extracted from that evidence*.
//! This proves the loop can "read → anchor an edit on what it read", not just
//! that two more tools exist.

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

/// A deterministic two-step script: first READ the file, then EDIT it anchored
/// on a line *taken from the read evidence fed back in the prompt*.
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
            0 => "<<<READ config.txt\n>>>".to_string(),
            _ => {
                // Anchor on the exact line the READ evidence carried. If the
                // content was not fed back, the anchor is garbage and the edit
                // (and then the gate) must fail — which is the point.
                let anchor = prompt
                    .lines()
                    .find(|l| l.trim().starts_with("retries = "))
                    .map(str::trim)
                    .unwrap_or("READ RESULTS WERE NOT FED BACK");
                format!("<<<EDIT config.txt\n{anchor}\n===\nretries = 5\n>>>")
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
                allowed_tools: vec!["fs.read".into(), "fs.write".into()],
                forbidden_tools: vec![],
                knowledge_scope: vec![],
            }],
        },
        gates: Gates {
            gates: vec![Gate {
                name: "retries-bumped".into(),
                condition: "config.txt has retries = 5 and the untouched line intact".into(),
                verify: Some(
                    "grep -q 'retries = 5' config.txt && grep -q 'timeout = 30' config.txt".into(),
                ),
                gatekeeper: Gatekeeper::Automated,
                timeout_secs: None,
            }],
        },
        budgets: ContextBudgets::default(),
        commands: Commands::default(),
    }
}

#[tokio::test]
async fn read_evidence_drives_the_next_edit() {
    let root = temp_dir("read-edit-roundtrip");
    std::fs::write(root.join("config.txt"), "retries = 1\ntimeout = 30\n").unwrap();

    let agent =
        Agent::with_provider(config(), &root, Box::new(Scripted { calls: AtomicUsize::new(0) }));
    let task = Task::new("Bump retries to 5", vec!["config.txt".into()]);
    let report = agent.run(task).await.unwrap();

    assert!(report.completed, "gate should pass; blockers: {:?}", report.blockers);
    assert_eq!(report.steps, 2, "step 1 reads, step 2 edits");
    assert!(report.tool_calls >= 2, "one read + one edit");

    // The edit is surgical: the anchored line changed, the neighbor did not.
    let written = std::fs::read_to_string(root.join("config.txt")).unwrap();
    assert_eq!(written, "retries = 5\ntimeout = 30\n", "anchored replace, not a rewrite");

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn an_ambiguous_anchor_is_a_blocker_but_does_not_stop_the_loop() {
    let root = temp_dir("edit-ambiguous");
    std::fs::write(root.join("config.txt"), "retries = 1\nretries = 1\n").unwrap();

    struct Ambiguous;
    #[async_trait]
    impl Provider for Ambiguous {
        fn id(&self) -> &str {
            "scripted"
        }
        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse> {
            Ok(ChatResponse {
                content: "<<<EDIT config.txt\nretries = 1\n===\nretries = 5\n>>>".into(),
                input_tokens: 0,
                output_tokens: 0,
            })
        }
    }

    let agent = Agent::with_provider(config(), &root, Box::new(Ambiguous));
    let task = Task::new("Bump retries", vec!["config.txt".into()]);
    let report = agent.run(task).await.unwrap();

    assert!(!report.completed, "the gate never passes");
    assert_eq!(report.steps, 3, "an anchor failure must not abort the bounded loop");
    assert!(
        report.blockers.iter().any(|b| b.contains("2 times")),
        "the ambiguity is recorded as a blocker: {:?}",
        report.blockers
    );
    // And the file was never touched.
    assert_eq!(
        std::fs::read_to_string(root.join("config.txt")).unwrap(),
        "retries = 1\nretries = 1\n"
    );

    let _ = std::fs::remove_dir_all(&root);
}
