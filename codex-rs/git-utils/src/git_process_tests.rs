use super::run_git_command_with_timeout_output_blocking;
use super::spawn_git_command;
use super::wait_for_git_command_with_timeout_output;
use pretty_assertions::assert_eq;
#[cfg(windows)]
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

#[derive(Clone, Copy)]
enum GitWrapperLifetime {
    WaitForChild,
    ExitBeforeTimeout,
}

async fn assert_timed_out_git_wrapper_does_not_leave_child_process_running(
    wrapper_lifetime: GitWrapperLifetime,
) {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let child_pid_file = temp_dir.path().join("child.pid");
    let child_ready_file = temp_dir.path().join("child-ready");
    let release_child_file = temp_dir.path().join("release-child");
    let child_survived_file = temp_dir.path().join("child-survived");
    let release_wrapper_file = temp_dir.path().join("release-wrapper");
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("/bin/sh");
        let wrapper_command = match wrapper_lifetime {
            GitWrapperLifetime::WaitForChild => {
                r#"( : > "$CHILD_READY_FILE"; while [ ! -f "$RELEASE_CHILD_FILE" ]; do sleep 0.01; done; sleep 1; : > "$CHILD_SURVIVED_FILE"; sleep 60 ) & child_pid=$!; printf '%s\n' "$child_pid" > "$CHILD_PID_FILE"; wait "$child_pid""#
            }
            GitWrapperLifetime::ExitBeforeTimeout => {
                r#"( : > "$CHILD_READY_FILE"; while [ ! -f "$RELEASE_CHILD_FILE" ]; do sleep 0.01; done; sleep 1; : > "$CHILD_SURVIVED_FILE"; sleep 60 ) & child_pid=$!; printf '%s\n' "$child_pid" > "$CHILD_PID_FILE"; while [ ! -f "$RELEASE_WRAPPER_FILE" ]; do sleep 0.01; done"#
            }
        };
        command.args(["-c", wrapper_command]);
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("powershell.exe");
        let child_command = "Set-Content -LiteralPath $env:CHILD_READY_FILE -Value ready; while (-not (Test-Path $env:RELEASE_CHILD_FILE)) { Start-Sleep -Milliseconds 25 }; Start-Sleep -Seconds 1; Set-Content -LiteralPath $env:CHILD_SURVIVED_FILE -Value survived; Start-Sleep -Seconds 60";
        let wrapper_command = match wrapper_lifetime {
            GitWrapperLifetime::WaitForChild => format!(
                "$child = Start-Process -FilePath powershell.exe -ArgumentList @('-NoProfile', '-NonInteractive', '-Command', '{child_command}') -PassThru -NoNewWindow; [System.IO.File]::WriteAllText($env:CHILD_PID_FILE, [string]$child.Id); Wait-Process -Id $child.Id"
            ),
            GitWrapperLifetime::ExitBeforeTimeout => format!(
                "$child = Start-Process -FilePath powershell.exe -ArgumentList @('-NoProfile', '-NonInteractive', '-Command', '{child_command}') -PassThru -NoNewWindow; [System.IO.File]::WriteAllText($env:CHILD_PID_FILE, [string]$child.Id); while (-not (Test-Path $env:RELEASE_WRAPPER_FILE)) {{ Start-Sleep -Milliseconds 25 }}"
            ),
        };
        command
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(wrapper_command);
        command
    };
    command
        .env("CHILD_PID_FILE", &child_pid_file)
        .env("CHILD_READY_FILE", &child_ready_file)
        .env("RELEASE_CHILD_FILE", &release_child_file)
        .env("CHILD_SURVIVED_FILE", &child_survived_file)
        .env("RELEASE_WRAPPER_FILE", &release_wrapper_file);

    let (mut wrapper, process_tree) = spawn_git_command(&mut command).expect("spawn Git wrapper");
    let child_pid = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(child_pid) = std::fs::read_to_string(&child_pid_file)
                && !child_pid.trim().is_empty()
                && child_ready_file.exists()
            {
                break child_pid.trim().to_string();
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("wait for Git wrapper child readiness");

    if matches!(wrapper_lifetime, GitWrapperLifetime::ExitBeforeTimeout) {
        std::fs::write(&release_wrapper_file, "release").expect("release Git wrapper");
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if wrapper
                    .try_wait()
                    .expect("check Git wrapper state")
                    .is_some()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("wait for Git wrapper exit");
    }

    let output =
        wait_for_git_command_with_timeout_output(wrapper, process_tree, Duration::from_millis(100))
            .await;
    assert_eq!(output, None);

    std::fs::write(&release_child_file, "release").expect("release Git wrapper child");
    tokio::time::sleep(Duration::from_secs(3)).await;
    if !child_survived_file.exists() {
        return;
    }

    #[cfg(unix)]
    let _ = std::process::Command::new("kill")
        .args(["-KILL", &child_pid])
        .status();
    #[cfg(windows)]
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &child_pid, "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    panic!("Git wrapper child process {child_pid} survived timeout cleanup");
}

#[tokio::test]
async fn timed_out_git_wrapper_does_not_leave_child_process_running() {
    assert_timed_out_git_wrapper_does_not_leave_child_process_running(
        GitWrapperLifetime::WaitForChild,
    )
    .await;
}

#[tokio::test]
async fn timed_out_exited_git_wrapper_does_not_leave_child_process_running() {
    assert_timed_out_git_wrapper_does_not_leave_child_process_running(
        GitWrapperLifetime::ExitBeforeTimeout,
    )
    .await;
}

#[tokio::test]
async fn blocking_git_preserves_output_inside_current_thread_runtime() {
    #[cfg(unix)]
    let mut command = std::process::Command::new("/bin/sh");
    #[cfg(unix)]
    command.args(["-c", "printf 'owned-git-output\\n'"]);
    #[cfg(windows)]
    let mut command = codex_utils_process::background_command("cmd.exe");
    #[cfg(windows)]
    command.args(["/C", "echo owned-git-output"]);
    let output =
        run_git_command_with_timeout_output_blocking(command, Duration::from_secs(/*secs*/ 10))
            .expect("blocking Git operation should complete without reentering the caller runtime");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 test output")
            .trim(),
        "owned-git-output"
    );
    assert_eq!(output.stderr, Vec::<u8>::new());
}

#[cfg(windows)]
#[test]
fn blocking_owner_exit_terminates_git_process_tree() {
    const TEST_NAME: &str = "git_process::tests::blocking_owner_exit_terminates_git_process_tree";
    const PHASE: &str = "CODEX_TEST_BLOCKING_GIT_OWNER_PHASE";
    if std::env::var(PHASE).as_deref() == Ok("owner") {
        let ready = std::path::PathBuf::from(
            std::env::var_os("CHILD_READY_FILE").expect("private child readiness path"),
        );
        let (completed, completion) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut command = codex_utils_process::background_command("powershell.exe");
            // Avoid a synthetic PowerShell -Command argument being parsed as Git's -C option.
            command.args(["-NoProfile", "-NonInteractive", "-File"]);
            command.arg(std::env::var_os("CHILD_SCRIPT_FILE").expect("private child script"));
            let output = run_git_command_with_timeout_output_blocking(
                command,
                Duration::from_secs(/*secs*/ 60),
            );
            let _ = completed.send(output);
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(/*secs*/ 30);
        while !ready.exists() {
            if let Ok(output) = completion.try_recv() {
                panic!("owned Git fixture exited before readiness: {output:?}");
            }
            assert!(
                std::time::Instant::now() < deadline,
                "owned Git child did not become ready"
            );
            std::thread::sleep(Duration::from_millis(/*millis*/ 25));
        }
        // Model application exit while a synchronous plugin worker still owns Git.
        std::process::exit(/*code*/ 0);
    }

    let temp = tempfile::tempdir().expect("private process-lifecycle fixture");
    let child_pid = temp.path().join("child.pid");
    let child_ready = temp.path().join("ready");
    let release_child = temp.path().join("release");
    let child_survived = temp.path().join("survived");
    let child_script = temp.path().join("child.ps1");
    std::fs::write(
        &child_script,
        "[System.IO.File]::WriteAllText($env:CHILD_PID_FILE, [string]$PID); [System.IO.File]::WriteAllText($env:CHILD_READY_FILE, 'ready'); while (-not (Test-Path $env:RELEASE_CHILD_FILE)) { Start-Sleep -Milliseconds 25 }; [System.IO.File]::WriteAllText($env:CHILD_SURVIVED_FILE, 'survived'); Start-Sleep -Seconds 60",
    )
    .expect("write private process-lifecycle script");
    let output = codex_utils_process::background_command(
        std::env::current_exe().expect("current test executable"),
    )
    .args(["--exact", TEST_NAME, "--nocapture"])
    .env(PHASE, "owner")
    .env("CHILD_PID_FILE", &child_pid)
    .env("CHILD_READY_FILE", &child_ready)
    .env("RELEASE_CHILD_FILE", &release_child)
    .env("CHILD_SURVIVED_FILE", &child_survived)
    .env("CHILD_SCRIPT_FILE", &child_script)
    .output()
    .expect("run isolated application-owner fixture");
    assert!(output.status.success(), "owner fixture failed: {output:?}");
    assert!(
        child_ready.exists(),
        "the owned child must actually have started"
    );
    std::fs::write(release_child, "release").expect("release only the private owned child");
    std::thread::sleep(Duration::from_secs(/*secs*/ 3));
    if child_survived.exists() {
        let pid = std::fs::read_to_string(child_pid).expect("private owned child PID");
        let _ = codex_utils_process::background_command("taskkill.exe")
            .args(["/PID", pid.trim(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        panic!("owned Git child survived application-owner exit");
    }
}
