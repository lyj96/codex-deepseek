use super::*;
use codex_app_server_protocol::ListMcpServerStatusResponse;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;

#[tokio::test]
async fn managed_restart_preserves_distinct_thread_mcp_overrides_and_tools() -> Result<()> {
    let home = TempDir::new()?;
    let mock = MockServer::start().await;
    let restarted = Arc::new(AtomicBool::new(false));
    let mock_restarted = Arc::clone(&restarted);
    Mock::given(method("POST"))
        .respond_with(move |request: &wiremock::Request| {
            let name = request.url.path().trim_matches('/');
            let generation = if mock_restarted.load(Ordering::SeqCst) { "after" } else { "before" };
            if request.headers.get("x-recovery-thread").and_then(|value| value.to_str().ok()) != Some(name) {
                return ResponseTemplate::new(403);
            }
            let input: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let result = match input["method"].as_str() {
                Some("initialize") => json!({
                    "protocolVersion":"2025-11-25", "capabilities":{"tools":{}},
                    "serverInfo":{"name":name,"version":"1.0.0"}
                }),
                Some("tools/list") => json!({"tools":[{
                    "name":format!("{name}_probe_{generation}"),"description":"Recovery probe",
                    "inputSchema":{"type":"object","properties":{}}
                }]}),
                Some("resources/list") => json!({"resources":[]}),
                Some("resources/templates/list") => json!({"resourceTemplates":[]}),
                _ => return ResponseTemplate::new(202),
            };
            ResponseTemplate::new(200).set_body_json(json!({"jsonrpc":"2.0","id":input["id"],"result":result}))
        })
        .mount(&mock)
        .await;
    create_config_toml(home.path(), &mock.uri(), "never")?;
    let socket = home.path().join("control/server.sock");
    let mut server = spawn_server(home.path(), &socket)?;
    let mut client = connect_default_daemon_client(&socket).await?;
    let mut threads = Vec::new();
    for (index, name) in ["alpha", "beta"].into_iter().enumerate() {
        let servers = std::collections::HashMap::from([(
            name.to_owned(), json!({"url":format!("{}/{name}",mock.uri()), "required":true,
                "http_headers":{"X-Recovery-Thread":name}}),
        )]);
        let thread = start_thread(&mut client, index as i64 + 10, json!({
            "config":{"mcp_servers":servers}
        })).await?;
        threads.push((name, thread.thread.id));
    }
    for (index, (name, id)) in threads.iter().enumerate() {
        assert_thread_tools(&mut client, index as i64 + 20, name, id, "before").await?;
    }
    request_shutdown(&server, &socket).await?;
    wait_success(&mut server).await?;
    let recovery_file = daemon_recovery_file_path(home.path());
    let handoff = daemon_recovery::read_snapshot(&recovery_file)?;
    assert_eq!(handoff.configs.len(), 2);
    let previous_requests = mock.received_requests().await.unwrap().len();
    restarted.store(true, Ordering::SeqCst);
    drop(client);
    let mut successor = spawn_server(home.path(), &socket)?;
    let mut client = connect_default_daemon_client(&socket).await?;
    timeout(DEFAULT_READ_TIMEOUT, async {
        loop {
            let loaded = request(&mut client, /*id*/ 30, "thread/loaded/list", json!({})).await?;
            if threads.iter().all(|(_, id)| loaded["data"].as_array().is_some_and(|items| items.contains(&json!(id)))) {
                return Ok::<(), anyhow::Error>(());
            }
            sleep(Duration::from_millis(25)).await;
        }
    }).await??;
    for (index, (name, id)) in threads.iter().enumerate() {
        assert_thread_tools(&mut client, index as i64 + 40, name, id, "after").await?;
    }
    assert!(!recovery_file.exists(), "handoff is consumed by the successor");
    for name in ["alpha", "beta"] {
        let requests = mock.received_requests().await.unwrap();
        assert!(requests[previous_requests..].iter().any(|request| {
            request.method.as_str() == "POST"
                && request.url.path() == format!("/{name}")
                && serde_json::from_slice::<serde_json::Value>(&request.body).unwrap()["method"] == "initialize"
        }), "each thread must initialize its own server in the successor generation");
    }
    request_shutdown(&successor, &socket).await?;
    wait_success(&mut successor).await?;
    Ok(())
}

async fn assert_thread_tools(
    client: &mut WebSocketStream<UnixStream>,
    request_id: i64,
    name: &str,
    thread_id: &str,
    generation: &str,
) -> Result<()> {
    timeout(DEFAULT_READ_TIMEOUT, async {
        loop {
            let value = request(client, request_id, "mcpServerStatus/list", json!({"threadId":thread_id})).await?;
            let response: ListMcpServerStatusResponse = serde_json::from_value(value)?;
            if response.data.len() == 1
                && response.data[0].tools.contains_key(&format!("{name}_probe_{generation}"))
            {
                assert_eq!(response.data[0].name, name);
                assert_eq!(response.data[0].tools.len(), 1);
                assert_eq!(response.next_cursor, None);
                return Ok::<(), anyhow::Error>(());
            }
            sleep(Duration::from_millis(25)).await;
        }
    }).await??;
    Ok(())
}
