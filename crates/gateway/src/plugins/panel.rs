//! A plug-in's web panel (CONTRACT note 60): the messages between the app and the page, the theme
//! as CSS variables, and the page's requests passed through to the plug-in's routes.

use std::fmt::Write as _;

use model::plugin::canvas::Role;
use model::plugin::panel::{HostMessage, Message, Request, Response, Theme};
use wire::plugin::PluginId;
use wire::plugin::canvas::ColorRole;
use wire::plugin::panel::{PanelHostMessage, PanelMessage, PanelScheme, PanelTheme};

use super::translate;
use crate::{Gateway, GatewayError};

/// The request headers a page's request keeps on its way to the plug-in.
const FORWARDED: [reqwest::header::HeaderName; 2] = [reqwest::header::CONTENT_TYPE, reqwest::header::ACCEPT];

fn wire_role(role: Role) -> ColorRole {
    match role {
        Role::Surface => ColorRole::Surface,
        Role::SurfaceContainer => ColorRole::SurfaceContainer,
        Role::SurfaceContainerHigh => ColorRole::SurfaceContainerHigh,
        Role::Primary => ColorRole::Primary,
        Role::PrimaryContainer => ColorRole::PrimaryContainer,
        Role::OnPrimary => ColorRole::OnPrimary,
        Role::OnPrimaryContainer => ColorRole::OnPrimaryContainer,
        Role::SecondaryContainer => ColorRole::SecondaryContainer,
        Role::OnSurface => ColorRole::OnSurface,
        Role::OnSurfaceVariant => ColorRole::OnSurfaceVariant,
        Role::Outline => ColorRole::Outline,
        Role::OutlineVariant => ColorRole::OutlineVariant,
        Role::Info => ColorRole::Info,
        Role::Success => ColorRole::Success,
        Role::Warning => ColorRole::Warning,
        Role::Danger => ColorRole::Danger,
        Role::Category1 => ColorRole::Category1,
        Role::Category2 => ColorRole::Category2,
        Role::Category3 => ColorRole::Category3,
        Role::Category4 => ColorRole::Category4,
        Role::Category5 => ColorRole::Category5,
        Role::Category6 => ColorRole::Category6,
    }
}

fn hex(rgb: u32) -> String {
    format!("#{:06x}", rgb & 0x00ff_ffff)
}

fn wire_theme(theme: &Theme) -> PanelTheme {
    PanelTheme {
        scheme: if theme.light {
            PanelScheme::Light
        } else {
            PanelScheme::Dark
        },
        colors: theme
            .colors
            .iter()
            .map(|(role, rgb)| (wire_role(*role), hex(*rgb)))
            .collect(),
        font_family: theme.font_family.clone(),
        font_mono: theme.font_mono.clone(),
        font_size: f64::from(theme.font_size),
        radius: f64::from(theme.radius),
    }
}

/// A message for the page, as the JSON it receives.
pub fn host_message(message: &HostMessage) -> Result<String, GatewayError> {
    let wire = match message {
        HostMessage::Init {
            theme,
            params,
            context,
            plugin,
        } => PanelHostMessage::Init {
            theme: wire_theme(theme),
            params: translate::wire_params(params.as_ref())?,
            context: translate::wire_context(context)?,
            plugin: PluginId::try_from(plugin.id.clone())
                .map_err(|err| GatewayError::Refused(format!("plug-in id {}: {err}", plugin.id)))?,
            node: plugin.node.clone(),
        },
        HostMessage::Theme(theme) => PanelHostMessage::Theme {
            theme: wire_theme(theme),
        },
    };
    serde_json::to_string(&wire).map_err(|err| GatewayError::Refused(err.to_string()))
}

/// What a page posted, or why it isn't a message the contract allows.
pub fn page_message(json: &str) -> Result<Message, String> {
    let message: PanelMessage = serde_json::from_str(json).map_err(|err| err.to_string())?;
    Ok(match message {
        PanelMessage::Ready => Message::Ready,
        PanelMessage::Resize { height } => Message::Resize(height),
        PanelMessage::Action { action } => Message::Action(translate::action(action)),
    })
}

/// The theme as the CSS variables a page reads: `--rh-color-<role>` with the role in kebab case,
/// the fonts, the text size, the radius and `color-scheme`.
pub fn style(theme: &Theme) -> String {
    let colors = theme
        .colors
        .iter()
        .fold(String::new(), |mut colors, (role, rgb)| {
            let name = serde_json::to_value(wire_role(*role))
                .ok()
                .and_then(|name| name.as_str().map(kebab))
                .unwrap_or_default();
            write!(colors, "--rh-color-{name}:{};", hex(*rgb)).ok();
            colors
        });
    format!(
        ":root{{{colors}--rh-font-family:{};--rh-font-mono:{};--rh-font-size:{}px;--rh-radius:{}px;color-scheme:{};}}",
        font_list(&theme.font_family, "sans-serif"),
        font_list(&theme.font_mono, "monospace"),
        theme.font_size,
        theme.radius,
        if theme.light { "light" } else { "dark" },
    )
}

fn kebab(camel: &str) -> String {
    let mut kebab = String::with_capacity(camel.len() + 4);
    for character in camel.chars() {
        if character.is_ascii_uppercase() {
            kebab.push('-');
        }
        kebab.push(character.to_ascii_lowercase());
    }
    kebab
}

/// A font family quoted for CSS, with a generic family behind it.
fn font_list(family: &str, generic: &str) -> String {
    let quoted = family.replace(['"', '\\', ';', '}'], "");
    format!("\"{quoted}\",{generic}")
}

impl Gateway {
    /// A web panel's request, passed through to its plug-in: `/http/…` to the plug-in's `http/`
    /// route, any other path to its `panel/` route. The plug-in's status comes back as it is.
    pub async fn plugin_page(&self, plugin: &str, request: Request) -> Result<Response, GatewayError> {
        let path = request.path.trim_start_matches('/');
        let (route, rest) = match path.strip_prefix("http/") {
            Some(rest) => ("http", rest),
            None => ("panel", path),
        };
        let root = self.api_url(&["api", "plugins", plugin, route, ""])?;
        let url = root
            .join(rest)
            .ok()
            .filter(|url| url.path().starts_with(root.path()))
            .ok_or_else(|| {
                GatewayError::Refused(format!("{} is outside the plug-in's panel", request.path))
            })?;
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|err| GatewayError::Refused(format!("method {}: {err}", request.method)))?;
        let mut sent = self.http.request(method, url);
        for (name, value) in &request.headers {
            if FORWARDED
                .iter()
                .any(|kept| kept.as_str().eq_ignore_ascii_case(name))
            {
                sent = sent.header(name, value);
            }
        }
        if !request.body.is_empty() {
            sent = sent.body(request.body);
        }
        let reply = sent.send().await.map_err(GatewayError::Http)?;
        let status = reply.status().as_u16();
        let media_type = reply
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = reply.bytes().await.map_err(GatewayError::Http)?.to_vec();
        Ok(Response {
            status,
            media_type,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use model::plugin::canvas::Role;
    use model::plugin::panel::{Message, Theme};
    use model::plugin::view::Action;

    use super::{page_message, style};

    #[test]
    fn the_theme_reaches_a_page_as_the_variables_the_contract_names() {
        let theme = Theme {
            light: false,
            colors: vec![
                (Role::SurfaceContainerHigh, 0x2b_332e),
                (Role::Category1, 0x8e_cbff),
            ],
            font_family: "Roboto Flex".into(),
            font_mono: "JetBrains Mono".into(),
            font_size: 14.,
            radius: 12.,
        };
        assert_eq!(
            style(&theme),
            ":root{--rh-color-surface-container-high:#2b332e;--rh-color-category1:#8ecbff;\
             --rh-font-family:\"Roboto Flex\",sans-serif;--rh-font-mono:\"JetBrains Mono\",monospace;\
             --rh-font-size:14px;--rh-radius:12px;color-scheme:dark;}"
        );
    }

    #[test]
    fn a_page_asks_for_actions_and_nothing_outside_the_contract() {
        assert_eq!(
            page_message(r#"{"type":"action","action":{"type":"toast","text":"Saved"}}"#),
            Ok(Message::Action(Action::Toast {
                text: "Saved".into(),
                tone: model::plugin::view::Tone::Neutral,
            }))
        );
        assert_eq!(
            page_message(r#"{"type":"resize","height":240}"#),
            Ok(Message::Resize(240))
        );
        assert!(page_message(r#"{"type":"eval","code":"1"}"#).is_err());
    }
}
