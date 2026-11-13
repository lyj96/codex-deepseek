//! Coordinates tests that share the runner's Windows sandbox accounts across processes.

#![cfg(windows)]

use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;

/// Serializes tests that provision or use the runner's shared Windows sandbox accounts.
/// Hold this from before setup until all sandbox commands and cleanup have finished.
#[must_use = "the account lock is released when the guard is dropped"]
pub struct WindowsSandboxAccountTestGuard {
    _file: File,
}

impl WindowsSandboxAccountTestGuard {
    /// Stages the real setup/runner binaries beside this test executable while
    /// holding the shared account lock. Existing resources must match exactly.
    pub fn acquire_with_helpers(helpers: &[PathBuf]) -> io::Result<Self> {
        let required = [
            "codex-command-runner.exe",
            "codex-windows-sandbox-setup.exe",
        ];
        if helpers.len() != required.len()
            || required.iter().any(|name| {
                helpers
                    .iter()
                    .filter(|path| path.file_name().and_then(|file| file.to_str()) == Some(*name))
                    .count()
                    != 1
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected the real Windows sandbox helper pair",
            ));
        }
        let guard = Self::acquire()?;
        let executable = std::env::current_exe()?;
        let resources = executable
            .parent()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "test executable has no parent")
            })?
            .join("codex-resources");
        if let Err(error) = std::fs::create_dir_all(&resources)
            && !(error.kind() == io::ErrorKind::PermissionDenied && resources.is_dir())
        {
            return Err(error);
        }
        for source in helpers {
            let destination = resources.join(source.file_name().expect("validated helper name"));
            if destination.is_file() && std::fs::read(source)? == std::fs::read(&destination)? {
                continue;
            }
            std::fs::copy(source, destination)?;
        }
        Ok(guard)
    }

    /// Acquires the same file lock across all test processes run by this Windows user.
    pub fn acquire() -> io::Result<Self> {
        let directory = dirs::data_local_dir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Windows local application data directory is unavailable",
            )
        })?;
        // Bazel uses per-test temporary directories, so the lock lives in the runner user's
        // Windows known folder instead. It is separate from the production setup lock.
        let path = directory.join("codex-windows-sandbox-integration-test.lock");
        // Leave the file in place: deleting it would let processes lock different files at this path.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("open Windows sandbox test lock: {error}"),
                )
            })?;
        file.lock().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("lock Windows sandbox test accounts: {error}"),
            )
        })?;
        Ok(Self { _file: file })
    }
}
