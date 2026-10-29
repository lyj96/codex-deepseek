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
    write_snapshot(&path, &snapshot).unwrap();
    assert_eq!(read_snapshot(&path).unwrap(), snapshot);
    assert!(!format!("{snapshot:?}").contains("synthetic-only"));
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
