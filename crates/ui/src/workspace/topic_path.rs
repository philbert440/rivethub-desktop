use std::time::{Duration, Instant};

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, IntoElement, MouseButton, ParentElement, Styled, div,
    prelude::*, px,
};
use model::TopicId;
use model::topics::{Atlas, MAX_LEVELS, Topic};

use super::Workspace;
use super::topic_edit::{DraggedTopic, Pick};
use crate::memory::Focus;
use crate::side_panel::{DragGhost, DraggedSession};
use crate::theme::Theme;

/// How long a click waits for a second one, as the platform counts a double-click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

impl Workspace {
    /// Where the topic screen stands inside the open topic, the topic itself shown as "All", and
    /// one row of the current step's subtopics. A step or pill takes a click to go there, a
    /// right-click for its menu, a dropped session to file it there and a dropped topic to nest it.
    pub(super) fn topic_path(&self, root: &TopicId, step: &TopicId, cx: &mut Context<Self>) -> AnyElement {
        let atlas = Atlas::new(&self.topics, &self.filed_sessions, &self.placements);
        let mut path = atlas.lineage(step);
        path.reverse();
        let last = path.len().saturating_sub(1);
        let deeper = path.len() < MAX_LEVELS;
        let mut steps = div().flex().flex_wrap().items_center().gap_1().text_sm();
        for (index, id) in path.into_iter().enumerate() {
            if index > 0 {
                steps = steps.child(div().px_1().text_color(Theme::ink_dim()).child("›"));
            }
            let count = atlas.filed_within(id, &self.conversations);
            let label = match self.topic(id) {
                Some(_) if id == root => format!("All · {count}"),
                Some(known) => format!("{} · {count}", known.name),
                None => continue,
            };
            steps = steps.child(self.path_step(index, id, root, label, index == last, cx));
        }
        let children = atlas.children(step);
        let renaming = step.clone();
        let pills: Vec<AnyElement> = children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                let below = atlas.children(&child.id).len();
                let count = atlas.filed_within(&child.id, &self.conversations);
                self.pill(index, child, count, below, cx)
            })
            .collect();
        let parent = step.clone();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(steps)
            .when(deeper || !pills.is_empty(), |path| {
                path.child(
                    div()
                        .id("subtopic-row")
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            if event.click_count() > 1 && this.just_stepped(&renaming) {
                                this.screen.stepped = None;
                                this.begin_retitle(renaming.clone(), window, cx);
                            }
                        }))
                        .children(pills)
                        .when(deeper, |row| {
                            row.child(
                                div()
                                    .id("add-subtopic")
                                    .h(px(34.))
                                    .px_4()
                                    .rounded(px(17.))
                                    .border_1()
                                    .border_dashed()
                                    .border_color(Theme::line())
                                    .flex()
                                    .items_center()
                                    .cursor_pointer()
                                    .text_sm()
                                    .text_color(Theme::ink_dim())
                                    .hover(|pill| pill.bg(Theme::card()))
                                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                                        if event.click_count() > 1 && this.just_stepped(&parent) {
                                            return;
                                        }
                                        this.choose(
                                            Pick::Child(parent.clone()),
                                            event.position(),
                                            window,
                                            cx,
                                        );
                                    }))
                                    .child("+ Subtopic"),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    fn path_step(
        &self,
        index: usize,
        id: &TopicId,
        root: &TopicId,
        label: String,
        current: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !current && let Some(field) = self.retitle_field(id, cx) {
            return Self::pill_sized(field);
        }
        let (target, renamed) = (id.clone(), id.clone());
        let step = (id != root).then(|| id.clone());
        let chip = div()
            .id(("path-step", index))
            .h(px(30.))
            .px_3()
            .rounded(px(15.))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if current && event.click_count() == 2 {
                    this.begin_retitle(renamed.clone(), window, cx);
                } else if !current {
                    this.screen.step.clone_from(&step);
                    this.screen.all_history = false;
                    this.sync_step(cx);
                    cx.notify();
                }
            }));
        let chip = if current {
            chip.bg(Theme::selected()).text_color(Theme::on_selected())
        } else {
            chip.text_color(Theme::ink_dim())
                .hover(|chip| chip.text_color(Theme::ink()))
        };
        Self::takes_drops(chip, target, cx)
            .child(label)
            .into_any_element()
    }

    fn pill(
        &self,
        index: usize,
        child: &Topic,
        count: usize,
        below: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(field) = self.retitle_field(&child.id, cx) {
            return Self::pill_sized(field);
        }
        let (clicked, step) = (child.id.clone(), child.parent.clone());
        let dragged = DraggedTopic {
            topic: child.id.clone(),
            name: child.name.clone().into(),
        };
        let pill = div()
            .id(ElementId::NamedInteger("subtopic".into(), index as u64))
            .h(px(34.))
            .px_4()
            .rounded(px(17.))
            .border_1()
            .border_color(Theme::line())
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .text_sm()
            .text_color(Theme::ink())
            .hover(|pill| pill.bg(Theme::card()))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.click_count() == 1 {
                    this.step_into(clicked.clone(), cx);
                } else if !step.as_ref().is_some_and(|step| this.just_stepped(step)) {
                    this.begin_retitle(clicked.clone(), window, cx);
                }
            }))
            .on_drag(dragged, |dragged, _, _, cx| {
                cx.new(|_| DragGhost(dragged.name.clone()))
            })
            .child(child.name.clone())
            .children(self.topic_glyph(child))
            .child(
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(count.to_string()),
            )
            .when(below > 0, |pill| {
                pill.child(
                    div()
                        .pl_2()
                        .border_l_1()
                        .border_color(Theme::line())
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child(format!("{below} ›")),
                )
            });
        Self::takes_drops(pill, child.id.clone(), cx).into_any_element()
    }

    /// A name field in the path or the pill row, about as wide as a pill rather than the whole row.
    fn pill_sized(field: AnyElement) -> AnyElement {
        div().w(px(260.)).child(field).into_any_element()
    }

    /// Steps into `topic` at once. A second click that makes this one a double-click lands on
    /// the row of its subtopics, which takes it as the pill's double-click.
    fn step_into(&mut self, topic: TopicId, cx: &mut Context<Self>) {
        self.screen.stepped = Some((topic.clone(), Instant::now()));
        self.screen.step = Some(topic);
        self.screen.all_history = false;
        self.sync_step(cx);
        cx.notify();
    }

    /// Whether `topic` was stepped into from its pill within a double-click's time.
    fn just_stepped(&self, topic: &TopicId) -> bool {
        self.screen
            .stepped
            .as_ref()
            .is_some_and(|(stepped, at)| stepped == topic && at.elapsed() < DOUBLE_CLICK)
    }

    /// A step or pill's right-click menu, and the sessions and topics it takes when dropped on it.
    fn takes_drops(
        element: gpui::Stateful<gpui::Div>,
        topic: TopicId,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let (filed, nested) = (topic.clone(), topic.clone());
        element
            .drag_over::<DraggedSession>(|style, _, _, _| {
                style.bg(Theme::selected()).text_color(Theme::on_selected())
            })
            .drag_over::<DraggedTopic>(|style, _, _, _| {
                style.bg(Theme::selected()).text_color(Theme::on_selected())
            })
            .on_drop(cx.listener(move |this, dragged: &DraggedSession, _, cx| {
                cx.stop_propagation();
                this.place(dragged.0.id.clone(), filed.clone(), cx);
            }))
            .on_drop(cx.listener(move |this, dragged: &DraggedTopic, _, cx| {
                cx.stop_propagation();
                this.nest(dragged.topic.clone(), Some(nested.clone()), cx);
            }))
            .on_mouse_down(MouseButton::Right, Self::on_right_click(Focus::Topic(topic), cx))
    }
}
