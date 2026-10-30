use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

use gpui::{ClipboardItem, Context, Entity, SharedString};
use model::plugin::view::{Action, Answer, Tone};
use model::plugin::{Context as About, PageKey, Params, PluginRef};

use super::frame::{Frame, Placement};
use super::{Dialog, Origin, Plugins, PluginsEvent, Toast};

const TOAST_FOR: Duration = Duration::from_secs(4);
const TOASTS_SHOWN: usize = 3;

/// Where saved files go: `$XDG_DOWNLOAD_DIR`, else `~/Downloads`.
fn downloads() -> PathBuf {
    std::env::var_os("XDG_DOWNLOAD_DIR").map_or_else(
        || {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("Downloads")
        },
        PathBuf::from,
    )
}

impl Plugins {
    pub(super) fn run(
        &mut self,
        origin: Origin,
        plugin: PluginRef,
        context: About,
        actions: Vec<Action>,
        cx: &mut Context<Self>,
    ) {
        let mut origin = origin;
        let mut queue: VecDeque<Action> = actions.into();
        while let Some(action) = queue.pop_front() {
            let leaves = matches!(
                action,
                Action::Navigate { .. } | Action::OpenSession(_) | Action::OpenTopic(_)
            );
            if leaves && origin == Origin::Dialog {
                origin = self.close_dialog(cx);
            }
            match action {
                Action::Call { action, params } => {
                    self.call(origin, plugin, context, (action, params), queue.into(), cx);
                    return;
                }
                Action::Navigate {
                    view,
                    params,
                    replace,
                } => self.navigate(&origin, &plugin, &context, (view, params), replace, cx),
                Action::Dialog { view, params } => {
                    self.open_dialog(&origin, plugin.clone(), context.clone(), (view, params), cx);
                }
                Action::Back => match &origin {
                    Origin::Dialog => origin = self.close_dialog(cx),
                    Origin::Page(key) => self.pop(&key.clone(), cx),
                    Origin::Card | Origin::Session => {}
                },
                Action::Refresh => self.refresh(&plugin, None, cx),
                Action::Toast { text, tone } => self.toast(text, tone, cx),
                Action::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
                Action::OpenSession(session) => cx.emit(PluginsEvent::OpenSession(session)),
                Action::OpenTopic(topic) => cx.emit(PluginsEvent::OpenTopic(topic)),
                Action::OpenUrl(url) => cx.open_url(&url),
                Action::Download { path, filename } => self.download(plugin.clone(), path, filename, cx),
            }
        }
    }

    /// Runs one of a plug-in's actions that no form holds, such as a session-menu entry, then
    /// what it answers and what was waiting behind it.
    fn call(
        &self,
        origin: Origin,
        plugin: PluginRef,
        context: About,
        (action, params): (String, Option<Params>),
        rest: Vec<Action>,
        cx: &mut Context<Self>,
    ) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let answered = hub
                .plugin_action(plugin.clone(), action, params, BTreeMap::new(), context.clone())
                .await;
            this.update(cx, |this, cx| match answered {
                Ok(Answer::Done(mut actions)) => {
                    actions.retain(|action| !matches!(action, Action::Call { .. }));
                    actions.extend(rest);
                    this.run(origin, plugin, context, actions, cx);
                }
                Ok(Answer::Refused { message, .. }) => this.toast(message, Tone::Danger, cx),
                Err(err) => this.toast(err.to_string(), Tone::Danger, cx),
            })
            .ok();
        })
        .detach();
    }

    /// The page a navigate from outside a page opens in: the plug-in's first sidebar entry, else
    /// a page of its own.
    fn page_for(&mut self, plugin: &PluginRef, cx: &mut Context<Self>) -> PageKey {
        let entry = self
            .find(plugin)
            .and_then(|known| known.pages.first().map(|page| page.id.clone()));
        let key = PageKey {
            plugin: plugin.clone(),
            entry,
        };
        if key.entry.is_some() {
            self.open_page(&key, false, cx);
        }
        key
    }

    fn navigate(
        &mut self,
        origin: &Origin,
        plugin: &PluginRef,
        context: &About,
        (view, params): (String, Option<Params>),
        replace: bool,
        cx: &mut Context<Self>,
    ) {
        let (key, elsewhere) = match origin {
            Origin::Page(key) => (key.clone(), false),
            Origin::Dialog | Origin::Card | Origin::Session => (self.page_for(plugin, cx), true),
        };
        let top = self.top(&key).filter(|_| replace);
        if let Some(top) = top {
            top.update(cx, |frame, cx| frame.retarget(view, params, cx));
        } else {
            let frame = self.frame(plugin.clone(), view, params, context.clone(), Placement::Page, cx);
            let shown = Self::show(frame, Origin::Page(key.clone()), cx);
            self.pages.entry(key.clone()).or_default().push(shown);
            self.settle(&key, cx);
        }
        if elsewhere {
            cx.emit(PluginsEvent::ShowPage(key));
        }
        cx.notify();
    }

    /// Goes back a page; the page uncovered is fetched again, since what was done above may have
    /// changed it.
    fn pop(&mut self, key: &PageKey, cx: &mut Context<Self>) {
        let Some(stack) = self.pages.get_mut(key).filter(|stack| stack.len() > 1) else {
            return;
        };
        stack.pop();
        if let Some(top) = stack.last() {
            top.frame.update(cx, Frame::load);
        }
        self.settle(key, cx);
    }

    /// Tells the page on top how many lie under it, for its back button.
    pub(super) fn settle(&self, key: &PageKey, cx: &mut Context<Self>) {
        if let Some(stack) = self.pages.get(key)
            && let Some(top) = stack.last()
        {
            let depth = stack.len() - 1;
            top.frame.update(cx, |frame, cx| frame.set_depth(depth, cx));
        }
        cx.notify();
    }

    fn open_dialog(
        &mut self,
        origin: &Origin,
        plugin: PluginRef,
        context: About,
        (view, params): (String, Option<Params>),
        cx: &mut Context<Self>,
    ) {
        let under = match origin {
            Origin::Dialog => self
                .dialog
                .as_ref()
                .map_or(Origin::Card, |dialog| dialog.under.clone()),
            other => other.clone(),
        };
        let frame = self.frame(plugin, view, params, context, Placement::Dialog, cx);
        let shown = Self::show(frame, Origin::Dialog, cx);
        self.dialog = Some(Dialog { shown, under });
        cx.notify();
    }

    /// Closes the dialog; where it was opened from, for what runs next.
    fn close_dialog(&mut self, cx: &mut Context<Self>) -> Origin {
        cx.notify();
        self.dialog.take().map_or(Origin::Card, |dialog| dialog.under)
    }

    /// Fetches again every view of the plug-in on screen, or only those named.
    pub(super) fn refresh(&self, plugin: &PluginRef, views: Option<&[String]>, cx: &mut Context<Self>) {
        let shown: Vec<Entity<Frame>> = self
            .pages
            .values()
            .filter_map(|stack| stack.last())
            .chain(self.dialog.iter().map(|dialog| &dialog.shown))
            .chain(self.cards.values())
            .map(|shown| shown.frame.clone())
            .collect();
        for frame in shown {
            let wanted = {
                let frame = frame.read(cx);
                frame.plugin() == plugin
                    && views.is_none_or(|views| views.iter().any(|view| view == frame.view_name()))
            };
            if wanted {
                frame.update(cx, Frame::load);
            }
        }
    }

    fn toast(&mut self, text: impl Into<SharedString>, tone: Tone, cx: &mut Context<Self>) {
        self.toasted += 1;
        let id = self.toasted;
        self.toasts.push(Toast {
            id,
            text: text.into(),
            tone,
        });
        let over = self.toasts.len().saturating_sub(TOASTS_SHOWN);
        self.toasts.drain(..over);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_FOR).await;
            this.update(cx, |this, cx| {
                this.toasts.retain(|toast| toast.id != id);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Saves one of the plug-in's files where the person picks, or in Downloads when the system
    /// can't ask.
    fn download(&self, plugin: PluginRef, path: String, filename: Option<String>, cx: &mut Context<Self>) {
        let name = filename.unwrap_or_else(|| {
            let route = path.split('?').next().unwrap_or_default();
            let last = route.rsplit('/').next().unwrap_or_default();
            if last.is_empty() {
                "download".to_owned()
            } else {
                last.to_owned()
            }
        });
        let folder = downloads();
        let asked = cx.prompt_for_new_path(&folder, Some(&name));
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let target = match asked.await {
                Ok(Ok(Some(target))) => target,
                Ok(Ok(None)) => return,
                _ => folder.join(&name),
            };
            let saved = match hub.plugin_file(plugin, path).await {
                Ok((bytes, _)) => cx
                    .background_executor()
                    .spawn(async move { std::fs::write(&target, bytes).map(|()| target) })
                    .await
                    .map_err(|err| err.to_string()),
                Err(err) => Err(err.to_string()),
            };
            this.update(cx, |this, cx| match saved {
                Ok(target) => this.toast(format!("Saved {}", target.display()), Tone::Success, cx),
                Err(problem) => this.toast(format!("Couldn't save {name}: {problem}"), Tone::Danger, cx),
            })
            .ok();
        })
        .detach();
    }
}
