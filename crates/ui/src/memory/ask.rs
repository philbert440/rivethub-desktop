use gpui::{
    AnyElement, ClickEvent, Context, HighlightStyle, IntoElement, ParentElement, Styled, StyledText, div,
    prelude::*, px,
};
use model::memory::{Found, Period, Source};
use model::search::{Snippet, snippet};

use super::{MemoryEvent, MemoryPage};
use crate::brand;
use crate::text::{ago, now_ms};
use crate::theme::Theme;

const COLUMN: f32 = 860.;

impl MemoryPage {
    pub(super) fn search(&mut self, query: String, cx: &mut Context<Self>) {
        self.asked = Some(query.clone());
        self.found = None;
        self.problem = None;
        let (hub, period) = (self.hub.clone(), self.period);
        cx.spawn(async move |this, cx| {
            let found = hub.recall(query.clone(), period).await;
            this.update(cx, |this, cx| {
                if this.asked.as_ref() != Some(&query) || this.period != period {
                    return;
                }
                match found {
                    Ok(found) => {
                        this.found = Some(found);
                        cx.notify();
                    }
                    Err(err) => this.failed(&err, cx),
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn pick_period(&mut self, period: Period, cx: &mut Context<Self>) {
        if period == self.period {
            return;
        }
        self.period = period;
        match self.asked.clone() {
            Some(query) => self.search(query, cx),
            None => cx.notify(),
        }
    }

    pub(super) fn ask(&self, cx: &mut Context<Self>) -> AnyElement {
        let bar = div()
            .px_5()
            .py_3()
            .rounded(px(26.))
            .bg(Theme::card())
            .border_1()
            .border_color(Theme::accent())
            .flex()
            .items_center()
            .gap_3()
            .child(brand::icon("icons/search.svg", 18., Theme::ink_dim()))
            .child(div().flex_1().text_base().child(self.query.clone()));
        let periods = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(Period::ALL.map(|period| {
                let here = period == self.period;
                div()
                    .id(period.label())
                    .px_3()
                    .py_1()
                    .rounded(px(14.))
                    .border_1()
                    .cursor_pointer()
                    .text_xs()
                    .map(|chip| {
                        if here {
                            chip.bg(Theme::selected())
                                .border_color(Theme::selected())
                                .text_color(Theme::on_selected())
                        } else {
                            chip.border_color(Theme::line())
                                .text_color(Theme::ink_dim())
                                .hover(|chip| chip.text_color(Theme::ink()))
                        }
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.pick_period(period, cx)))
                    .child(period.label())
            }));
        div()
            .id("memory-ask")
            .size_full()
            .overflow_y_scroll()
            .py_8()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(COLUMN))
                    .px_8()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(bar)
                    .child(periods)
                    .children(
                        self.problem
                            .clone()
                            .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
                    )
                    .child(self.results(cx)),
            )
            .into_any_element()
    }

    fn results(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(asked) = &self.asked else {
            return div()
                .pt_4()
                .text_color(Theme::ink_dim())
                .child("Search what your sessions said and what memory summarized from them.")
                .into_any_element();
        };
        let Some(found) = &self.found else {
            return div()
                .text_color(Theme::ink_dim())
                .child("Searching…")
                .into_any_element();
        };
        let note = match found.len() {
            0 => "nothing found".to_owned(),
            1 => "1 match".to_owned(),
            count => format!("{count} matches"),
        };
        let now = now_ms();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_3()
                    .child(div().text_color(Theme::ink()).child(format!("“{asked}”")))
                    .child(div().text_xs().text_color(Theme::ink_dim()).child(note)),
            )
            .children(
                found
                    .iter()
                    .enumerate()
                    .map(|(index, hit)| Self::hit(index, hit, asked, now, cx)),
            )
            .into_any_element()
    }

    fn hit(index: usize, hit: &Found, asked: &str, now: i64, cx: &mut Context<Self>) -> AnyElement {
        let shown = snippet(&hit.text, asked);
        let kind = match hit.source {
            Source::Message => "Message",
            Source::Summary => "Summary",
        };
        let badge = |text: String| {
            div()
                .px_2()
                .rounded(px(9.))
                .bg(Theme::surface())
                .text_xs()
                .text_color(Theme::ink_dim())
                .child(text)
        };
        div()
            .px_4()
            .py_3()
            .rounded(px(16.))
            .bg(Theme::card())
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(badge(kind.to_owned()))
                    .children(
                        hit.session
                            .as_ref()
                            .map(|session| badge(session.harness().display_name().to_owned())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(ago(now, hit.at_ms)),
                    )
                    .child(div().flex_1())
                    .children(hit.session.clone().map(|session| {
                        div()
                            .id(("open-hit", index))
                            .px_3()
                            .rounded(px(12.))
                            .border_1()
                            .border_color(Theme::line())
                            .cursor_pointer()
                            .text_xs()
                            .text_color(Theme::accent())
                            .hover(|link| link.bg(Theme::selected()))
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.emit(MemoryEvent::OpenSession(session.clone()));
                            }))
                            .child("Open session")
                    })),
            )
            .child(
                div()
                    .line_height(px(20.))
                    .text_color(Theme::ink_dim())
                    .child(Self::marked(&shown)),
            )
            .into_any_element()
    }

    fn marked(shown: &Snippet) -> StyledText {
        let start = shown.before.len();
        let end = start + shown.matched.len();
        let text = format!("{}{}{}", shown.before, shown.matched, shown.after);
        let mark = HighlightStyle {
            color: Some(Theme::warn().into()),
            background_color: Some(Theme::warn_container().into()),
            ..HighlightStyle::default()
        };
        StyledText::new(text).with_highlights([(start..end, mark)])
    }
}
