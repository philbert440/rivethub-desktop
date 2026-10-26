mod session;
mod terminal;

pub use session::SessionHandle;
pub use terminal::{TerminalHandle, TerminalState};

use gateway::{Gateway, GatewayError};
use model::{HarnessId, SessionId, session::SessionSummary};
use tokio::runtime::Handle;

#[derive(Debug, Clone)]
pub struct Hub {
    runtime: Handle,
    gateway: Gateway,
}

impl Hub {
    pub fn new(runtime: Handle, gateway: Gateway) -> Self {
        Self { runtime, gateway }
    }

    pub fn open_session(&self, id: SessionId) -> SessionHandle {
        session::spawn(&self.runtime, self.gateway.clone(), id)
    }

    pub fn open_terminal(&self, id: SessionId, size: term::GridSize) -> TerminalHandle {
        terminal::spawn(&self.runtime, self.gateway.clone(), id, size)
    }

    pub async fn has_terminal(&self, id: SessionId) -> Result<bool, GatewayError> {
        let gateway = self.gateway.clone();
        self.runtime
            .spawn(async move { gateway.existing_terminal(&id).await.map(|pty| pty.is_some()) })
            .await
            .unwrap_or_else(|join| {
                Err(GatewayError::Refused(format!(
                    "checking the terminal stopped: {join}"
                )))
            })
    }

    pub async fn recent_sessions(&self, harness: HarnessId) -> Result<Vec<SessionSummary>, GatewayError> {
        let gateway = self.gateway.clone();
        self.runtime
            .spawn(async move { gateway.sessions(&harness).await })
            .await
            .unwrap_or_else(|join| Err(GatewayError::Refused(format!("listing sessions stopped: {join}"))))
    }

    pub async fn start_session(&self, harness: HarnessId) -> Result<SessionSummary, GatewayError> {
        let gateway = self.gateway.clone();
        self.runtime
            .spawn(async move { gateway.start_session(&harness, None, None).await })
            .await
            .unwrap_or_else(|join| {
                Err(GatewayError::Refused(format!(
                    "starting the session stopped: {join}"
                )))
            })
    }
}
