use model::SessionId;
use model::preview::{Preview, PreviewKind, Previews};
use wire::gateway::sessions::{SessionPreviewKind, SessionPreviewList};

use crate::{Gateway, GatewayError, translate};

/// rh previews at most this many sessions per call.
const PER_CALL: usize = 50;

impl Gateway {
    /// What each of these sessions is doing now, or last said; sessions with nothing to show, or
    /// that this node can't see, are left out.
    pub async fn previews(&self, sessions: &[SessionId]) -> Result<Previews, GatewayError> {
        let mut previews = Previews::new();
        for chunk in sessions.chunks(PER_CALL) {
            let mut url = self.api_url(&["api", "harness-sessions", "previews"])?;
            for session in chunk {
                url.query_pairs_mut()
                    .append_pair("session", translate::wire_session(session)?.as_str());
            }
            let list: SessionPreviewList = Self::read_json(self.http.get(url).send().await)?.await?;
            previews.extend(list.previews.into_iter().map(|preview| {
                (
                    translate::session(&preview.session_id),
                    Preview {
                        kind: match preview.kind {
                            SessionPreviewKind::Running => PreviewKind::Running,
                            SessionPreviewKind::Asking => PreviewKind::Asking,
                            SessionPreviewKind::Approval => PreviewKind::Approval,
                            SessionPreviewKind::Said => PreviewKind::Said,
                        },
                        text: preview.text,
                        at_ms: preview.at.timestamp_millis(),
                    },
                )
            }));
        }
        Ok(previews)
    }
}
