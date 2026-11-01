use std::collections::VecDeque;
use std::sync::Arc;

use model::SessionId;
use model::prompt::Prompt;
use model::session::{Decision, SessionEvent, SessionView};
use parking_lot::RwLock;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};

const COMMAND_BUFFER: usize = 32;

#[derive(Debug)]
enum Command {
    Send(Prompt),
    Answer(Prompt),
    Stop,
    Steer(Prompt),
    Unqueue(Prompt),
    Decide { request_id: String, decision: Decision },
    Earlier,
}

#[derive(Debug, Clone)]
pub struct SessionHandle {
    id: SessionId,
    view: Arc<RwLock<SessionView>>,
    queue: Arc<RwLock<VecDeque<Prompt>>>,
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

    pub fn send(&self, prompt: Prompt) {
        let _ = self.commands.try_send(Command::Send(prompt));
    }

    pub fn stop(&self) {
        let _ = self.commands.try_send(Command::Stop);
    }

    /// Answers an open ask-user question. A harness whose ask tool blocks keeps its turn running
    /// until it is stopped, so the answer goes out as the next turn after that.
    pub fn answer(&self, prompt: Prompt) {
        let _ = self.commands.try_send(Command::Answer(prompt));
    }

    pub fn queued(&self) -> Vec<Prompt> {
        self.queue.read().iter().cloned().collect()
    }

    pub fn unqueue(&self, prompt: Prompt) {
        let _ = self.commands.try_send(Command::Unqueue(prompt));
    }

    /// Hands a queued prompt to the turn that is running; with none running it goes as the next
    /// turn.
    pub fn steer(&self, prompt: Prompt) {
        let _ = self.commands.try_send(Command::Steer(prompt));
    }

    /// Takes a queued prompt back out of the queue, to change before it goes; one already sent is
    /// gone.
    pub fn take_queued(&self, prompt: &Prompt) -> Option<Prompt> {
        take(&self.queue, prompt)
    }

    pub fn decide(&self, request_id: String, decision: Decision) {
        let _ = self.commands.try_send(Command::Decide { request_id, decision });
    }

    /// Reads back the page of a long transcript before the turns the view holds.
    pub fn read_earlier(&self) {
        let _ = self.commands.try_send(Command::Earlier);
    }
}

struct Actor {
    hub: crate::Hub,
    node: Option<String>,
    id: SessionId,
    view: Arc<RwLock<SessionView>>,
    queue: Arc<RwLock<VecDeque<Prompt>>>,
    revision: watch::Sender<u64>,
}

impl Actor {
    async fn gateway(&self) -> Result<gateway::Gateway, gateway::GatewayError> {
        self.hub.gateway_for(self.node.as_deref()).await
    }

    fn changed(&self) {
        self.revision.send_modify(|revision| *revision += 1);
    }

    async fn apply(&self, event: SessionEvent) {
        let finished = matches!(event, SessionEvent::TurnComplete { .. });
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

    async fn send(&self, prompt: Prompt) {
        let outcome = match self.gateway().await {
            Ok(gateway) => gateway.send_turn(&self.id, &prompt).await,
            Err(err) => Err(err),
        };
        match outcome {
            Ok(()) => {
                let held = {
                    let view = self.view.read();
                    view.driving().by_terminal() && view.driving().busy
                };
                if !held {
                    self.view.write().accepted(&prompt, epoch_ms());
                }
                self.changed();
            }
            Err(err) if err.is_turn_in_flight() => {
                self.queue.write().push_front(prompt);
                self.changed();
            }
            Err(err) => self.fail(&err.to_string()),
        }
    }

    async fn steer(&self, prompt: Prompt) -> Result<(), gateway::GatewayError> {
        let Some(prompt) = take(&self.queue, &prompt) else {
            return Ok(());
        };
        self.changed();
        let steered = match self.gateway().await {
            Ok(gateway) => gateway.steer(&self.id, &prompt).await,
            Err(err) => Err(err),
        };
        match steered {
            Ok(true) => {
                self.view.write().steered(&prompt, epoch_ms());
                self.changed();
                Ok(())
            }
            Ok(false) => {
                self.queue.write().push_front(prompt);
                self.changed();
                self.drain().await;
                Ok(())
            }
            Err(err) => {
                self.queue.write().push_front(prompt);
                self.changed();
                Err(err)
            }
        }
    }

    async fn earlier(&self) -> Result<(), gateway::GatewayError> {
        let Some(before) = self.view.write().begin_earlier() else {
            return Ok(());
        };
        self.changed();
        let read = match self.gateway().await {
            Ok(gateway) => gateway.earlier(&self.id, before).await,
            Err(err) => Err(err),
        };
        let outcome = match read {
            Ok(page) => {
                self.view.write().earlier_read(before, page);
                Ok(())
            }
            Err(err) => {
                self.view.write().earlier_failed();
                Err(err)
            }
        };
        self.changed();
        outcome
    }

    fn fail(&self, message: &str) {
        self.view.write().apply(SessionEvent::Error(message.to_owned()));
        self.changed();
    }

    /// No stream follows the session now, as `message` says, until it reconnects.
    fn lose(&self, message: &str) {
        self.view.write().disconnected(message.to_owned());
        self.changed();
    }

    async fn run(self, mut commands: mpsc::Receiver<Command>) {
        let mut retry = self.hub.backoff(self.node.as_deref());
        loop {
            let opened = match self.gateway().await {
                Ok(gateway) => gateway.open_session(&self.id).await,
                Err(err) => Err(err),
            };
            match opened {
                Ok(mut stream) => {
                    retry.reset();
                    self.view.write().reconnected();
                    self.changed();
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
                    self.lose("Lost the connection to this session's node. Reconnecting…");
                }
                Err(err) if err.is_unreachable() => {
                    tracing::debug!(%err, session = %self.id, "the session's node is out of reach");
                    self.lose("Can't reach this session's node. Retrying…");
                }
                Err(err) => self.lose(&err.to_string()),
            }
            tokio::select! {
                () = tokio::time::sleep(retry.next_wait()) => {}
                command = commands.recv() => match command {
                    Some(command) => self.handle(command).await,
                    None => return,
                },
            }
        }
    }

    async fn handle(&self, command: Command) {
        let outcome = match command {
            Command::Send(prompt) => {
                if self.view.read().busy() || !self.queue.read().is_empty() {
                    self.queue.write().push_back(prompt);
                    self.changed();
                } else {
                    self.send(prompt).await;
                }
                Ok(())
            }
            Command::Answer(prompt) => {
                if self.view.read().held_by_question() {
                    self.queue.write().push_front(prompt);
                    self.changed();
                    let stopped = match self.gateway().await {
                        Ok(gateway) => gateway.stop(&self.id).await,
                        Err(err) => Err(err),
                    };
                    self.drain().await;
                    stopped
                } else {
                    self.send(prompt).await;
                    Ok(())
                }
            }
            Command::Steer(prompt) => self.steer(prompt).await,
            Command::Unqueue(prompt) => {
                take(&self.queue, &prompt);
                self.changed();
                Ok(())
            }
            Command::Stop => match self.gateway().await {
                Ok(gateway) => gateway.stop(&self.id).await,
                Err(err) => Err(err),
            },
            Command::Decide { request_id, decision } => match self.gateway().await {
                Ok(gateway) => gateway.decide(&self.id, &request_id, decision).await,
                Err(err) => Err(err),
            },
            Command::Earlier => self.earlier().await,
        };
        if let Err(err) = outcome {
            self.fail(&err.to_string());
        }
    }
}

/// Takes `prompt` out of the queue where it sits now; one already sent is gone.
fn take(queue: &RwLock<VecDeque<Prompt>>, prompt: &Prompt) -> Option<Prompt> {
    let mut queue = queue.write();
    let at = queue.iter().position(|queued| queued == prompt)?;
    queue.remove(at)
}

fn epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX))
}

pub fn spawn(runtime: &Handle, hub: crate::Hub, id: SessionId, node: Option<String>) -> SessionHandle {
    let view = Arc::new(RwLock::new(SessionView::default()));
    let queue = Arc::new(RwLock::new(VecDeque::new()));
    let (revision_tx, revision) = watch::channel(0);
    let (commands, commands_rx) = mpsc::channel(COMMAND_BUFFER);
    let actor = Actor {
        hub,
        node,
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
