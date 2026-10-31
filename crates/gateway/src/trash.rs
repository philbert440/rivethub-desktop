use model::SessionId;
use model::trash::{Deletion, Trash, TrashReason};
use wire::gateway::session_delete::{
    SessionDeleteError, SessionDeleteRequest, SessionDeleteResult, SessionDeleteResults,
    SessionTrashCandidate, SessionTrashList, SessionTrashReason,
};

use crate::{Gateway, GatewayError, translate};

/// The most sessions rh deletes for one request.
const BATCH: usize = 200;

fn trash(raw: SessionTrashCandidate) -> Trash {
    Trash {
        session: translate::session(&raw.session),
        title: raw.title,
        updated_at_ms: raw.updated_at.timestamp_millis(),
        reason: match raw.reason {
            SessionTrashReason::Empty => TrashReason::Empty,
            SessionTrashReason::OneWordReply => TrashReason::OneWordReply,
        },
        reply: raw.reply,
        node: None,
    }
}

fn deletion(result: SessionDeleteResult) -> (SessionId, Deletion) {
    match result {
        SessionDeleteResult::Deleted(deleted) => (translate::session(&deleted.session), Deletion::Deleted),
        SessionDeleteResult::Refused(refused) => (
            translate::session(&refused.session),
            match refused.error {
                SessionDeleteError::Live => Deletion::Live,
                SessionDeleteError::NotFound => Deletion::NotFound,
                SessionDeleteError::Failed => Deletion::Failed,
            },
        ),
    }
}

impl Gateway {
    /// The sessions this node counts as obvious trash; `None` from an rh older than the route.
    pub async fn trash(&self) -> Result<Option<Vec<Trash>>, GatewayError> {
        let url = self.api_url(&["api", "sessions", "trash"])?;
        match Self::read_json::<SessionTrashList>(self.http.get(url).send().await)?.await {
            Ok(list) => Ok(Some(list.candidates.into_iter().map(trash).collect())),
            Err(err) if err.is_missing_route() => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Keeps the session out of this node's trash review until it is used again.
    pub async fn keep_session(&self, session: &SessionId) -> Result<(), GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let url = self.api_url(&["api", "sessions", &segment, "keep"])?;
        Self::check(self.http.put(url).send().await.map_err(GatewayError::Http)?)
            .await
            .map(drop)
    }

    /// Deletes what memory holds of each session and hides it from the lists until it is used
    /// again, saying what became of each. One session's refusal never stops the others.
    pub async fn delete_sessions(
        &self,
        sessions: &[SessionId],
    ) -> Result<Vec<(SessionId, Deletion)>, GatewayError> {
        let url = self.api_url(&["api", "sessions", "delete"])?;
        let mut outcomes = Vec::with_capacity(sessions.len());
        for chunk in sessions.chunks(BATCH) {
            let body = SessionDeleteRequest {
                sessions: chunk
                    .iter()
                    .map(translate::wire_session)
                    .collect::<Result<_, _>>()?,
            };
            let done: SessionDeleteResults =
                Self::read_json(self.http.post(url.clone()).json(&body).send().await)?.await?;
            outcomes.extend(done.results.into_iter().map(deletion));
        }
        Ok(outcomes)
    }
}
