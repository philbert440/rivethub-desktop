pub mod panel;
mod translate;

use std::collections::BTreeMap;
use std::sync::Arc;

use model::plugin::canvas::{Event, Reply};
use model::plugin::view::{Answer, Value, View};
use model::plugin::{Context, Listing, LogLine, LogStream, Params, Permission, Settings};
use wire::plugin::action::{ViewActionRefusal, ViewActionRequest, ViewActionResult};
use wire::plugin::canvas::{CanvasEventRequest, CanvasResponse};
use wire::plugin::host::{
    PluginEnableRequest, PluginList, PluginLogStream, PluginLogs, PluginSettings, PluginSettingsUpdate,
};
use wire::plugin::view::ViewRequest;

use crate::events::{self, PluginEvent};
use crate::topics::rh_message;
use crate::{Gateway, GatewayError};

/// rh's answer for a plug-in that couldn't answer, in words, with what rh says went wrong: the
/// field it refused in an answer, or the status and body of a failed one. Any other failure as
/// it is.
fn failure(err: GatewayError) -> GatewayError {
    let GatewayError::Status { detail: body, .. } = &err else {
        return err;
    };
    let Ok(said) = serde_json::from_str::<wire::gateway::GatewayError>(body) else {
        return err;
    };
    let (why, detail) = match said.code.as_deref() {
        Some("plugin_unavailable") => ("isn't running", None),
        Some("plugin_timeout") => ("didn't answer in time", None),
        Some("plugin_invalid_response") => {
            ("answered with something the contract doesn't allow", said.detail)
        }
        Some("plugin_error") => ("failed", Some(said.error)),
        _ => return err,
    };
    GatewayError::Plugin { why, detail }
}

impl Gateway {
    /// The plug-ins installed on this gateway's node.
    pub async fn plugins(&self) -> Result<Listing, GatewayError> {
        let url = self.api_url(&["api", "plugins"])?;
        let list: PluginList = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(Listing {
            node: list.node,
            plugins: list.plugins.into_iter().map(translate::plugin).collect(),
        })
    }

    pub async fn plugin_view(
        &self,
        plugin: &str,
        view: &str,
        params: Option<&Params>,
        context: &Context,
    ) -> Result<View, GatewayError> {
        let url = self.api_url(&["api", "plugins", plugin, "views", view])?;
        let body = ViewRequest {
            params: translate::wire_params(params)?,
            context: translate::wire_context(context)?,
        };
        let view: wire::plugin::view::View = Self::read_json(self.http.post(url).json(&body).send().await)?
            .await
            .map_err(failure)?;
        Ok(translate::view(view))
    }

    /// Runs one of a plug-in's actions; a refusal comes back as the plug-in put it, beside the
    /// form fields it names.
    pub async fn plugin_action(
        &self,
        plugin: &str,
        action: &str,
        params: Option<&Params>,
        values: &BTreeMap<String, Value>,
        context: &Context,
    ) -> Result<Answer, GatewayError> {
        let url = self.api_url(&["api", "plugins", plugin, "actions", action])?;
        let body = ViewActionRequest {
            params: translate::wire_params(params)?,
            values: translate::wire_values(values),
            context: translate::wire_context(context)?,
        };
        let answered: Result<ViewActionResult, GatewayError> =
            Self::read_json(self.http.post(url).json(&body).send().await)?.await;
        match answered {
            Ok(result) => Ok(Answer::Done(
                result.actions.into_iter().map(translate::action).collect(),
            )),
            Err(GatewayError::Status { status: 422, detail }) => {
                match serde_json::from_str::<ViewActionRefusal>(&detail) {
                    Ok(refusal) => Ok(Answer::Refused {
                        message: refusal.message,
                        fields: refusal.fields,
                    }),
                    Err(_) => Err(GatewayError::Status { status: 422, detail }),
                }
            }
            Err(err) => Err(failure(err)),
        }
    }

    /// Tells a plug-in what the person did on one of its canvases.
    pub async fn canvas_event(
        &self,
        plugin: &str,
        canvas: &str,
        event: &Event,
        params: Option<&Params>,
        context: &Context,
    ) -> Result<Reply, GatewayError> {
        let url = self.api_url(&["api", "plugins", plugin, "canvas", canvas, "events"])?;
        let body = CanvasEventRequest {
            event: translate::wire_event(event),
            params: translate::wire_params(params)?,
            context: translate::wire_context(context)?,
        };
        let response: CanvasResponse = Self::read_json(self.http.post(url).json(&body).send().await)?
            .await
            .map_err(failure)?;
        Ok(translate::reply(response))
    }

    /// One of a plug-in's own files, from its `http/` route, with its media type when it names
    /// one. `path` is relative to the route and may carry a query; it can't climb out of it.
    pub async fn plugin_file(
        &self,
        plugin: &str,
        path: &str,
    ) -> Result<(Vec<u8>, Option<String>), GatewayError> {
        let root = self.api_url(&["api", "plugins", plugin, "http", ""])?;
        let url = root
            .join(path.trim_start_matches('/'))
            .ok()
            .filter(|url| url.path().starts_with(root.path()))
            .ok_or_else(|| GatewayError::Refused(format!("{path} isn't one of the plug-in's files")))?;
        let reply = Self::check(self.http.get(url).send().await.map_err(GatewayError::Http)?)
            .await
            .map_err(failure)?;
        let media_type = reply
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_owned());
        let bytes = reply.bytes().await.map_err(GatewayError::Http)?.to_vec();
        Ok((bytes, media_type))
    }

    /// The newest lines of a plug-in's log, oldest first: what it printed and what rh said about
    /// it. Only its node's owner may read it.
    pub async fn plugin_logs(&self, plugin: &str) -> Result<Vec<LogLine>, GatewayError> {
        let url = self.api_url(&["api", "plugins", plugin, "logs"])?;
        let logs: PluginLogs = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(logs
            .lines
            .into_iter()
            .map(|line| LogLine {
                at_ms: line.at.timestamp_millis(),
                stream: match line.stream {
                    PluginLogStream::Stdout => LogStream::Stdout,
                    PluginLogStream::Stderr => LogStream::Stderr,
                    PluginLogStream::Host => LogStream::Host,
                },
                text: line.text,
            })
            .collect())
    }

    pub async fn plugin_settings(&self, plugin: &str) -> Result<Settings, GatewayError> {
        let url = self.api_url(&["api", "plugins", plugin, "settings"])?;
        let settings: PluginSettings = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(Settings {
            values: settings
                .values
                .into_iter()
                .map(|(key, value)| (key, translate::value(value)))
                .collect(),
            secrets_set: settings.secrets_set,
        })
    }

    /// Keeps new values for a plug-in's settings; `None` clears one.
    pub async fn save_plugin_settings(
        &self,
        plugin: &str,
        values: &BTreeMap<String, Option<Value>>,
    ) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "plugins", plugin, "settings"])?;
        let body = PluginSettingsUpdate {
            values: values
                .iter()
                .map(|(key, value)| (key.clone(), value.as_ref().map(translate::wire_value)))
                .collect(),
        };
        Self::managed(self.http.put(url).json(&body).send().await).await
    }

    /// Grants a plug-in the permissions it waits on, `approve` naming exactly those, and starts it.
    pub async fn enable_plugin(&self, plugin: &str, approve: &[Permission]) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "plugins", plugin, "enable"])?;
        let body = PluginEnableRequest {
            approve: approve.iter().copied().map(translate::wire_permission).collect(),
        };
        Self::managed(self.http.post(url).json(&body).send().await).await
    }

    /// Stops a plug-in, keeping its grants, settings and data.
    pub async fn disable_plugin(&self, plugin: &str) -> Result<(), GatewayError> {
        let url = self.api_url(&["api", "plugins", plugin, "disable"])?;
        Self::managed(self.http.post(url).send().await).await
    }

    /// What the node says about its plug-ins as it happens.
    pub async fn watch_plugins(&self) -> Result<tokio::sync::mpsc::Receiver<PluginEvent>, GatewayError> {
        events::watch_plugins(&self.base_url, Arc::clone(&self.tls)).await
    }

    /// A management write's reply. rh's refusals (400, 403 and 409: a value that doesn't fit its
    /// field, an approval without a device certificate, permissions not approved) come back as
    /// `Conflict` carrying rh's own message.
    async fn managed(sent: Result<reqwest::Response, reqwest::Error>) -> Result<(), GatewayError> {
        let reply = sent.map_err(GatewayError::Http)?;
        match Self::check(reply).await {
            Err(GatewayError::Status { status: 403, detail }) if detail.contains("device_certificate_required") => {
                Err(GatewayError::Conflict(
                    "Only a device paired with this plug-in's node can approve what it asks for, and this one \
                     isn't paired with it."
                        .into(),
                ))
            }
            Err(GatewayError::Status {
                status: 400 | 403 | 409,
                detail,
            }) => Err(GatewayError::Conflict(rh_message(detail))),
            checked => checked.map(drop),
        }
    }
}
