use super::ensure_safe_repository_discovery;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::process::Command;

fn embedded_repository(path: &Path) {
    std::fs::create_dir_all(path.join("objects")).unwrap();
    std::fs::create_dir_all(path.join("refs")).unwrap();
    std::fs::write(path.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    // core.bare=false must not let tracked metadata masquerade as a worktree.
    std::fs::write(path.join("config"), "[core]\n bare = false\n worktree = .\n").unwrap();
}

#[test]
fn rejects_embedded_repository_and_descendants() {
    let temp = tempfile::tempdir().unwrap();
    embedded_repository(temp.path());
    let descendant = temp.path().join("child/grandchild");
    std::fs::create_dir_all(&descendant).unwrap();
    for cwd in [temp.path(), descendant.as_path()] {
        let mut command = Command::new("git");
        command.current_dir(cwd).args(["status", "--short"]);
        assert_eq!(
            ensure_safe_repository_discovery(&command).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied,
        );
    }
}

#[test]
fn follows_repeated_relative_git_c_options() {
    let temp = tempfile::tempdir().unwrap();
    let nested = temp.path().join("nested");
    embedded_repository(&nested);
    std::fs::create_dir(nested.join("child")).unwrap();
    let mut command = Command::new("git");
    command.current_dir(temp.path()).args([
        "-c", "safe.bareRepository=explicit", "-C", "nested", "-C", "child", "status",
    ]);
    assert!(ensure_safe_repository_discovery(&command).is_err());
    let mut attached = Command::new("git");
    attached.current_dir(temp.path()).args(["--work-tree", ".", "-Cnested", "-Cchild", "status"]);
    assert!(ensure_safe_repository_discovery(&attached).is_err());
}

#[test]
fn explicit_repository_selection_remains_allowed() {
    let temp = tempfile::tempdir().unwrap();
    embedded_repository(temp.path());
    let mut by_arg = Command::new("git");
    by_arg.current_dir(temp.path()).arg("--git-dir").arg(temp.path()).arg("status");
    ensure_safe_repository_discovery(&by_arg).unwrap();
    let mut by_equals = Command::new("git");
    by_equals.current_dir(temp.path()).arg(format!("--git-dir={}", temp.path().display())).arg("status");
    ensure_safe_repository_discovery(&by_equals).unwrap();
    let mut by_env = Command::new("git");
    by_env.current_dir(temp.path()).env("GIT_DIR", temp.path()).arg("status");
    ensure_safe_repository_discovery(&by_env).unwrap();
}

#[test]
fn removed_empty_or_positional_repository_selection_does_not_bypass_guard() {
    let temp = tempfile::tempdir().unwrap();
    embedded_repository(temp.path());
    for args in [vec!["--git-dir=", "status"], vec!["status", "--", "--git-dir=."]] {
        let mut command = Command::new("git");
        command.current_dir(temp.path()).env_remove("GIT_DIR").args(args);
        assert!(ensure_safe_repository_discovery(&command).is_err());
    }
    let mut command = Command::new("git");
    command.current_dir(temp.path()).env("GIT_DIR", "").arg("status");
    assert!(ensure_safe_repository_discovery(&command).is_err());
}

#[test]
fn ordinary_and_linked_worktrees_remain_allowed() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let output = Command::new("git").current_dir(&repo).args(["init", "--quiet"]).output().unwrap();
    assert!(output.status.success());
    let output = Command::new("git").current_dir(&repo).args([
        "-c", "user.name=Codex Tests", "-c", "user.email=codex-tests@example.com",
        "commit", "--allow-empty", "--quiet", "-m", "fixture",
    ]).output().unwrap();
    assert!(output.status.success());
    let linked = temp.path().join("linked");
    let output = Command::new("git").current_dir(&repo).args(["worktree", "add", "--quiet", "--detach"]).arg(&linked).output().unwrap();
    assert!(output.status.success());
    for root in [&repo, &linked] {
        let child = root.join("child");
        std::fs::create_dir(&child).unwrap();
        let mut command = Command::new("git");
        command.current_dir(&child).arg("status");
        ensure_safe_repository_discovery(&command).unwrap();
    }
}

#[test]
fn non_repository_commands_remain_allowed_but_missing_cwd_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let mut command = Command::new("git");
    command.current_dir(temp.path()).arg("init");
    ensure_safe_repository_discovery(&command).unwrap();
    command.current_dir(temp.path().join("missing"));
    assert!(ensure_safe_repository_discovery(&command).is_err());
}

#[test]
fn tracked_linked_worktree_metadata_cannot_bypass_guard() {
    let temp = tempfile::tempdir().unwrap();
    embedded_repository(&temp.path().join(".git"));
    let nested = temp.path().join("nested");
    embedded_repository(&nested);
    std::fs::write(nested.join("commondir"), temp.path().join(".git").display().to_string()).unwrap();
    std::fs::write(nested.join("gitdir"), temp.path().join(".git").display().to_string()).unwrap();
    let (discovered, _) = gix::discover::upwards(&nested).unwrap();
    assert!(matches!(discovered, gix::discover::repository::Path::LinkedWorkTree { .. }));
    let mut command = Command::new("git");
    command.current_dir(&nested).arg("status");
    assert_eq!(
        ensure_safe_repository_discovery(&command).unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied,
    );
}

#[cfg(unix)]
#[test]
fn symlinked_cwd_uses_physical_ancestors() {
    let temp = tempfile::tempdir().unwrap();
    let nested = temp.path().join("nested");
    embedded_repository(&nested);
    std::fs::create_dir(nested.join("child")).unwrap();
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(nested.join("child"), &alias).unwrap();
    let mut command = Command::new("git");
    command.current_dir(&alias).arg("status");
    assert!(ensure_safe_repository_discovery(&command).is_err());
}
