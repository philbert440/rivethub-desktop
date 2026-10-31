use model::flagged::{FlaggedAction, FlaggedGroup, FlaggedJob};
use wire::gateway::flagged::{FlaggedGroup as Group, FlaggedJob as Job, FlaggedJobs};

use crate::{Gateway, GatewayError};

fn job(raw: Job) -> FlaggedJob {
    FlaggedJob {
        id: raw.id,
        key: raw.key,
        subject: raw.subject,
        payload: if raw.payload.as_map().is_empty() {
            String::new()
        } else {
            serde_json::to_string(&raw.payload).unwrap_or_default()
        },
        error: raw.error,
        failures: raw.failures,
        last_failed_ms: raw.last_failed_at.timestamp_millis(),
    }
}

fn group(raw: Group) -> FlaggedGroup {
    FlaggedGroup {
        task: raw.task,
        cause: raw.cause,
        count: raw.count,
        sample: job(raw.sample),
    }
}

impl Gateway {
    /// The jobs memory stopped retrying, one group per task and cause, newest failure first;
    /// `None` when the node keeps none for this client: an rh older than the route, a node
    /// without the store, or one this client doesn't own.
    pub async fn flagged(&self) -> Result<Option<Vec<FlaggedGroup>>, GatewayError> {
        let url = self.api_url(&["api", "memory", "flagged"])?;
        match Self::read_json::<FlaggedJobs>(self.http.get(url).send().await)?.await {
            Ok(jobs) => Ok(Some(jobs.groups.into_iter().map(group).collect())),
            Err(GatewayError::Status {
                status: 403 | 404 | 503,
                ..
            }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Retries or dismisses one flagged job.
    pub async fn act_on_flagged(&self, id: i64, action: FlaggedAction) -> Result<(), GatewayError> {
        let verb = match action {
            FlaggedAction::Retry => "retry",
            FlaggedAction::Dismiss => "dismiss",
        };
        let url = self.api_url(&["api", "memory", "flagged", &id.to_string(), verb])?;
        Self::check(self.http.post(url).send().await.map_err(GatewayError::Http)?)
            .await
            .map(drop)
    }
}
