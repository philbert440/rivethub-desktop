use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, IntoElement, ParentElement, Styled, Subscription,
    Window, div, prelude::*, px,
};
use model::TopicId;
use model::topics::{Topic, TopicDirectory};

use super::Workspace;
use crate::input::{Submitted, TextInput};
use crate::theme::Theme;

/// The topic whose working directory is being typed.
pub(super) struct Directing {
    topic: TopicId,
    input: Entity<TextInput>,
    problem: Option<String>,
    _submitted: Subscription,
}

impl Workspace {
    fn begin_directory(&mut self, topic: TopicId, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .topic(&topic)
            .and_then(|known| known.directory.as_ref())
            .map(|directory| directory.path().to_owned())
            .unwrap_or_default();
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Absolute path, such as /home/you/work/project", cx);
            input.set_text(&current, cx);
            input
        });
        let submitted = cx.subscribe(&input, |this, _, Submitted(path), cx| {
            this.commit_directory(path, cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.directing = Some(Directing {
            topic,
            input,
            problem: None,
            _submitted: submitted,
        });
        cx.notify();
    }

    fn commit_directory(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(directing) = self.directing.as_mut() else {
            return;
        };
        if !path.starts_with('/') {
            directing.problem = Some("Use an absolute path, starting with /.".into());
            cx.notify();
            return;
        }
        let Some(node) = self.hub.local_node() else {
            directing.problem = Some("This node hasn't reported its name yet; try again in a moment.".into());
            cx.notify();
            return;
        };
        let topic = directing.topic.clone();
        self.save_directory(topic, Some(TopicDirectory::typed(path, &node)), cx);
    }

    fn save_directory(&mut self, topic: TopicId, directory: Option<TopicDirectory>, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let saved = hub.set_topic_directory(topic, directory).await;
            this.update(cx, |this, cx| {
                match saved {
                    Ok(_) => {
                        this.directing = None;
                        this.load(cx);
                    }
                    Err(err) => {
                        let problem = format!("saving the directory failed: {err}");
                        match this.directing.as_mut() {
                            Some(directing) => directing.problem = Some(problem),
                            None => this.problem = Some(problem),
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// "Works in /path on node", with Change and Clear; or the editor while it is being typed.
    pub(super) fn directory_line(&self, topic: &Topic, cx: &mut Context<Self>) -> AnyElement {
        if let Some(directing) = self
            .directing
            .as_ref()
            .filter(|directing| directing.topic == topic.id)
        {
            return div()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .child(div().text_color(Theme::ink_dim()).child("Works in"))
                .child(
                    div()
                        .w(px(420.))
                        .px_3()
                        .py_1()
                        .rounded(px(10.))
                        .bg(Theme::card())
                        .border_1()
                        .border_color(Theme::accent())
                        .child(directing.input.clone()),
                )
                .child(Self::link("cancel-directory", "Cancel").on_click(cx.listener(
                    |this, _: &ClickEvent, window, cx| {
                        this.directing = None;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    },
                )))
                .children(
                    directing
                        .problem
                        .clone()
                        .map(|problem| div().text_color(Theme::warn()).child(problem)),
                )
                .into_any_element();
        }
        let change = topic.id.clone();
        let line = div()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(Theme::ink_dim());
        let Some(directory) = &topic.directory else {
            return line
                .child(
                    Self::link("set-directory", "Set a working directory").on_click(cx.listener(
                        move |this, _: &ClickEvent, window, cx| {
                            this.begin_directory(change.clone(), window, cx);
                        },
                    )),
                )
                .into_any_element();
        };
        let clear = topic.id.clone();
        let place = match directory {
            TopicDirectory::Local { node, .. } => format!("on {node}"),
            TopicDirectory::Shared { .. } => "shared".to_owned(),
        };
        line.child("Works in")
            .child(div().text_color(Theme::ink()).child(directory.path().to_owned()))
            .child(place)
            .child(Self::link("change-directory", "Change").on_click(cx.listener(
                move |this, _: &ClickEvent, window, cx| this.begin_directory(change.clone(), window, cx),
            )))
            .child(Self::link("clear-directory", "Clear").on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| this.save_directory(clear.clone(), None, cx)),
            ))
            .into_any_element()
    }

    fn link(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .cursor_pointer()
            .text_color(Theme::ink_dim())
            .hover(|link| link.text_color(Theme::accent()))
            .child(label)
    }
}
