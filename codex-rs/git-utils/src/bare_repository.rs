use std::ffi::OsStr;
use std::io;
use std::process::Command;

/// Rejects implicit bare-repository discovery before starting Git, including
/// versions predating `safe.bareRepository`. Explicit command-scoped repository
/// selections remain allowed. Keep the Git configuration too, so modern Git
/// also enforces this policy during its own discovery.
pub fn ensure_safe_repository_discovery(command: &Command) -> io::Result<()> {
    let mut cwd = command
        .get_current_dir()
        .map(std::path::Path::to_path_buf)
        .map_or_else(std::env::current_dir, Ok)?;
    if command.get_envs().any(|(name, value)| {
        name == OsStr::new("GIT_DIR") && value.is_some_and(|value| !value.is_empty())
    }) {
        return Ok(());
    }

    // Only parse global options: a path passed to a subcommand must not bypass
    // the guard. Git permits repeated relative -C options.
    let mut args = command.get_args();
    while let Some(arg) = args.next() {
        if arg == "--git-dir" {
            if args.next().is_some_and(|path| !path.is_empty()) {
                return Ok(());
            }
            return Err(io::Error::other("missing explicit Git directory"));
        }
        if arg.as_encoded_bytes().starts_with(b"--git-dir=") {
            if arg.as_encoded_bytes().len() > b"--git-dir=".len() {
                return Ok(());
            }
            return Err(io::Error::other("missing explicit Git directory"));
        }
        if arg == "-C" {
            let path = args
                .next()
                .ok_or_else(|| io::Error::other("missing Git -C path"))?;
            if !path.is_empty() {
                cwd = cwd.join(path);
            }
        } else if arg.as_encoded_bytes().starts_with(b"-C") {
            let path = arg
                .to_str()
                .and_then(|arg| arg.strip_prefix("-C"))
                .ok_or_else(|| io::Error::other("invalid attached Git -C path"))?;
            if !path.is_empty() {
                cwd = cwd.join(path);
            }
        } else if arg == "-c"
            || arg == "--config-env"
            || arg == "--work-tree"
            || arg == "--namespace"
            || arg == "--super-prefix"
        {
            args.next()
                .ok_or_else(|| io::Error::other("missing Git configuration option"))?;
        } else if !arg.as_encoded_bytes().starts_with(b"-") {
            break;
        }
    }

    // Resolve symlinks before walking ancestors, matching the child's cwd.
    // This discovery only inspects metadata; it does not load repository config
    // or execute a repository-selected hook, fsmonitor, or transport helper.
    let cwd = cwd.canonicalize()?;
    let options = gix::discover::upwards::Options {
        cross_fs: true,
        ..Default::default()
    };
    match gix::discover::upwards_opts(&cwd, options) {
        Ok((repository, _)) => {
            // Tracked commondir/gitdir files can also look like linked worktree
            // metadata. Require the same repository to be reachable through an
            // actual .git boundary, not just a claimed worktree classification.
            let dot_git_options = gix::discover::upwards::Options {
                cross_fs: true,
                dot_git_only: true,
                ..Default::default()
            };
            if !matches!(repository, gix::discover::repository::Path::Repository(_))
                && let Ok((dot_git_repository, _)) =
                    gix::discover::upwards_opts(&cwd, dot_git_options)
                && repository == dot_git_repository
            {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "not a git repository: implicitly discovered bare repository refused",
                ))
            }
        }
        Err(gix::discover::upwards::Error::NoGitRepository { .. }) => Ok(()),
        Err(err) => Err(io::Error::other(format!(
            "unsafe Git repository discovery: {err}"
        ))),
    }
}

#[cfg(test)]
#[path = "bare_repository_tests.rs"]
mod tests;
