use gpui::{AnyElement, Div, IntoElement, ParentElement, Styled, div, prelude::*};
use model::trust::{Certificate, Validity};

use super::SettingsPage;
use super::parts::{Load, chip, dim, dot, field, mono, pending, row, section};
use crate::text::now_ms;
use crate::theme::Theme;

impl SettingsPage {
    pub(super) fn nodes_tab(&self) -> Vec<Div> {
        vec![
            section("This app connects to", self.connection()),
            section("Nodes", self.nodes()),
        ]
    }

    pub(super) fn devices(&self) -> Vec<Div> {
        vec![
            section("This device", self.device()),
            section("Pair a phone", vec![self.pairing.clone().into_any_element()]),
            section("Certificate authority", self.authority()),
        ]
    }

    fn nodes(&self) -> Vec<AnyElement> {
        let nodes = match &self.nodes {
            Load::Ready(nodes) if nodes.is_empty() => {
                return vec![row().child(dim("The mesh lists no nodes.")).into_any_element()];
            }
            Load::Ready(nodes) => nodes,
            other => return pending(other),
        };
        let local = self.hub.local_node();
        nodes
            .iter()
            .map(|node| {
                let here = local.as_deref().is_some_and(|name| node.answers_to(name));
                let status = match (node.online, node.sessions) {
                    (false, _) => "offline".to_owned(),
                    (true, Some(1)) => "online · 1 session".to_owned(),
                    (true, Some(count)) => format!("online · {count} sessions"),
                    (true, None) => "online".to_owned(),
                };
                row()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .min_w_0()
                            .child(dot(if node.online {
                                Theme::accent()
                            } else {
                                Theme::line()
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(node.name.clone())
                                            .when(here, |name| name.child(chip("this node"))),
                                    )
                                    .child(mono(node.url.clone())),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_end()
                            .flex_none()
                            .child(dim(status))
                            .children(match (node.release(), node.online) {
                                (Some(release), _) => Some(mono(release)),
                                (None, true) => Some(mono("version not reported")),
                                (None, false) => None,
                            }),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn connection(&self) -> Vec<AnyElement> {
        vec![
            field("Gateway", mono(self.gateway.url.clone())),
            field(
                "Set by",
                if self.gateway.from_env {
                    mono("RIVETHUB_URL")
                } else {
                    dim("The default")
                },
            ),
        ]
    }

    fn device(&self) -> Vec<AnyElement> {
        let trust = match &self.trust {
            Load::Ready(trust) => trust,
            other => return pending(other),
        };
        let Some(cert) = &trust.device else {
            return vec![
                row()
                    .child("No device certificate")
                    .child(dim("This app connects without one."))
                    .into_any_element(),
            ];
        };
        let holder = cert.holder().into_iter().flat_map(|holder| {
            [
                field("Device", div().child(holder.device.to_owned())),
                field("User", div().child(holder.user.to_owned())),
            ]
        });
        holder
            .chain([
                field("Identity", mono(cert.subject.clone())),
                field("Issued by", div().child(cert.issuer.clone())),
                field("Expires", expiry(cert)),
            ])
            .collect()
    }

    fn authority(&self) -> Vec<AnyElement> {
        match &self.trust {
            Load::Ready(trust) => vec![
                field("Authority", div().child(trust.authority.subject.clone())),
                field("Expires", expiry(&trust.authority)),
            ],
            other => pending(other),
        }
    }
}

pub(super) fn expiry(cert: &Certificate) -> Div {
    div().child(match cert.validity(now_ms()) {
        Validity::Valid { days_left: 0 } => "today".to_owned(),
        Validity::Valid { days_left: 1 } => "in 1 day".to_owned(),
        Validity::Valid { days_left } if days_left < 730 => format!("in {days_left} days"),
        Validity::Valid { days_left } => format!("in {} years", days_left / 365),
        Validity::Expired { days_ago: 0 } => {
            return div().text_color(Theme::removed()).child("expired today");
        }
        Validity::Expired { days_ago } => {
            return div()
                .text_color(Theme::removed())
                .child(format!("expired {days_ago} days ago"));
        }
    })
}
