use std::sync::Arc;

use gateway::{TermInput, TermOutput};
use model::SessionId;
use parking_lot::RwLock;
use term::{GridSize, Screen};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};

const INPUT_BUFFER: usize = 256;
/// Keys typed while the terminal reconnects wait for it, up to this many bytes.
const HELD_INPUT: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalState {
    #[default]
    Connecting,
    Live,
    Exited,
}

#[derive(Debug)]
enum Command {
    Bytes(Vec<u8>),
    Resize(GridSize),
    End,
}

#[derive(Debug, Clone)]
pub struct TerminalHandle {
    screen: Arc<RwLock<Screen>>,
    state: Arc<RwLock<TerminalState>>,
    revision: watch::Receiver<u64>,
    commands: mpsc::Sender<Command>,
}

impl TerminalHandle {
    pub fn screen(&self) -> Arc<RwLock<Screen>> {
        Arc::clone(&self.screen)
    }

    pub fn state(&self) -> TerminalState {
        *self.state.read()
    }

    pub fn revisions(&self) -> watch::Receiver<u64> {
        self.revision.clone()
    }

    pub fn write(&self, bytes: Vec<u8>) {
        let _ = self.commands.try_send(Command::Bytes(bytes));
    }

    pub fn end(&self) {
        let _ = self.commands.try_send(Command::End);
    }

    pub fn resize(&self, size: GridSize) {
        if self.screen.read().size() != size {
            let _ = self.commands.try_send(Command::Resize(size));
        }
    }
}

struct Actor {
    hub: crate::Hub,
    node: Option<String>,
    session: SessionId,
    screen: Arc<RwLock<Screen>>,
    state: Arc<RwLock<TerminalState>>,
    revision: watch::Sender<u64>,
}

impl Actor {
    async fn attach(&self, size: GridSize) -> Result<gateway::TerminalStream, gateway::GatewayError> {
        let gateway = self.hub.gateway_for(self.node.as_deref()).await?;
        let pty = gateway
            .session_terminal(&self.session, size.columns, size.rows)
            .await?;
        gateway.attach_terminal(&pty).await
    }

    fn changed(&self) {
        self.revision.send_modify(|revision| *revision += 1);
    }

    fn set_state(&self, state: TerminalState) {
        *self.state.write() = state;
        self.changed();
    }

    async fn run(self, mut commands: mpsc::Receiver<Command>) {
        let mut retry = self.hub.backoff(self.node.as_deref());
        // Keys typed while no stream was attached, sent as soon as one is.
        let mut held = Vec::new();
        loop {
            let size = self.screen.read().size();
            let attached = self.attach(size).await;
            match attached {
                Ok(mut stream) => {
                    retry.reset();
                    // The node replays the terminal's output to every attach, so the screen starts
                    // over rather than showing it twice after a reconnect.
                    *self.screen.write() = Screen::new(size);
                    let _ = stream
                        .input
                        .send(TermInput::Claim {
                            columns: size.columns,
                            rows: size.rows,
                        })
                        .await;
                    if !held.is_empty() {
                        let _ = stream
                            .input
                            .send(TermInput::Bytes(std::mem::take(&mut held)))
                            .await;
                    }
                    self.set_state(TerminalState::Live);
                    loop {
                        tokio::select! {
                            output = stream.output.recv() => match output {
                                Some(TermOutput::Bytes(bytes)) => {
                                    let replies = self.screen.write().feed(&bytes);
                                    if !replies.is_empty() {
                                        let _ = stream.input.send(TermInput::Bytes(replies)).await;
                                    }
                                    self.changed();
                                }
                                Some(TermOutput::Exited { .. }) => {
                                    self.set_state(TerminalState::Exited);
                                    return;
                                }
                                Some(TermOutput::Started { .. } | TermOutput::Owner { .. }) => {}
                                Some(TermOutput::Closed) | None => break,
                            },
                            command = commands.recv() => match command {
                                Some(Command::Bytes(bytes)) => {
                                    if let Err(unsent) = stream.input.send(TermInput::Bytes(bytes)).await
                                        && let TermInput::Bytes(bytes) = unsent.0
                                    {
                                        hold(&mut held, &bytes);
                                    }
                                }
                                Some(Command::Resize(size)) => {
                                    self.screen.write().resize(size);
                                    let _ = stream.input.send(TermInput::Resize { columns: size.columns, rows: size.rows }).await;
                                    self.changed();
                                }
                                Some(Command::End) => {
                                    let _ = stream.input.send(TermInput::Kill).await;
                                }
                                None => return,
                            },
                        }
                    }
                }
                Err(err) => tracing::warn!(%err, session = %self.session, "terminal attach failed"),
            }
            self.set_state(TerminalState::Connecting);
            let wait = tokio::time::sleep(retry.next_wait());
            tokio::pin!(wait);
            loop {
                tokio::select! {
                    () = &mut wait => break,
                    command = commands.recv() => match command {
                        Some(Command::Bytes(bytes)) => hold(&mut held, &bytes),
                        Some(Command::Resize(size)) => {
                            self.screen.write().resize(size);
                            break;
                        }
                        Some(Command::End) => break,
                        None => return,
                    },
                }
            }
        }
    }
}

fn hold(held: &mut Vec<u8>, bytes: &[u8]) {
    let room = HELD_INPUT.saturating_sub(held.len());
    held.extend_from_slice(&bytes[..bytes.len().min(room)]);
}

pub fn spawn(
    runtime: &Handle,
    hub: crate::Hub,
    session: SessionId,
    node: Option<String>,
    size: GridSize,
) -> TerminalHandle {
    let screen = Arc::new(RwLock::new(Screen::new(size)));
    let state = Arc::new(RwLock::new(TerminalState::default()));
    let (revision_tx, revision) = watch::channel(0);
    let (commands, commands_rx) = mpsc::channel(INPUT_BUFFER);
    let actor = Actor {
        hub,
        node,
        session,
        screen: Arc::clone(&screen),
        state: Arc::clone(&state),
        revision: revision_tx,
    };
    runtime.spawn(actor.run(commands_rx));
    TerminalHandle {
        screen,
        state,
        revision,
        commands,
    }
}
