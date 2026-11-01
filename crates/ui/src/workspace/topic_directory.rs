use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, IntoElement, ParentElement, Styled, Subscription,
    Window, div, prelude::*, px,
};
use model::TopicId;
use model::topics::{Atlas, Directory, Topic};

use super::Workspace;
use super::topic_edit::CancelRetitle;
use crate::input::{Submitted, TextInput};
use crate::theme::Theme;

/// A topic's working directory being set in its heading: the full path, and the node it is on or
/// every node.
pub(super) struct Rooting {
    topic: TopicId,
    path: Entity<TextInput>,
    /// The nodes to pick from, this one first.
    nodes: Vec<String>,
    /// The node picked, or `None` for every node.
    node: Option<String>,
    problem: Option<String>,
    _submitted: Subscription,
}

impl Workspace {
    /// Opens the working directory field in the topic's heading, filled with where its sessions
    /// start now, its own directory or the nearest one above it.
    pub(super) fn begin_directory(&mut self, topic: TopicId, window: &mut Window, cx: &mut Context<Self>) {
        let current = Atlas::new(&self.topics, &self.sessions, &self.placements)
            .directory(&topic)
            .cloned();
        let path = cx.new(|cx| {
            TextInput::new("Full path, such as /home/you/work/project", cx)
                .keeping()
                .with_text(current.as_ref().map_or("", Directory::path))
        });
        let submitted = cx.subscribe(&path, |this, _, Submitted(_), cx| this.save_directory(cx));
        window.focus(&path.focus_handle(cx), cx);
        let node = match &current {
            Some(directory) => directory.node().map(str::to_owned),
            None => self.hub.local_node(),
        };
        self.rooting = Some(Rooting {
            topic,
            path,
            nodes: node.iter().cloned().collect(),
            node,
            problem: None,
            _submitted: submitted,
        });
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let Ok(mut nodes) = hub.agent_hosts().await else {
                return;
            };
            this.update(cx, |this, cx| {
                if let Some(rooting) = this.rooting.as_mut() {
                    if let Some(picked) = rooting.node.as_ref().filter(|picked| !nodes.contains(picked)) {
                        nodes.push(picked.clone());
                    }
                    rooting.nodes = nodes;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn cancel_directory(&mut self, _: &CancelRetitle, _: &mut Window, cx: &mut Context<Self>) {
        self.rooting = None;
        cx.notify();
    }

    fn save_directory(&mut self, cx: &mut Context<Self>) {
        let Some(rooting) = self.rooting.as_mut() else {
            return;
        };
        let typed = rooting.path.read(cx).text().to_owned();
        let Some(directory) = Directory::typed(&typed, rooting.node.clone()) else {
            rooting.problem = Some("A working directory is a full path, starting with /.".into());
            cx.notify();
            return;
        };
        let topic = rooting.topic.clone();
        self.rooting = None;
        self.set_directory(topic, Some(directory), cx);
    }

    /// Shows the topic's new directory at once and tells rh; the topics read after it has rh's
    /// word, so a refusal puts the old one back.
    fn set_directory(&mut self, topic: TopicId, directory: Option<Directory>, cx: &mut Context<Self>) {
        let Some(known) = self.topics.iter_mut().find(|known| known.id == topic) else {
            return;
        };
        known.directory.clone_from(&directory);
        let hub = self.hub.clone();
        self.edit_topics(
            async move { hub.set_topic_directory(topic, directory).await },
            "Setting its working directory failed",
            |_| {},
            cx,
        );
    }

    /// Where the step's sessions start, a click from changing it: its own directory, the one it
    /// takes from a topic above it, or an offer to set one.
    pub(super) fn directory_line(&self, step: &Topic, cx: &mut Context<Self>) -> AnyElement {
        if let Some(rooting) = self.rooting.as_ref().filter(|rooting| rooting.topic == step.id) {
            return Self::directory_field(rooting, step.directory.is_some(), cx);
        }
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        let from = atlas
            .lineage(&step.id)
            .into_iter()
            .find_map(|id| self.topic(id).filter(|known| known.directory.is_some()));
        let opened = step.id.clone();
        let line = div()
            .id("topic-directory")
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .rounded(px(8.))
            .hover(|line| line.text_color(Theme::ink()))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.begin_directory(opened.clone(), window, cx);
            }));
        match from.and_then(|known| Some((known, known.directory.as_ref()?))) {
            Some((known, directory)) => line
                .child(
                    div()
                        .font_family(Theme::MONO)
                        .text_color(Theme::ink())
                        .child(directory.path().to_owned()),
                )
                .child(match directory.node() {
                    Some(node) => format!("on {node}"),
                    None => "on every node".to_owned(),
                })
                .when(known.id != step.id, |line| {
                    line.child(format!("· from {}", known.name))
                })
                .into_any_element(),
            None => line
                .child(crate::brand::icon("icons/plus.svg", 14., Theme::ink_dim()))
                .child("Working directory")
                .into_any_element(),
        }
    }

    fn directory_field(rooting: &Rooting, own: bool, cx: &mut Context<Self>) -> AnyElement {
        let chip = |id: (&'static str, usize), label: String, picked: bool| {
            div()
                .id(id)
                .h(px(28.))
                .px_3()
                .rounded(px(14.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_xs()
                .map(|chip| {
                    if picked {
                        chip.bg(Theme::selected()).text_color(Theme::on_selected())
                    } else {
                        chip.border_1()
                            .border_color(Theme::line())
                            .text_color(Theme::ink())
                            .hover(|chip| chip.bg(Theme::card()))
                    }
                })
                .child(label)
        };
        let nodes = rooting.nodes.iter().enumerate().map(|(index, node)| {
            let picked = node.clone();
            chip(
                ("directory-node", index),
                node.clone(),
                rooting.node.as_ref() == Some(node),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if let Some(rooting) = this.rooting.as_mut() {
                    rooting.node = Some(picked.clone());
                }
                cx.notify();
            }))
        });
        let cleared = rooting.topic.clone();
        div()
            .key_context("TopicRetitle")
            .on_action(cx.listener(Self::cancel_directory))
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(420.))
                            .px_3()
                            .py_1()
                            .rounded(px(10.))
                            .bg(Theme::panel())
                            .border_1()
                            .border_color(Theme::accent())
                            .font_family(Theme::MONO)
                            .text_color(Theme::ink())
                            .child(rooting.path.clone()),
                    )
                    .children(nodes)
                    .child(
                        chip(("directory-every", 0), "Every node".to_owned(), rooting.node.is_none()).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                if let Some(rooting) = this.rooting.as_mut() {
                                    rooting.node = None;
                                }
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        chip(("directory-save", 0), "Save".to_owned(), true)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save_directory(cx))),
                    )
                    .when(own, |row| {
                        row.child(
                            chip(("directory-clear", 0), "Clear".to_owned(), false).on_click(cx.listener(
                                move |this, _: &ClickEvent, _, cx| {
                                    this.rooting = None;
                                    this.set_directory(cleared.clone(), None, cx);
                                },
                            )),
                        )
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rooting.problem.as_ref().map_or_else(Theme::ink_dim, |_| Theme::warn()))
                    .child(rooting.problem.clone().unwrap_or_else(|| {
                        "New sessions here start in this folder, on the node picked. Enter saves · Esc cancels"
                            .to_owned()
                    })),
            )
            .into_any_element()
    }
}
