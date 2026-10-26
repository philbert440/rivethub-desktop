use std::path::PathBuf;
use std::time::Duration;

use app::Hub;
use gateway::{Endpoint, Gateway};
use model::HarnessId;
use model::session::Role;

fn hub() -> Result<Hub, gateway::GatewayError> {
    let endpoint = endpoint();
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let dir = home.join(".config/RivetHub/mtls");
    let identity = gateway::Identity {
        cert_pem: dir.join("device.crt"),
        key_pem: dir.join("device.key"),
    };
    Ok(Hub::new(
        tokio::runtime::Handle::current(),
        Gateway::connect(&endpoint)?,
        endpoint.ca_pem,
        Some(identity),
    ))
}

fn endpoint() -> Endpoint {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    Endpoint {
        base_url: std::env::var("RIVETHUB_URL").unwrap_or_else(|_| "https://127.0.0.1:5174".into()),
        ca_pem: std::env::var_os("RIVETHUB_CA")
            .map_or_else(|| home.join(".rivethub/step-root.crt"), PathBuf::from),
        identity: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a running rh gateway; run with --ignored --test-threads=1"]
async fn a_turn_streams_back_into_the_session_view() -> Result<(), Box<dyn std::error::Error>> {
    let hub = hub()?;
    let summary = hub.start_session(HarnessId::new("claude-code")).await?;
    let session = hub.open_session(&summary);
    let mut revisions = session.revisions();
    session.send("Reply with exactly the word: ready".into());
    let answered = tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            if revisions.changed().await.is_err() {
                return false;
            }
            let view = session.view();
            let view = view.read();
            if view
                .turns()
                .iter()
                .any(|turn| turn.role == Role::Assistant && turn.text.contains("ready"))
            {
                return true;
            }
        }
    })
    .await?;
    session.stop();
    assert!(answered, "the session stream closed before the reply arrived");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a running rh gateway; run with --ignored --test-threads=1"]
async fn a_session_terminal_draws_the_harness_tui() -> Result<(), Box<dyn std::error::Error>> {
    let hub = hub()?;
    let summary = hub.start_session(HarnessId::new("claude-code")).await?;
    let terminal = hub.open_terminal(
        &summary,
        term::GridSize {
            columns: 100,
            rows: 30,
        },
    );
    let mut revisions = terminal.revisions();
    let drew = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if revisions.changed().await.is_err() {
                return false;
            }
            let screen = terminal.screen();
            let snapshot = screen.read().snapshot();
            let text: usize = snapshot
                .lines
                .iter()
                .flatten()
                .map(|span| span.text.trim().len())
                .sum();
            if text > 40 {
                return true;
            }
        }
    })
    .await?;
    terminal.end();
    let exited = tokio::time::timeout(Duration::from_secs(20), async {
        while terminal.state() != app::TerminalState::Exited {
            if revisions.changed().await.is_err() {
                break;
            }
        }
    })
    .await;
    assert!(drew, "the terminal stream closed before the TUI drew anything");
    assert!(exited.is_ok(), "the terminal did not end");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a running rh gateway; run with --ignored --test-threads=1"]
async fn a_message_sent_mid_turn_waits_in_the_queue() -> Result<(), Box<dyn std::error::Error>> {
    let hub = hub()?;
    let summary = hub.start_session(HarnessId::new("claude-code")).await?;
    let session = hub.open_session(&summary);
    let mut revisions = session.revisions();
    session.send("Count from 1 to 60, one number per line, then write the word first.".into());
    tokio::time::timeout(Duration::from_secs(30), async {
        while revisions.changed().await.is_ok() && !session.view().read().busy() {}
    })
    .await?;
    session.send("Reply with exactly the word: second".into());
    tokio::time::sleep(Duration::from_millis(200)).await;
    let waited = session.queued() == vec!["Reply with exactly the word: second".to_owned()];
    let answered = tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            if revisions.changed().await.is_err() {
                return false;
            }
            let view = session.view();
            let view = view.read();
            let replies: Vec<&str> = view
                .turns()
                .iter()
                .filter(|turn| turn.role == Role::Assistant)
                .map(|turn| turn.text.as_str())
                .collect();
            if replies.iter().any(|text| text.contains("second")) && session.queued().is_empty() {
                return replies.iter().any(|text| text.contains("first"));
            }
        }
    })
    .await?;
    session.stop();
    assert!(waited, "the second message was not held while the first turn ran");
    assert!(answered, "the queued message was never sent after the first turn");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a running rh gateway; run with --ignored --test-threads=1"]
async fn the_panel_reads_agents_and_groups_search_hits() -> Result<(), Box<dyn std::error::Error>> {
    let hub = hub()?;
    let roster = hub.agents().await?;
    assert!(!roster.node.is_empty(), "the gateway did not name its node");
    assert!(!roster.agents.is_empty(), "no agent presets came back");
    let found = hub.search("renewal deadline".into()).await?;
    assert!(!found.is_empty(), "search found no sessions");
    let sessions: std::collections::HashSet<_> = found.iter().map(|hit| hit.session.clone()).collect();
    assert_eq!(sessions.len(), found.len(), "a session appeared twice");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the mesh and a device certificate; run with --ignored"]
async fn an_agent_on_another_node_starts_and_answers_there() -> Result<(), Box<dyn std::error::Error>> {
    let hub = hub()?;
    let roster = hub.agents().await?;
    let remote = roster
        .agents
        .into_iter()
        .find(|agent| {
            agent
                .harness
                .as_ref()
                .is_some_and(|harness| harness.as_str() == "claude-code")
                && agent
                    .node
                    .as_deref()
                    .is_some_and(|node| !node.eq_ignore_ascii_case(&roster.node))
        })
        .ok_or("no claude-code agent on another node")?;
    let summary = hub.start_with(remote.clone()).await?;
    assert_eq!(summary.node, remote.node);
    let session = hub.open_session(&summary);
    let mut revisions = session.revisions();
    session.send("Reply with exactly the word: remote".into());
    let answered = tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            if revisions.changed().await.is_err() {
                return false;
            }
            let view = session.view();
            let view = view.read();
            if view
                .turns()
                .iter()
                .any(|turn| turn.role == Role::Assistant && turn.text.contains("remote"))
            {
                return true;
            }
        }
    })
    .await?;
    session.stop();
    assert!(answered, "the remote session never answered");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a running rh gateway; run with --ignored --test-threads=1"]
async fn the_workspace_feed_notices_a_new_turn() -> Result<(), Box<dyn std::error::Error>> {
    let hub = hub()?;
    let feed = hub.watch_workspace();
    let mut changes = feed.changes();
    tokio::time::timeout(Duration::from_secs(10), changes.changed()).await??;
    let summary = hub.start_session(HarnessId::new("claude-code")).await?;
    let session = hub.open_session(&summary);
    session.send("Reply with exactly the word: noticed".into());
    let seen = *changes.borrow_and_update();
    let ticked = tokio::time::timeout(Duration::from_secs(120), async {
        while changes.changed().await.is_ok() {
            if *changes.borrow_and_update() > seen {
                return true;
            }
        }
        false
    })
    .await?;
    session.stop();
    assert!(ticked, "the feed never reported the new session's turn");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a running rh gateway with topics; run with --ignored --test-threads=1"]
async fn a_session_placed_in_a_new_topic_reads_back_under_it() -> Result<(), Box<dyn std::error::Error>> {
    let hub = hub()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    let session = model::SessionId::new(
        HarnessId::new("claude-code"),
        model::NativeSessionId::new(format!("00000000-0000-4000-8000-{stamp:012}")),
    );
    let unplaced = hub.suggestions(vec![session.clone()]).await?;
    assert!(unplaced.is_empty());
    let name = format!("Desktop smoke {stamp}");
    let created = hub.place_in_new(session.clone(), name.clone()).await?;
    let (topics, placements) = hub.topics().await?;
    assert!(
        topics
            .iter()
            .any(|topic| topic.id == created.id && topic.name == name)
    );
    assert_eq!(placements.get(&session), Some(&created.id));
    let found = hub.search_topics(name.to_lowercase()).await?;
    assert_eq!(found.first().map(|topic| &topic.id), Some(&created.id));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a running rh gateway; run with --ignored --test-threads=1"]
async fn a_command_tool_call_arrives_typed_with_its_output() -> Result<(), Box<dyn std::error::Error>> {
    let hub = hub()?;
    let summary = hub.start_session(HarnessId::new("claude-code")).await?;
    let session = hub.open_session(&summary);
    let mut revisions = session.revisions();
    session.send("Use the Bash tool to run exactly: echo tool-check-42 . Then reply done.".into());
    let seen = tokio::time::timeout(Duration::from_secs(240), async {
        loop {
            if revisions.changed().await.is_err() {
                return None;
            }
            let view = session.view();
            let view = view.read();
            for approval in view.approvals() {
                session.decide(approval.request_id.clone(), model::session::Decision::Allow);
            }
            let finished = view.turns().iter().chain(view.live()).flat_map(|turn| &turn.tools).find(|tool| {
                matches!(&tool.input, model::tools::ToolInput::Command { command, .. } if command.contains("tool-check-42"))
                    && tool.output.as_ref().is_some_and(|output| output.text.contains("tool-check-42"))
            });
            if let Some(tool) = finished {
                return Some(tool.clone());
            }
        }
    })
    .await?;
    session.stop();
    assert!(
        seen.is_some(),
        "no Bash call with its command and output reached the view"
    );
    Ok(())
}
