use std::fmt::Write as _;

use gpui::{AnyElement, Context, Div, IntoElement, ParentElement, Styled, Window, div, prelude::*};
use model::health::Status;
use model::trust::{Certificate, Validity};

use super::SettingsPage;
use super::parts::{Load, button, dim, field, labelled, mono, path, row, section};
use crate::copy::{self, CopyIcon};
use crate::text::now_ms;

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The commit this build was made from, when it was built from a git checkout.
const REVISION: Option<&str> = option_env!("RIVETHUB_DESKTOP_REVISION");

impl SettingsPage {
    pub(super) fn about(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Div> {
        let mut app = vec![field("Version", mono(release()))];
        app.extend(
            [
                ("Preferences", self.hub.preferences_path()),
                ("Log", crate::log_path()),
            ]
            .into_iter()
            .filter_map(|(label, path)| Some(path_row(label, &path?.display().to_string()))),
        );
        vec![
            section("RivetHub desktop", app),
            section("Diagnostics", vec![self.diagnostics_row(window, cx)]),
        ]
    }

    fn diagnostics_row(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let copied = copy::state("copy-diagnostics", window, cx);
        let done = copied.read(cx).just();
        let report = self.diagnostics();
        row()
            .child(labelled(
                "For a bug report",
                "This build, the gateway, every node, the certificates and memory's health, as text.",
            ))
            .child(
                button("copy-diagnostics", if done { "Copied" } else { "Copy" })
                    .on_click(move |_, _, cx| copy::copy(&copied, &report, cx)),
            )
            .into_any_element()
    }

    fn diagnostics(&self) -> String {
        let mut report = format!("RivetHub desktop {}\n", release());
        let set_by = if self.gateway.from_env {
            "RIVETHUB_URL"
        } else {
            "default"
        };
        let _ = writeln!(report, "Gateway: {} ({set_by})", self.gateway.url);
        if let Some(node) = self.hub.local_node() {
            let _ = writeln!(report, "This node: {node}");
        }
        match &self.nodes {
            Load::Ready(nodes) => {
                for node in nodes {
                    let state = if node.online { "online" } else { "offline" };
                    let release = node.release().unwrap_or_else(|| "version not reported".into());
                    let _ = writeln!(report, "Node {}: {state}, {release}, {}", node.name, node.url);
                }
            }
            Load::Failed(problem) => {
                let _ = writeln!(report, "Nodes: {problem}");
            }
            Load::Waiting => {}
        }
        match &self.trust {
            Load::Ready(trust) => {
                let device = trust.device.as_ref().map_or_else(|| "none".to_owned(), described);
                let _ = writeln!(report, "Device certificate: {device}");
                let _ = writeln!(report, "Certificate authority: {}", described(&trust.authority));
            }
            Load::Failed(problem) => {
                let _ = writeln!(report, "Certificates: {problem}");
            }
            Load::Waiting => {}
        }
        match &self.memory {
            Load::Ready(health) => {
                let status = match health.status {
                    Status::Ok => "ok",
                    Status::Degraded => "degraded",
                    Status::Error => "error",
                };
                let search = if health.semantic {
                    "semantic"
                } else {
                    "keywords only"
                };
                let _ = writeln!(
                    report,
                    "Memory: {status}, {search} search, {} sessions, {} messages",
                    health.conversations, health.messages
                );
            }
            Load::Failed(problem) => {
                let _ = writeln!(report, "Memory: {problem}");
            }
            Load::Waiting => {}
        }
        if let Some(log) = crate::log_path() {
            let _ = writeln!(report, "Log: {}", log.display());
        }
        report
    }
}

/// "0.7.0 · e2399f6", or the version alone for a build made outside git.
fn release() -> String {
    match REVISION {
        Some(revision) => format!("{VERSION} · {revision}"),
        None => VERSION.to_owned(),
    }
}

fn described(cert: &Certificate) -> String {
    let when = match cert.validity(now_ms()) {
        Validity::Valid { days_left } => format!("expires in {days_left} days"),
        Validity::Expired { days_ago } => format!("expired {days_ago} days ago"),
    };
    format!("{} ({when})", cert.subject)
}

fn path_row(label: &'static str, location: &str) -> AnyElement {
    row()
        .child(dim(label).flex_none())
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(path(location.to_owned()))
                .child(CopyIcon::new(format!("copy-{label}"), location)),
        )
        .into_any_element()
}
