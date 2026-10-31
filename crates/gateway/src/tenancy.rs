use model::visibility::{Me, Space, Visibility};
use model::{SessionId, TopicId};
use wire::gateway::tenancy::{
    MeResponse, SessionVisibilityRecord, SpaceKind, TopicVisibilityRecord, UserSettings, UserSettingsUpdate,
    VisibilityUpdate,
};

use crate::topics::rh_message;
use crate::{Gateway, GatewayError, translate};

fn me(found: MeResponse) -> Me {
    Me {
        user: found.user,
        space: found
            .spaces
            .into_iter()
            .find(|space| space.kind == SpaceKind::Household)
            .map(|space| Space {
                id: space.id,
                name: space.name,
            }),
        default: Visibility::parse(&found.settings.default_visibility),
    }
}

fn update(visibility: Option<&Visibility>) -> VisibilityUpdate {
    VisibilityUpdate {
        visibility: visibility.map(|visibility| visibility.as_str().to_owned()),
    }
}

impl Gateway {
    pub async fn me(&self) -> Result<Me, GatewayError> {
        let url = self.api_url(&["api", "me"])?;
        Ok(me(Self::read_json(self.http.get(url).send().await)?.await?))
    }

    pub async fn set_default_visibility(&self, visibility: &Visibility) -> Result<Visibility, GatewayError> {
        let url = self.api_url(&["api", "me", "settings"])?;
        let body = UserSettingsUpdate {
            default_visibility: visibility.as_str().to_owned(),
        };
        let saved: UserSettings = Self::owned(self.http.put(url).json(&body).send().await).await?;
        Ok(Visibility::parse(&saved.default_visibility))
    }

    pub async fn mark_session(
        &self,
        session: &SessionId,
        visibility: Option<&Visibility>,
    ) -> Result<(), GatewayError> {
        let segment = wire::session_id::encode_segment(&translate::wire_session(session)?);
        let url = self.api_url(&["api", "sessions", &segment, "visibility"])?;
        let _: SessionVisibilityRecord =
            Self::owned(self.http.patch(url).json(&update(visibility)).send().await).await?;
        Ok(())
    }

    pub async fn mark_topic(&self, topic: &TopicId, visibility: &Visibility) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "topics", topic.as_str(), "visibility"])?;
        let _: TopicVisibilityRecord =
            Self::owned(self.http.patch(url).json(&update(Some(visibility))).send().await).await?;
        Ok(())
    }

    async fn owned<T: serde::de::DeserializeOwned>(
        sent: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<T, GatewayError> {
        match Self::read_json(sent)?.await {
            Err(GatewayError::Status { status: 403, detail }) => {
                Err(GatewayError::Conflict(rh_message(detail)))
            }
            read => read,
        }
    }
}
