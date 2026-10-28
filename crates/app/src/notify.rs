use std::time::Duration;

use notify_rust::{Hint, Notification, NotificationResponse};

use crate::Hub;

const APP: &str = "rivethub-desktop";
/// How long a click is waited for. Each wait holds a session-bus connection, and a server that
/// keeps notifications in a history may never report them closed.
const CLICK_WINDOW: Duration = Duration::from_mins(30);

impl Hub {
    /// Shows a desktop notification and resolves to whether the person clicked it. Resolves to
    /// `false` when the notification closes unclicked, goes unclicked for half an hour, or the
    /// desktop has no notification server.
    pub async fn notify(&self, headline: String, body: String) -> bool {
        let shown = self.inner.runtime.spawn(async move {
            let handle = Notification::new()
                .appname("RivetHub")
                .summary(&headline)
                .body(&body)
                .icon(APP)
                .hint(Hint::DesktopEntry(APP.to_owned()))
                .action("default", "Open")
                .show_async()
                .await;
            match handle {
                Ok(handle) => {
                    let mut clicked = false;
                    let waited = tokio::time::timeout(
                        CLICK_WINDOW,
                        handle.wait_for_action_async(|response| {
                            clicked = matches!(
                                response,
                                NotificationResponse::Default | NotificationResponse::Action(_)
                            );
                        }),
                    )
                    .await;
                    waited.is_ok() && clicked
                }
                Err(err) => {
                    tracing::warn!(%err, "showing a desktop notification failed");
                    false
                }
            }
        });
        shown.await.unwrap_or(false)
    }
}
