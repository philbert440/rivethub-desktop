use std::path::PathBuf;
use std::time::Duration;

use app::Hub;
use gateway::{Endpoint, Gateway};
use model::HarnessId;
use model::session::Role;

fn endpoint() -> Endpoint {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    Endpoint {
        base_url: std::env::var("RIVETHUB_URL").unwrap_or_else(|_| "https://127.0.0.1:5174".into()),
        ca_pem: std::env::var_os("RIVETHUB_CA")
            .map_or_else(|| home.join(".rivethub/step-root.crt"), PathBuf::from),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a running rh gateway; run with --ignored"]
async fn a_turn_streams_back_into_the_session_view() -> Result<(), Box<dyn std::error::Error>> {
    let hub = Hub::new(tokio::runtime::Handle::current(), Gateway::connect(&endpoint())?);
    let summary = hub.start_session(HarnessId::new("claude-code")).await?;
    let session = hub.open_session(summary.id);
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
#[ignore = "needs a running rh gateway; run with --ignored"]
async fn a_session_terminal_draws_the_harness_tui() -> Result<(), Box<dyn std::error::Error>> {
    let hub = Hub::new(tokio::runtime::Handle::current(), Gateway::connect(&endpoint())?);
    let summary = hub.start_session(HarnessId::new("claude-code")).await?;
    let terminal = hub.open_terminal(
        summary.id,
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
    terminal.write(b"/exit\r".to_vec());
    assert!(drew, "the terminal stream closed before the TUI drew anything");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a running rh gateway; run with --ignored"]
async fn a_message_sent_mid_turn_waits_in_the_queue() -> Result<(), Box<dyn std::error::Error>> {
    let hub = Hub::new(tokio::runtime::Handle::current(), Gateway::connect(&endpoint())?);
    let summary = hub.start_session(HarnessId::new("claude-code")).await?;
    let session = hub.open_session(summary.id);
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
