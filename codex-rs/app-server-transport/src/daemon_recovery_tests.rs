use super::*;
use pretty_assertions::assert_eq;

#[test]
fn private_thread_configs_roundtrip_without_debug_disclosure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("recovery.json");
    let config = ThreadRecoveryConfig {
        overrides: [("mcp_servers".into(), serde_json::json!({"private":{"http_headers":{"Authorization":"synthetic-only"}}}))].into(),
        extensions: Default::default(),
    };
    let snapshot = RecoverySnapshot {
        loaded: ["thread-a".into(), "thread-b".into()].into(),
        configs: [("thread-a".into(), config)].into(),
        ..Default::default()
    };
    std::fs::write(&path, "[]").unwrap();
    write_snapshot(&path, &snapshot).unwrap();
    assert_eq!(read_snapshot(&path).unwrap(), snapshot);
    assert!(!format!("{snapshot:?}").contains("synthetic-only"));
    #[cfg(windows)]
    {
        let output = std::process::Command::new("powershell.exe")
            .env_remove("PSModulePath")
            .args(["-NoProfile", "-NonInteractive", "-Command", r#"
$ErrorActionPreference = 'Stop'
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$acl = Get-Acl -LiteralPath $env:CODEX_TEST_PRIVATE_HANDOFF
$rules = @($acl.Access)
if ($rules.Count -ne 1) { throw 'unexpected recovery file access rules' }
if ($rules[0].IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value -ne $sid) { throw 'recovery file accessible by another principal' }
if ($rules[0].AccessControlType -ne 'Allow') { throw 'owner cannot read recovery file' }
"#])
            .env("CODEX_TEST_PRIVATE_HANDOFF", &path)
            .output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
    }
}

#[test]
fn configs_cannot_revive_threads_absent_from_the_candidate_set() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("recovery.json");
    write_snapshot(&path, &RecoverySnapshot {
        configs: [("stale-thread".into(), ThreadRecoveryConfig::default())].into(),
        ..Default::default()
    }).unwrap();
    assert_eq!(read_snapshot(&path).unwrap(), RecoverySnapshot::default());
}

#[test]
fn corrupt_private_metadata_is_rejected_without_disclosing_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("recovery.json");
    std::fs::write(&path, serde_json::to_vec(&[
        "thread-a", "codex-interrupted-v1:{synthetic-private-value",
    ]).unwrap()).unwrap();
    let error = read_snapshot(&path).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(!error.to_string().contains("synthetic-private-value"));
}
