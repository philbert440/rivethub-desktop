use gateway::GatewayError;
use model::visibility::{Me, Visibility};
use model::{SessionId, TopicId};

use crate::Hub;

impl Hub {
    pub fn me(&self) -> Option<Me> {
        self.inner.me.read().clone()
    }

    pub async fn load_me(&self) -> Result<Me, GatewayError> {
        let gateway = self.inner.local.clone();
        let me = self.on_runtime(async move { gateway.me().await }).await?;
        *self.inner.me.write() = Some(me.clone());
        Ok(me)
    }

    pub async fn set_default_visibility(&self, visibility: Visibility) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        let saved = self
            .on_runtime(async move { gateway.set_default_visibility(&visibility).await })
            .await?;
        if let Some(me) = self.inner.me.write().as_mut() {
            me.default = saved;
        }
        Ok(())
    }

    pub async fn mark_session(
        &self,
        session: SessionId,
        visibility: Option<Visibility>,
    ) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.mark_session(&session, visibility.as_ref()).await })
            .await
    }

    pub async fn mark_topic(&self, topic: TopicId, visibility: Visibility) -> Result<(), GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.mark_topic(&topic, &visibility).await })
            .await
    }
}
