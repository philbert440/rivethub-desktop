//! One app per login: a second launch asks the running one to raise its window, then exits.
//!
//! The running instance holds an advisory lock on `$XDG_RUNTIME_DIR/rivethub-desktop.lock`, which the
//! kernel drops if it dies, and listens on `rivethub-desktop.sock` beside it. A later launch that finds
//! the lock taken connects to the socket; the connection alone is the request.

use std::fs::{File, TryLockError};
use std::io::ErrorKind;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::mpsc;

const NAME: &str = "rivethub-desktop";
/// How long a second launch waits for a just-started first one to begin listening.
const CONNECT_TRIES: u32 = 20;
const CONNECT_PAUSE: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub enum Claim {
    /// This is the only instance.
    First(Owner),
    /// Another instance is running and has been asked to come forward.
    Running,
    /// There is nowhere to coordinate, so this instance runs on its own.
    Alone,
}

/// The running instance's hold on the lock and socket; dropping it removes the socket.
#[derive(Debug)]
pub struct Owner {
    _lock: File,
    listener: UnixListener,
    socket: PathBuf,
}

pub fn claim() -> Claim {
    let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
        tracing::warn!("XDG_RUNTIME_DIR is unset, so another launch can't find this window");
        return Claim::Alone;
    };
    let socket = dir.join(format!("{NAME}.sock"));
    let lock = match File::create(dir.join(format!("{NAME}.lock"))) {
        Ok(lock) => lock,
        Err(err) => {
            tracing::warn!(%err, "creating the instance lock failed");
            return Claim::Alone;
        }
    };
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return ask_running(&socket),
        Err(TryLockError::Error(err)) => {
            tracing::warn!(%err, "taking the instance lock failed");
            return Claim::Alone;
        }
    }
    if let Err(err) = std::fs::remove_file(&socket)
        && err.kind() != ErrorKind::NotFound
    {
        tracing::warn!(%err, "removing the last run's socket failed");
    }
    match UnixListener::bind(&socket) {
        Ok(listener) => Claim::First(Owner {
            _lock: lock,
            listener,
            socket,
        }),
        Err(err) => {
            tracing::warn!(%err, "listening for later launches failed");
            Claim::Alone
        }
    }
}

fn ask_running(socket: &Path) -> Claim {
    for _ in 0..CONNECT_TRIES {
        if UnixStream::connect(socket).is_ok() {
            return Claim::Running;
        }
        std::thread::sleep(CONNECT_PAUSE);
    }
    tracing::warn!("another instance holds the lock but isn't answering, so this one runs on its own");
    Claim::Alone
}

impl Owner {
    /// Yields once for every later launch.
    pub fn launches(&self) -> std::io::Result<mpsc::UnboundedReceiver<()>> {
        let listener = self.listener.try_clone()?;
        let (signal, launches) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("instance".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if let Err(err) = stream {
                        tracing::warn!(%err, "listening for later launches stopped");
                        break;
                    }
                    if signal.send(()).is_err() {
                        break;
                    }
                }
            })?;
        Ok(launches)
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        std::fs::remove_file(&self.socket).ok();
    }
}
