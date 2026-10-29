//! Shared on-disk candidate set for managed daemon restarts.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::Path;

#[cfg(not(windows))]
use codex_core::path_utils::write_atomically;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct RecoverySnapshot {
    #[serde(skip)]
    pub loaded: BTreeSet<String>,
    pub interrupted: BTreeMap<String, InterruptedTurn>,
    /// Private, one-generation handoff; never exposed through protocol responses or logs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub configs: BTreeMap<String, ThreadRecoveryConfig>,
}

#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ThreadRecoveryConfig {
    pub overrides: HashMap<String, serde_json::Value>,
    pub extensions: HashMap<String, serde_json::Value>,
}

impl fmt::Debug for ThreadRecoveryConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A config override may contain a credential or private MCP endpoint.
        formatter.write_str("ThreadRecoveryConfig([REDACTED])")
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InterruptedTurn {
    pub turn_id: String,
    pub output_schema: Option<serde_json::Value>,
    pub service_tier: Option<String>,
    pub cyber_access_program: Option<codex_protocol::turn_input::CyberAccessProgram>,
    /// Only local execution with thread-owned configuration can continue automatically.
    /// Older snapshots without this identity are reloaded without continuation.
    pub local_environment: Option<codex_app_server_protocol::ThreadEnvironment>,
}

// Old servers accept the array and skip this non-thread entry during best-effort
// restoration. Keeping metadata in the same atomic file avoids stale sidecars.
const INTERRUPTION_PREFIX: &str = "codex-interrupted-v1:";

pub fn read_snapshot(path: &Path) -> io::Result<RecoverySnapshot> {
    let mut loaded: BTreeSet<String> = match std::fs::read(path) {
        Ok(contents) => serde_json::from_slice(&contents).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid daemon recovery candidate set")
        })?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(RecoverySnapshot::default()),
        Err(err) => return Err(err),
    };
    let mut snapshot = RecoverySnapshot::default();
    for entry in &loaded {
        if let Some(metadata) = entry.strip_prefix(INTERRUPTION_PREFIX) {
            // Never include the malformed payload in an error: it may contain credentials.
            snapshot = serde_json::from_str(metadata).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid daemon recovery metadata")
            })?;
        }
    }
    loaded.retain(|entry| !entry.starts_with(INTERRUPTION_PREFIX));
    snapshot.interrupted.retain(|id, _| loaded.contains(id));
    snapshot.configs.retain(|id, _| loaded.contains(id));
    snapshot.loaded = loaded;
    Ok(snapshot)
}

pub fn read_candidates(path: &Path) -> io::Result<BTreeSet<String>> {
    Ok(read_snapshot(path)?.loaded)
}

pub fn write_candidates(path: &Path, candidates: &BTreeSet<String>) -> io::Result<()> {
    write_snapshot(
        path,
        &RecoverySnapshot {
            loaded: candidates.clone(),
            ..Default::default()
        },
    )
}

pub fn write_snapshot(path: &Path, snapshot: &RecoverySnapshot) -> io::Result<()> {
    let mut saved = snapshot.loaded.clone();
    if !snapshot.interrupted.is_empty() || !snapshot.configs.is_empty() {
        saved.insert(format!(
            "{INTERRUPTION_PREFIX}{}",
            serde_json::to_string(snapshot).map_err(io::Error::other)?
        ));
    }
    let contents = serde_json::to_string(&saved).map_err(io::Error::other)?;
    #[cfg(not(windows))]
    return write_atomically(path, &contents);
    #[cfg(windows)]
    {
        use std::io::Write;

        let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
        let staging = parent.join(".recovery-staging");
        codex_uds::prepare_private_directory(&staging)?;
        let (staged_path, _guard) =
            codex_uds::validate_private_socket_path(&staging.join("snapshot"))?;
        let mut file = tempfile::NamedTempFile::new_in(
            staged_path.parent().ok_or(io::ErrorKind::InvalidInput)?,
        )?;
        file.write_all(contents.as_bytes())?;
        // Same-volume atomic replacement retains the protected directory's user-only DACL.
        file.persist(path)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "daemon_recovery_tests.rs"]
mod tests;
