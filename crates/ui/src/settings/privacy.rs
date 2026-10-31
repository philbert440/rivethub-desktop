use gpui::{
    AnyElement, ClickEvent, Context, Div, IntoElement, ParentElement, SharedString, Styled, div, prelude::*,
};
use model::visibility::{Me, Visibility};

use super::SettingsPage;
use super::parts::{Load, labelled, pending, row, section, segment, segments};
use crate::theme::Theme;

const DEFAULT_SINCE: &str = "Changing this applies to sessions you start afterwards; earlier sessions stay as \
                             they are until you share them.";

impl SettingsPage {
    pub(super) fn privacy(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let rows = match &self.me {
            Load::Ready(me) => self.default_visibility(me, cx),
            other => pending(other),
        };
        vec![section("Sharing", rows)]
    }

    fn default_visibility(&self, me: &Me, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut choices: Vec<(&'static str, SharedString, Visibility)> =
            vec![("default-private", "Private".into(), Visibility::Private)];
        if let Some(space) = &me.space {
            choices.push((
                "default-shared",
                format!("Shared with {}", space.name.to_lowercase()).into(),
                Visibility::Space(space.id.clone()),
            ));
        }
        let options = choices.into_iter().map(|(id, label, visibility)| {
            let here = visibility.is_private() == me.default.is_private();
            segment(id, label, here).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if !here {
                    this.save_default(visibility.clone(), cx);
                }
            }))
        });
        let mut rows = vec![
            row()
                .child(labelled("Default for new sessions and topics", DEFAULT_SINCE))
                .child(segments().children(options))
                .into_any_element(),
        ];
        rows.extend(self.privacy_problem.clone().map(|problem| {
            row()
                .child(div().text_color(Theme::warn()).child(problem))
                .into_any_element()
        }));
        rows
    }

    pub(super) fn load_me(&mut self, cx: &mut Context<Self>) {
        self.me = Load::Waiting;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let me = hub.load_me().await;
            this.update(cx, |this, cx| {
                this.me = match me {
                    Err(err) if err.is_missing_route() => {
                        Load::Failed("Privacy needs a newer rh on this node.".into())
                    }
                    other => Load::from(other),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn save_default(&mut self, visibility: Visibility, cx: &mut Context<Self>) {
        self.privacy_problem = None;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let saved = hub.set_default_visibility(visibility).await;
            this.update(cx, |this, cx| {
                match saved {
                    Ok(()) => {
                        if let Some(me) = this.hub.me() {
                            this.me = Load::Ready(me);
                        }
                    }
                    Err(err) => this.privacy_problem = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
