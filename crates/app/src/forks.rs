//! The forks made here and where each came from, kept between runs so a fork reopened later still
//! shows the conversation it carries.

use std::collections::BTreeMap;

use model::fork::ForkOrigin;
use model::{HarnessId, NativeSessionId, SessionId};
use serde::{Deserialize, Serialize};

use crate::Hub;
use crate::prefs::config_file;

const FILE: &str = "forks.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    harness: String,
    native: String,
    node: Option<String>,
    title: Option<String>,
    prompts: u32,
    #[serde(default)]
    from: Option<u64>,
}

impl From<&ForkOrigin> for Stored {
    fn from(origin: &ForkOrigin) -> Self {
        Self {
            harness: origin.source.harness().as_str().to_owned(),
            native: origin.source.native().as_str().to_owned(),
            node: origin.node.clone(),
            title: origin.title.clone(),
            prompts: origin.prompts,
            from: origin.from,
        }
    }
}

impl From<Stored> for ForkOrigin {
    fn from(stored: Stored) -> Self {
        Self {
            source: SessionId::new(
                HarnessId::new(stored.harness),
                NativeSessionId::new(stored.native),
            ),
            node: stored.node,
            title: stored.title,
            prompts: stored.prompts,
            from: stored.from,
        }
    }
}

/// Every fork known here, by the fork's id; an unreadable file reads as none.
fn read() -> BTreeMap<String, Stored> {
    let Some(path) = config_file(FILE) else {
        return BTreeMap::new();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            tracing::warn!(%err, path = %path.display(), "the forks file is unreadable");
            BTreeMap::new()
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(err) => {
            tracing::warn!(%err, path = %path.display(), "reading the forks file failed");
            BTreeMap::new()
        }
    }
}

/// Writes beside the file and renames over it, so a crash mid-write never leaves half a file.
fn write(forks: &BTreeMap<String, Stored>) -> std::io::Result<()> {
    let path =
        config_file(FILE).ok_or_else(|| std::io::Error::other("neither XDG_CONFIG_HOME nor HOME is set"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(forks).map_err(std::io::Error::other)?;
    let staged = path.with_extension("json.tmp");
    std::fs::write(&staged, json)?;
    std::fs::rename(staged, path)
}

impl Hub {
    /// Remembers where a fork made here came from.
    pub fn remember_fork(&self, fork: &SessionId, origin: &ForkOrigin) {
        let _guard = self.inner.forks.lock();
        let mut forks = read();
        forks.insert(fork.to_string(), Stored::from(origin));
        if let Err(err) = write(&forks) {
            tracing::warn!(%err, "the fork's origin couldn't be saved");
        }
    }

    /// Where the fork came from, when it was made here.
    pub fn fork_origin(&self, fork: &SessionId) -> Option<ForkOrigin> {
        let _guard = self.inner.forks.lock();
        read().remove(&fork.to_string()).map(ForkOrigin::from)
    }
}
