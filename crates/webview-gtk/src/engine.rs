//! The web thread: GTK and `WebKit` live on it, running GTK's own main loop, so the app's loop
//! never polls theirs and a slow page never holds up the app's frames.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gtk::glib;
use webkit2gtk::{DownloadExt, SecurityManagerExt, WebContextExt};
use webview::protocol::ToEngine;

use crate::page::Page;
use crate::scheme::{self, Pending};

pub(crate) enum Command {
    App(ToEngine),
    Respond {
        request: u64,
        status: u16,
        media_type: Option<String>,
        body: Vec<u8>,
    },
}

static COMMANDS: OnceLock<UnboundedSender<Command>> = OnceLock::new();

pub(crate) fn send(command: Command) {
    if let Some(commands) = COMMANDS.get() {
        commands.unbounded_send(command).ok();
    }
}

/// Starts the web thread and waits until GTK is up on it.
pub(crate) fn start() -> Result<(), String> {
    let (commands, received) = unbounded();
    let (started, starting) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("webview".into())
        .spawn(move || run(received, &started))
        .map_err(|err| err.to_string())?;
    starting
        .recv()
        .map_err(|_| "the web thread ended as it started".to_owned())??;
    COMMANDS.set(commands).ok();
    Ok(())
}

fn run(commands: UnboundedReceiver<Command>, started: &std::sync::mpsc::Sender<Result<(), String>>) {
    if let Err(err) = gtk::init() {
        started.send(Err(format!("GTK: {err}"))).ok();
        return;
    }
    let web = Web::new();
    started.send(Ok(())).ok();
    let main = glib::MainLoop::new(None, false);
    let ending = main.clone();
    glib::MainContext::default().spawn_local(async move {
        web.serve(commands).await;
        ending.quit();
    });
    main.run();
}

/// Each origin's host name, and the pages open at each host, which answer its requests.
#[derive(Default)]
pub(crate) struct Origins {
    hosts: HashMap<String, String>,
    pages: HashMap<String, Vec<u64>>,
}

impl Origins {
    /// The host for `origin`, the same each time it's asked for.
    fn host(&mut self, origin: &str, panel: u64) -> String {
        let next = self.hosts.len() + 1;
        let host = self
            .hosts
            .entry(origin.to_owned())
            .or_insert_with(|| format!("o{next}"))
            .clone();
        self.pages.entry(host.clone()).or_default().push(panel);
        host
    }

    fn close(&mut self, panel: u64) {
        for pages in self.pages.values_mut() {
            pages.retain(|open| *open != panel);
        }
    }

    /// A page open at `host`, to answer its requests.
    pub(crate) fn page(&self, host: &str) -> Option<u64> {
        self.pages.get(host).and_then(|pages| pages.first()).copied()
    }
}

struct Web {
    context: webkit2gtk::WebContext,
    origins: Rc<RefCell<Origins>>,
    pending: Pending,
    pages: HashMap<u64, Page>,
}

impl Web {
    fn new() -> Self {
        let context = webkit2gtk::WebContext::new_ephemeral();
        // A page is a plug-in's own code: its process gets bubblewrap's sandbox and the smallest
        // cache, and loads nothing but what `scheme` answers.
        context.set_sandbox_enabled(true);
        context.set_cache_model(webkit2gtk::CacheModel::DocumentViewer);
        if let Some(security) = context.security_manager() {
            security.register_uri_scheme_as_secure(scheme::SCHEME);
        }
        let origins: Rc<RefCell<Origins>> = Rc::default();
        let pending = Pending::default();
        let (answering, waiting) = (Rc::clone(&origins), pending.clone());
        context.register_uri_scheme(scheme::SCHEME, move |request| {
            scheme::ask(request, &answering.borrow(), &waiting);
        });
        context.connect_download_started(|_, download| download.cancel());
        Self {
            context,
            origins,
            pending,
            pages: HashMap::new(),
        }
    }

    async fn serve(mut self, mut commands: UnboundedReceiver<Command>) {
        while let Some(command) = commands.next().await {
            match command {
                Command::App(ToEngine::Open {
                    panel,
                    origin,
                    entry,
                    style,
                    background,
                    size,
                }) => {
                    let host = self.origins.borrow_mut().host(&origin, panel);
                    let opened = Page::open(&self.context, panel, &host, &entry, &style, background, size);
                    self.pages.insert(panel, opened);
                }
                Command::App(ToEngine::To { panel, command }) => {
                    if let Some(page) = self.pages.get(&panel) {
                        page.handle(command);
                    }
                }
                Command::App(ToEngine::Close { panel }) => {
                    self.origins.borrow_mut().close(panel);
                    if let Some(page) = self.pages.remove(&panel) {
                        page.close();
                    }
                }
                Command::Respond {
                    request,
                    status,
                    media_type,
                    body,
                } => self.pending.answer(request, status, media_type.as_deref(), body),
            }
        }
    }
}
