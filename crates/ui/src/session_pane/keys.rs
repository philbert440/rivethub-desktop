use std::time::{Duration, Instant};

use gpui::{Context, Window};
use model::prompt::{Prompt, Switch};

use super::{EscapeComposer, SessionPane, SteerDraft};
use crate::input::TextInput;

/// How soon the second Esc has to follow the first to stop the running turn.
const TWICE: Duration = Duration::from_millis(600);

impl SessionPane {
    fn busy(&self) -> bool {
        self.handle.view().read().busy()
    }

    /// Esc twice in the composer, close together, stops the running turn. The first one says so
    /// beside Stop until its time is up.
    pub(super) fn escape_composer(&mut self, _: &EscapeComposer, _: &mut Window, cx: &mut Context<Self>) {
        if !self.busy() {
            self.escaped = None;
            return;
        }
        let now = Instant::now();
        if self
            .escaped
            .is_some_and(|first| now.duration_since(first) <= TWICE)
        {
            self.escaped = None;
            self.handle.stop();
        } else {
            self.escaped = Some(now);
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TWICE).await;
                this.update(cx, |this, cx| {
                    if this.escaped == Some(now) {
                        this.escaped = None;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    /// Ctrl+Shift+Enter in the composer hands the draft to the running turn, for a harness that
    /// takes steering. Otherwise it does nothing: the draft stays, and nothing is sent.
    pub(super) fn steer_draft(&mut self, _: &SteerDraft, _: &mut Window, cx: &mut Context<Self>) {
        if !self.steers() || !self.busy() || self.uploading > 0 {
            return;
        }
        let Some(text) = self.composer.update(cx, TextInput::take) else {
            return;
        };
        let prompt = Prompt {
            text,
            attachments: std::mem::take(&mut self.attachments),
            switch: Switch::default(),
        };
        // A steer goes through the queue, as Steer on a queued message does.
        self.handle.send(prompt.clone());
        self.handle.steer(prompt);
        cx.notify();
    }
}
