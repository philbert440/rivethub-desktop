use std::sync::Arc;
use std::time::Duration;

use gateway::{Gateway, TermInput, TermOutput};
use model::SessionId;
use parking_lot::RwLock;
use term::{GridSize, Screen};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};

const FIRST_RETRY: Duration = Duration::from_millis(500);
const LAST_RETRY: Duration = Duration::from_secs(15);
const INPUT_BUFFER: usize = 256;

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

    pub fn resize(&self, size: GridSize) {
        if self.screen.read().size() != size {
            let _ = self.commands.try_send(Command::Resize(size));
        }
    }
}

struct Actor {
    gateway: Gateway,
    session: SessionId,
    screen: Arc<RwLock<Screen>>,
    state: Arc<RwLock<TerminalState>>,
    revision: watch::Sender<u64>,
}

impl Actor {
    fn changed(&self) {
        self.revision.send_modify(|revision| *revision += 1);
    }

    fn set_state(&self, state: TerminalState) {
        *self.state.write() = state;
        self.changed();
    }

    async fn run(self, mut commands: mpsc::Receiver<Command>) {
        let mut retry = FIRST_RETRY;
        loop {
            let size = self.screen.read().size();
            let attached = match self
                .gateway
                .session_terminal(&self.session, size.columns, size.rows)
                .await
            {
                Ok(pty) => self.gateway.attach_terminal(&pty).await,
                Err(err) => Err(err),
            };
            match attached {
                Ok(mut stream) => {
                    retry = FIRST_RETRY;
                    let _ = stream
                        .input
                        .send(TermInput::Claim {
                            columns: size.columns,
                            rows: size.rows,
                        })
                        .await;
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
                                    let _ = stream.input.send(TermInput::Bytes(bytes)).await;
                                }
                                Some(Command::Resize(size)) => {
                                    self.screen.write().resize(size);
                                    let _ = stream.input.send(TermInput::Resize { columns: size.columns, rows: size.rows }).await;
                                    self.changed();
                                }
                                None => return,
                            },
                        }
                    }
                }
                Err(err) => tracing::warn!(%err, session = %self.session, "terminal attach failed"),
            }
            self.set_state(TerminalState::Connecting);
            tokio::select! {
                () = tokio::time::sleep(retry) => {}
                command = commands.recv() => match command {
                    Some(Command::Resize(size)) => self.screen.write().resize(size),
                    Some(Command::Bytes(_)) => {}
                    None => return,
                },
            }
            retry = (retry * 2).min(LAST_RETRY);
        }
    }
}

pub fn spawn(runtime: &Handle, gateway: Gateway, session: SessionId, size: GridSize) -> TerminalHandle {
    let screen = Arc::new(RwLock::new(Screen::new(size)));
    let state = Arc::new(RwLock::new(TerminalState::default()));
    let (revision_tx, revision) = watch::channel(0);
    let (commands, commands_rx) = mpsc::channel(INPUT_BUFFER);
    let actor = Actor {
        gateway,
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
