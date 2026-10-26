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
