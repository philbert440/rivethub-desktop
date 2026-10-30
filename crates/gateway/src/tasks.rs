use model::TaskId;
use model::session::{SessionSummary, TaskStop};
use wire::task::{TaskKillResponse, TaskStatus};

use crate::{Gateway, GatewayError, translate};

impl Gateway {
    /// The chats this node's rh runs now, with the task and parent of a delegated one, which a
    /// harness's own session list doesn't name.
    pub async fn live_sessions(&self) -> Result<Vec<SessionSummary>, GatewayError> {
        let url = self.api_url(&["api", "harness-sessions"])?;
        let list: wire::harness::HarnessSessionList =
            Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(list.sessions.into_iter().map(translate::summary).collect())
    }

    /// Stops a delegated task, from any node: its turn stops and the task ends as killed.
    pub async fn stop_task(&self, task: &TaskId) -> Result<TaskStop, GatewayError> {
        let url = self.api_url(&["api", "tasks", task.as_str(), "kill"])?;
        let killed: TaskKillResponse = Self::read_json(self.http.post(url).send().await)?.await?;
        Ok(match killed.prior {
            Some(TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Killed | TaskStatus::Timeout) => {
                TaskStop::AlreadyOver
            }
            Some(TaskStatus::Queued | TaskStatus::Running | TaskStatus::AwaitingInput) | None => {
                TaskStop::Stopped
            }
        })
    }
}
