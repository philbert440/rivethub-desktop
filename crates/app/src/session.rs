use std::sync::Arc;
use std::time::Duration;

use gateway::Gateway;
use model::SessionId;
use model::session::{SessionEvent, SessionView};
use parking_lot::RwLock;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};

const FIRST_RETRY: Duration = Duration::from_millis(500);
const LAST_RETRY: Duration = Duration::from_secs(15);
const COMMAND_BUFFER: usize = 32;

#[derive(Debug)]
enum Command {
    Send(String),
    Stop,
}

#[derive(Debug, Clone)]
pub struct SessionHandle {
    id: SessionId,
    view: Arc<RwLock<SessionView>>,
    revision: watch::Receiver<u64>,
    commands: mpsc::Sender<Command>,
}

impl SessionHandle {
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub fn view(&self) -> Arc<RwLock<SessionView>> {
        Arc::clone(&self.view)
    }

    pub fn revisions(&self) -> watch::Receiver<u64> {
        self.revision.clone()
    }

    pub fn send(&self, text: String) {
        let _ = self.commands.try_send(Command::Send(text));
    }

    pub fn stop(&self) {
        let _ = self.commands.try_send(Command::Stop);
    }
}

struct Actor {
    gateway: Gateway,
    id: SessionId,
    view: Arc<RwLock<SessionView>>,
    revision: watch::Sender<u64>,
}

impl Actor {
    fn apply(&self, event: SessionEvent) {
        self.view.write().apply(event);
        self.revision.send_modify(|revision| *revision += 1);
    }

    async fn run(self, mut commands: mpsc::Receiver<Command>) {
        let mut retry = FIRST_RETRY;
        loop {
            match self.gateway.open_session(&self.id).await {
                Ok(mut stream) => {
                    retry = FIRST_RETRY;
                    stream.resync();
                    loop {
                        tokio::select! {
                            event = stream.events.recv() => match event {
                                Some(event) => self.apply(event),
                                None => break,
                            },
                            command = commands.recv() => match command {
                                Some(command) => self.handle(command).await,
                                None => return,
                            },
                        }
                    }
                }
                Err(err) => self.apply(SessionEvent::Error(err.to_string())),
            }
            tokio::select! {
                () = tokio::time::sleep(retry) => {}
                command = commands.recv() => match command {
                    Some(command) => self.handle(command).await,
                    None => return,
                },
            }
            retry = (retry * 2).min(LAST_RETRY);
        }
    }

    async fn handle(&self, command: Command) {
        let outcome = match command {
            Command::Send(text) => self.gateway.send_turn(&self.id, &text).await,
            Command::Stop => self.gateway.stop(&self.id).await,
        };
        if let Err(err) = outcome {
            self.apply(SessionEvent::Error(err.to_string()));
        }
    }
}

pub fn spawn(runtime: &Handle, gateway: Gateway, id: SessionId) -> SessionHandle {
    let view = Arc::new(RwLock::new(SessionView::default()));
    let (revision_tx, revision) = watch::channel(0);
    let (commands, commands_rx) = mpsc::channel(COMMAND_BUFFER);
    let actor = Actor {
        gateway,
        id: id.clone(),
        view: Arc::clone(&view),
        revision: revision_tx,
    };
    runtime.spawn(actor.run(commands_rx));
    SessionHandle {
        id,
        view,
        revision,
        commands,
    }
}
