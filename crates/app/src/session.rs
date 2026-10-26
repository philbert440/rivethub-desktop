use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use gateway::Gateway;
use model::SessionId;
use model::session::{Decision, SessionEvent, SessionView};
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
    Unqueue(usize),
    Decide { request_id: String, decision: Decision },
}

#[derive(Debug, Clone)]
pub struct SessionHandle {
    id: SessionId,
    view: Arc<RwLock<SessionView>>,
    queue: Arc<RwLock<VecDeque<String>>>,
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

    pub fn queued(&self) -> Vec<String> {
        self.queue.read().iter().cloned().collect()
    }

    pub fn unqueue(&self, index: usize) {
        let _ = self.commands.try_send(Command::Unqueue(index));
    }

    pub fn decide(&self, request_id: String, decision: Decision) {
        let _ = self.commands.try_send(Command::Decide { request_id, decision });
    }
}

struct Actor {
    gateway: Gateway,
    id: SessionId,
    view: Arc<RwLock<SessionView>>,
    queue: Arc<RwLock<VecDeque<String>>>,
    revision: watch::Sender<u64>,
}

impl Actor {
    fn changed(&self) {
        self.revision.send_modify(|revision| *revision += 1);
    }

    async fn apply(&self, event: SessionEvent) {
        let finished = event == SessionEvent::TurnComplete;
        self.view.write().apply(event);
        self.changed();
        if finished {
            self.drain().await;
        }
    }

    async fn drain(&self) {
        if self.view.read().busy() {
            return;
        }
        let Some(next) = self.queue.write().pop_front() else {
            return;
        };
        self.changed();
        self.send(next).await;
    }

    async fn send(&self, text: String) {
        match self.gateway.send_turn(&self.id, &text).await {
            Ok(()) => {}
            Err(err) if err.is_turn_in_flight() => {
                self.queue.write().push_front(text);
                self.changed();
            }
            Err(err) => self.fail(&err.to_string()),
        }
    }

    fn fail(&self, message: &str) {
        self.view.write().apply(SessionEvent::Error(message.to_owned()));
        self.changed();
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
                                Some(event) => self.apply(event).await,
                                None => break,
                            },
                            command = commands.recv() => match command {
                                Some(command) => self.handle(command).await,
                                None => return,
                            },
                        }
                    }
                }
                Err(err) => self.fail(&err.to_string()),
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
            Command::Send(text) => {
                if self.view.read().busy() || !self.queue.read().is_empty() {
                    self.queue.write().push_back(text);
                    self.changed();
                } else {
                    self.send(text).await;
                }
                Ok(())
            }
            Command::Unqueue(index) => {
                self.queue.write().remove(index);
                self.changed();
                Ok(())
            }
            Command::Stop => self.gateway.stop(&self.id).await,
            Command::Decide { request_id, decision } => {
                self.gateway.decide(&self.id, &request_id, decision).await
            }
        };
        if let Err(err) = outcome {
            self.fail(&err.to_string());
        }
    }
}

pub fn spawn(runtime: &Handle, gateway: Gateway, id: SessionId) -> SessionHandle {
    let view = Arc::new(RwLock::new(SessionView::default()));
    let queue = Arc::new(RwLock::new(VecDeque::new()));
    let (revision_tx, revision) = watch::channel(0);
    let (commands, commands_rx) = mpsc::channel(COMMAND_BUFFER);
    let actor = Actor {
        gateway,
        id: id.clone(),
        view: Arc::clone(&view),
        queue: Arc::clone(&queue),
        revision: revision_tx,
    };
    runtime.spawn(actor.run(commands_rx));
    SessionHandle {
        id,
        view,
        queue,
        revision,
        commands,
    }
}
