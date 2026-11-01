use gateway::GatewayError;
use gpui::Context;
use model::SessionId;
use model::memory::{SessionMemory, Summary};

use super::MemoryPage;
use super::recalled::Recalled;

/// How many of a session's top-level summaries are read at a time.
const TOP_WINDOW: usize = 40;

/// A part of a session's memory read since its page opened.
enum MemoryRead {
    Children(Vec<Summary>),
    Top(SessionMemory),
}

impl MemoryPage {
    pub(super) fn load_session(&mut self, cx: &mut Context<Self>) {
        let Some(viewing) = self.viewing.as_mut() else {
            return;
        };
        viewing.loading = true;
        let id = viewing.session.id.clone();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let memory = hub.session_tree(id.clone(), 0, TOP_WINDOW).await;
            this.update(cx, |this, cx| {
                let Some(viewing) = this.viewing.as_mut().filter(|viewing| viewing.session.id == id) else {
                    return;
                };
                viewing.loading = false;
                match memory {
                    Ok(memory)
                        if viewing
                            .recalled
                            .as_ref()
                            .is_some_and(|recalled| recalled.shows(&memory)) => {}
                    Ok(memory) => {
                        match viewing.recalled.as_mut() {
                            Some(recalled) if recalled.tree.is_some() && memory.counts.is_some() => {
                                recalled.refresh(memory);
                            }
                            _ => viewing.recalled = Some(Recalled::new(memory, viewing.recalled.take())),
                        }
                        this.problem = None;
                        cx.notify();
                    }
                    Err(err) => this.failed(&err, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Reads what the rows from this one on want of the open session's memory: the children of
    /// summaries that have some not read yet, and the next window of the top level near its end.
    pub(super) fn read_ahead(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(viewing) = self.viewing.as_mut() else {
            return;
        };
        let Some(recalled) = viewing.recalled.as_mut() else {
            return;
        };
        let wants = recalled.wants(row);
        let id = viewing.session.id.clone();
        for summary in wants.children {
            let (hub, id) = (self.hub.clone(), id.clone());
            cx.spawn(async move |this, cx| {
                let read = hub.summary_children(id.clone(), summary).await;
                this.update(cx, |this, cx| {
                    this.memory_read(&id, read.map(MemoryRead::Children), cx);
                })
                .ok();
            })
            .detach();
        }
        if let Some(from) = wants.top_from {
            let hub = self.hub.clone();
            cx.spawn(async move |this, cx| {
                let read = hub.session_tree(id.clone(), from, TOP_WINDOW).await;
                this.update(cx, |this, cx| {
                    this.memory_read(&id, read.map(MemoryRead::Top), cx);
                })
                .ok();
            })
            .detach();
        }
    }

    /// Adds what was read of the open session's memory to its page.
    fn memory_read(
        &mut self,
        id: &SessionId,
        read: Result<MemoryRead, GatewayError>,
        cx: &mut Context<Self>,
    ) {
        let Some(recalled) = self
            .viewing
            .as_mut()
            .filter(|viewing| viewing.session.id == *id)
            .and_then(|viewing| viewing.recalled.as_mut())
        else {
            return;
        };
        match read {
            Ok(MemoryRead::Children(children)) => recalled.merge(children),
            Ok(MemoryRead::Top(memory)) => recalled.more_top(memory),
            Err(err) => self.failed(&err, cx),
        }
        cx.notify();
    }

    /// Reads one of the open session's summaries whole, for a summary opened past its start.
    pub(super) fn read_whole(&self, summary: String, cx: &mut Context<Self>) {
        let Some(viewing) = self.viewing.as_ref() else {
            return;
        };
        let id = viewing.session.id.clone();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let read = hub.session_summary(id.clone(), summary).await;
            this.update(cx, |this, cx| {
                let Some(viewing) = this.viewing.as_mut().filter(|viewing| viewing.session.id == id) else {
                    return;
                };
                match read {
                    Ok(whole) => {
                        if let Some(recalled) = viewing.recalled.as_mut() {
                            recalled.read_whole(whole);
                        }
                        cx.notify();
                    }
                    Err(err) => this.failed(&err, cx),
                }
            })
            .ok();
        })
        .detach();
    }
}
