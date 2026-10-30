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

use crate::page::{self, Page};
use crate::{Error, Loader, scheme};

pub(crate) enum Command {
    Open(Box<page::Opening>),
    To(u64, page::Command),
    Close(u64),
}

static ENGINE: OnceLock<Result<UnboundedSender<Command>, Error>> = OnceLock::new();

/// Sends a command to the web thread, starting it the first time.
pub(crate) fn send(command: Command) -> Result<(), Error> {
    let commands = ENGINE.get_or_init(start).as_ref().map_err(Clone::clone)?;
    commands
        .unbounded_send(command)
        .map_err(|_| Error("the web thread stopped".into()))
}

fn start() -> Result<UnboundedSender<Command>, Error> {
    let (commands, received) = unbounded();
    let (started, starting) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("webview".into())
        .spawn(move || run(received, &started))
        .map_err(|err| Error(err.to_string()))?;
    starting
        .recv()
        .map_err(|_| Error("the web thread ended as it started".into()))??;
    Ok(commands)
}

fn run(commands: UnboundedReceiver<Command>, started: &std::sync::mpsc::Sender<Result<(), Error>>) {
    if let Err(err) = gtk::init() {
        started.send(Err(Error(err.to_string()))).ok();
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

/// Each origin's host name and the loader that answers it.
#[derive(Default)]
pub(crate) struct Origins {
    hosts: HashMap<String, String>,
    loaders: HashMap<String, Loader>,
}

impl Origins {
    /// The host for `origin`, the same each time it's asked for.
    fn host(&mut self, origin: &str, loader: Loader) -> String {
        let next = self.hosts.len() + 1;
        let host = self
            .hosts
            .entry(origin.to_owned())
            .or_insert_with(|| format!("o{next}"))
            .clone();
        self.loaders.insert(host.clone(), loader);
        host
    }

    pub(crate) fn loader(&self, host: &str) -> Option<Loader> {
        self.loaders.get(host).cloned()
    }
}

struct Web {
    context: webkit2gtk::WebContext,
    origins: Rc<RefCell<Origins>>,
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
        let answering = Rc::clone(&origins);
        context.register_uri_scheme(scheme::SCHEME, move |request| {
            scheme::answer(request, &answering.borrow());
        });
        context.connect_download_started(|_, download| download.cancel());
        Self {
            context,
            origins,
            pages: HashMap::new(),
        }
    }

    async fn serve(mut self, mut commands: UnboundedReceiver<Command>) {
        while let Some(command) = commands.next().await {
            match command {
                Command::Open(opening) => {
                    let host = self
                        .origins
                        .borrow_mut()
                        .host(&opening.options.origin, opening.options.loader.clone());
                    let id = opening.id;
                    let page = Page::open(&self.context, *opening, &host);
                    self.pages.insert(id, page);
                }
                Command::To(id, command) => {
                    if let Some(page) = self.pages.get(&id) {
                        page.handle(command);
                    }
                }
                Command::Close(id) => {
                    if let Some(page) = self.pages.remove(&id) {
                        page.close();
                    }
                }
            }
        }
    }
}
