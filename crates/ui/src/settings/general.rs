use gpui::{ClickEvent, Context, Div, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::prefs::{LastLaunch, SessionView};

use super::SettingsPage;
use super::parts::{button, dim, labelled, row, section, segment, segments, toggle, toggle_if};
use crate::launch_choice::{LaunchChoices, Picked};
use crate::theme::Theme;

impl SettingsPage {
    pub(super) fn general(&self, cx: &mut Context<Self>) -> Vec<Div> {
        vec![
            section("New sessions", self.new_sessions(cx)),
            section(
                "Sessions",
                vec![
                    self.opens_in(cx),
                    toggle(
                        "jump-to-newest",
                        "Jump to newest",
                        "A send or a finished reply brings an open chat to its end, even when scrolled up.",
                        self.hub.preferences().jump_to_newest,
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            let mut prefs = this.hub.preferences();
                            prefs.jump_to_newest = !prefs.jump_to_newest;
                            this.hub.set_preferences(prefs);
                            cx.notify();
                        }),
                    ),
                ],
            ),
            section("Notifications", self.notifications(cx)),
        ]
    }

    /// What the launcher preselects: the last thing started, which can also be picked here.
    fn new_sessions(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let prefs = self.hub.preferences();
        let (choice, detail) = prefs.last_launch.as_ref().map_or(
            (
                "Claude on this node".to_owned(),
                "The built-in choice, until you start a session or pick one.",
            ),
            |last| {
                (
                    launch_label(&self.hub, last),
                    "The last one you started or picked.",
                )
            },
        );
        let choosing = self.choosing.is_some();
        let controls = div()
            .flex()
            .items_center()
            .gap_2()
            .when(prefs.last_launch.is_some() && !choosing, |line| {
                line.child(button("reset-new-session", "Reset").on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        let mut prefs = this.hub.preferences();
                        prefs.last_launch = None;
                        this.hub.set_preferences(prefs);
                        cx.notify();
                    },
                )))
            })
            .child(
                button("change-new-session", if choosing { "Done" } else { "Change" }).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.choosing = match this.choosing.take() {
                            Some(_) => None,
                            None => Some(this.choices(cx)),
                        };
                        cx.notify();
                    },
                )),
            );
        let mut rows = vec![
            row()
                .child(labelled("Start with", detail))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .min_w_0()
                        .child(dim(choice).truncate())
                        .child(controls),
                )
                .into_any_element(),
        ];
        if let Some((choices, _)) = &self.choosing {
            rows.push(
                div()
                    .mx_3()
                    .mb_3()
                    .p_4()
                    .rounded(px(16.))
                    .bg(Theme::surface())
                    .child(choices.clone())
                    .into_any_element(),
            );
        }
        rows
    }

    /// A pick here is kept at once, as starting a session with it would keep it.
    fn choices(&self, cx: &mut Context<Self>) -> (gpui::Entity<LaunchChoices>, gpui::Subscription) {
        let choices = cx.new(|cx| LaunchChoices::new(&self.hub, cx));
        let picked = cx.subscribe(&choices, |this, choices, _: &Picked, cx| {
            if let Some(choice) = choices.read(cx).choice() {
                choice.remember(&this.hub);
            }
            cx.notify();
        });
        (choices, picked)
    }

    /// Which view a session opens on. The terminal is the session's own: rh attaches to the
    /// one running, or, at a click, resumes the session there instead of in chat.
    fn opens_in(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let chosen = self.hub.preferences().opens_in;
        let options = [
            (SessionView::Chat, "opens-in-chat", "Chat"),
            (SessionView::Terminal, "opens-in-terminal", "Terminal"),
        ]
        .map(|(view, id, label)| {
            segment(id, label, view == chosen).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                let mut prefs = this.hub.preferences();
                prefs.opens_in = view;
                this.hub.set_preferences(prefs);
                cx.notify();
            }))
        });
        row()
            .child(labelled(
                "Open sessions in",
                "The terminal attaches to the session's own, or offers to start it there.",
            ))
            .child(segments().children(options))
            .into_any_element()
    }

    /// The master switch first; the kinds and the sound below it only count while it's on.
    fn notifications(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let prefs = self.hub.preferences();
        let on = prefs.notifications;
        vec![
            toggle(
                "notifications",
                "Desktop notifications",
                "While RivetHub is in the background or on another session. Off silences all of them.",
                on,
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    let mut prefs = this.hub.preferences();
                    prefs.notifications = !prefs.notifications;
                    this.hub.set_preferences(prefs);
                    cx.notify();
                }),
            ),
            toggle_if(
                on,
                "notify-finished",
                "An agent finished a reply",
                "When you're in another window or another session.",
                prefs.notify_finished,
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    let mut prefs = this.hub.preferences();
                    prefs.notify_finished = !prefs.notify_finished;
                    this.hub.set_preferences(prefs);
                    cx.notify();
                }),
            ),
            toggle_if(
                on,
                "notify-needs-you",
                "A session needs you",
                "When an agent waits on an approval or an answer.",
                prefs.notify_needs_you,
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    let mut prefs = this.hub.preferences();
                    prefs.notify_needs_you = !prefs.notify_needs_you;
                    this.hub.set_preferences(prefs);
                    cx.notify();
                }),
            ),
            toggle_if(
                on,
                "notify-sound",
                "Play a sound",
                "A short chime with each notification. Turning it on plays it once.",
                prefs.notify_sound,
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    let mut prefs = this.hub.preferences();
                    prefs.notify_sound = !prefs.notify_sound;
                    if prefs.notify_sound {
                        this.hub.chime();
                    }
                    this.hub.set_preferences(prefs);
                    cx.notify();
                }),
            ),
        ]
    }
}

/// "Grok · grok-4 on ct115" for a harness, "Scout (agent)" for an agent, with the model's name
/// when its node has said what it is.
fn launch_label(hub: &app::Hub, last: &LastLaunch) -> String {
    match last {
        LastLaunch::Agent { name, .. } => format!("{name} (agent)"),
        LastLaunch::Harness { node, harness, model } => {
            let place = node.as_deref().unwrap_or("this node");
            match model {
                Some(model) => {
                    let model = hub
                        .model_label(node.as_deref(), harness, model)
                        .unwrap_or_else(|| model.clone());
                    format!("{} · {model} on {place}", harness.display_name())
                }
                None => format!("{} on {place}", harness.display_name()),
            }
        }
    }
}
