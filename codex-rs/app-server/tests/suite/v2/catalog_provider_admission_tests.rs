//! Configured catalog routes remain usable without changing the global default provider.

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_final_assistant_message_sse_response;
use app_test_support::create_mock_responses_server_sequence_unchecked;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadSettingsUpdateResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;
use test_case::test_case;
use tokio::time::timeout;
use wiremock::MockServer;

const WAIT: Duration = Duration::from_secs(/*secs*/ 10);

fn write_catalog_config(home: &Path, default_server: &str, external_server: &str) -> Result<()> {
    MockResponsesConfig::new(default_server)
        .with_extra_config(&format!(
            r#"
[model_providers.qwen]
name = "Qwen"
base_url = "{external_server}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
supports_websockets = false
"#
        ))
        .write(home)?;
    let directory = home.join("model-catalogs");
    std::fs::create_dir(&directory)?;
    let mut catalog = codex_models_manager::bundled_models_response()?;
    catalog.models.truncate(/*len*/ 2);
    catalog.models[0].slug = "qwen-api".into();
    catalog.models[1].slug = "qwen-flash-api".into();
    std::fs::write(directory.join("qwen.json"), serde_json::to_vec(&catalog)?)?;
    std::fs::write(
        directory.join("routes.json"),
        serde_json::to_vec(&json!({
            "version": 1,
            "models": {
                "qwen-public": {"provider": "qwen", "api_model": "qwen-api"},
                "qwen-flash-public": {"provider": "qwen", "api_model": "qwen-flash-api"}
            }
        }))?,
    )?;
    Ok(())
}

async fn finish_turn(server: &mut TestAppServer, mut params: TurnStartParams) -> Result<()> {
    params.input = vec![UserInput::Text {
        text: "Continue on this provider.".into(),
        text_elements: Vec::new(),
    }];
    let id = server.send_turn_start_request(params).await?;
    let response: TurnStartResponse = timeout(WAIT, server.read_response(id)).await??;
    assert!(!response.turn.id.is_empty());
    timeout(
        WAIT,
        server.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    Ok(())
}

async fn inference_models(server: &MockServer) -> Result<Vec<String>> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|request| request.method == "POST" && request.url.path().ends_with("/responses"))
        .map(|request| {
            Ok(serde_json::from_slice::<Value>(&request.body)?["model"]
                .as_str()
                .expect("inference request includes model")
                .to_owned())
        })
        .collect::<Result<Vec<_>>>()
}

#[tokio::test]
async fn four_model_entries_keep_catalog_provider_while_default_is_different() -> Result<()> {
    let unused_default = responses::start_mock_server().await;
    let external = create_mock_responses_server_sequence_unchecked(vec![
        create_final_assistant_message_sse_response("one")?,
        create_final_assistant_message_sse_response("two")?,
        create_final_assistant_message_sse_response("three")?,
        create_final_assistant_message_sse_response("four")?,
    ])
    .await;
    let home = TempDir::new()?;
    write_catalog_config(home.path(), &unused_default.uri(), &external.uri())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let started = server
        .start_thread(ThreadStartParams {
            model: Some("qwen-public".into()),
            ..Default::default()
        })
        .await?;
    assert_eq!(started.model_provider, "qwen");
    assert_eq!(started.model, "qwen-api");
    let thread_id = started.thread.id;
    finish_turn(
        &mut server,
        TurnStartParams {
            thread_id: thread_id.clone(),
            model: Some("qwen-flash-public".into()),
            ..Default::default()
        },
    )
    .await?;
    let _: ThreadSettingsUpdateResponse = server
        .request(|request_id| ClientRequest::ThreadSettingsUpdate {
            request_id,
            params: ThreadSettingsUpdateParams {
                thread_id: thread_id.clone(),
                model: Some("qwen-public".into()),
                ..Default::default()
            },
        })
        .await?;
    finish_turn(
        &mut server,
        TurnStartParams {
            thread_id: thread_id.clone(),
            ..Default::default()
        },
    )
    .await?;
    finish_turn(
        &mut server,
        TurnStartParams {
            thread_id: thread_id.clone(),
            collaboration_mode: Some(CollaborationMode {
                mode: ModeKind::Default,
                settings: Settings {
                    model: "qwen-flash-public".into(),
                    reasoning_effort: None,
                    developer_instructions: None,
                },
            }),
            ..Default::default()
        },
    )
    .await?;
    finish_turn(
        &mut server,
        TurnStartParams {
            thread_id,
            model: Some("future-provider-api".into()),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        inference_models(&external).await?,
        vec![
            "qwen-flash-api",
            "qwen-api",
            "qwen-flash-api",
            "future-provider-api"
        ]
    );
    assert!(inference_models(&unused_default).await?.is_empty());
    assert!(
        timeout(WAIT, server.shutdown_gracefully())
            .await??
            .success()
    );
    Ok(())
}

#[test_case(ThreadHistoryMode::Legacy, "qwen-api"; "legacy_catalog_api")]
#[test_case(ThreadHistoryMode::Paginated, "qwen-api"; "paginated_catalog_api")]
#[test_case(ThreadHistoryMode::Legacy, "future-provider-api"; "legacy_unknown_api")]
#[test_case(ThreadHistoryMode::Paginated, "future-provider-api"; "paginated_unknown_api")]
#[tokio::test]
async fn cold_catalog_resume_retains_provider_and_api_model(
    history_mode: ThreadHistoryMode,
    model: &str,
) -> Result<()> {
    let unused_default = responses::start_mock_server().await;
    let external = create_mock_responses_server_sequence_unchecked(vec![
        create_final_assistant_message_sse_response("before restart")?,
        create_final_assistant_message_sse_response("after restart")?,
    ])
    .await;
    let home = TempDir::new()?;
    write_catalog_config(home.path(), &unused_default.uri(), &external.uri())?;
    let mut first = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let started = first
        .start_thread(ThreadStartParams {
            model: Some("qwen-public".into()),
            history_mode: Some(history_mode),
            ..Default::default()
        })
        .await?;
    let thread_id = started.thread.id;
    finish_turn(
        &mut first,
        TurnStartParams {
            thread_id: thread_id.clone(),
            model: Some(model.into()),
            ..Default::default()
        },
    )
    .await?;
    assert!(timeout(WAIT, first.shutdown_gracefully()).await??.success());
    let mut second = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let resumed: ThreadResumeResponse = second
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: thread_id.clone(),
                ..Default::default()
            },
        })
        .await?;
    assert_eq!(resumed.model_provider, "qwen");
    assert_eq!(resumed.model, model);
    finish_turn(
        &mut second,
        TurnStartParams {
            thread_id,
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(inference_models(&external).await?, vec![model, model]);
    assert!(inference_models(&unused_default).await?.is_empty());
    assert!(
        timeout(WAIT, second.shutdown_gracefully())
            .await??
            .success()
    );
    Ok(())
}

#[test_case(ThreadHistoryMode::Legacy, false; "loaded_legacy")]
#[test_case(ThreadHistoryMode::Paginated, false; "loaded_paginated")]
#[test_case(ThreadHistoryMode::Legacy, true; "cold_legacy")]
#[test_case(ThreadHistoryMode::Paginated, true; "cold_paginated")]
#[tokio::test]
async fn resume_public_alias_cannot_move_history_to_another_provider(
    history_mode: ThreadHistoryMode,
    cold: bool,
) -> Result<()> {
    let unused_default = responses::start_mock_server().await;
    let other_provider = responses::start_mock_server().await;
    let external = create_mock_responses_server_sequence_unchecked(vec![
        create_final_assistant_message_sse_response("saved history")?,
        create_final_assistant_message_sse_response("same provider continues")?,
    ])
    .await;
    let home = TempDir::new()?;
    write_catalog_config(home.path(), &unused_default.uri(), &external.uri())?;
    let config_path = home.path().join("config.toml");
    let mut config = std::fs::read_to_string(&config_path)?;
    config.push_str(&format!(
        r#"
[model_providers.deepseek]
name = "DeepSeek"
base_url = "{}/v1"
wire_api = "responses"
supports_websockets = false
"#,
        other_provider.uri()
    ));
    std::fs::write(config_path, config)?;
    let directory = home.path().join("model-catalogs");
    let mut catalog = codex_models_manager::bundled_models_response()?;
    catalog.models.truncate(/*len*/ 1);
    catalog.models[0].slug = "deepseek-api".into();
    std::fs::write(
        directory.join("deepseek.json"),
        serde_json::to_vec(&catalog)?,
    )?;
    let routes_path = directory.join("routes.json");
    let mut routes: Value = serde_json::from_slice(&std::fs::read(&routes_path)?)?;
    routes["models"]["deepseek-public"] =
        json!({"provider": "deepseek", "api_model": "deepseek-api"});
    std::fs::write(routes_path, serde_json::to_vec(&routes)?)?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let started = server
        .start_thread(ThreadStartParams {
            model: Some("qwen-public".into()),
            history_mode: Some(history_mode),
            ..Default::default()
        })
        .await?;
    let thread_id = started.thread.id;
    finish_turn(
        &mut server,
        TurnStartParams {
            thread_id: thread_id.clone(),
            ..Default::default()
        },
    )
    .await?;
    if cold {
        assert!(
            timeout(WAIT, server.shutdown_gracefully())
                .await??
                .success()
        );
        server = TestAppServer::builder()
            .with_codex_home(home.path())
            .build_initialized()
            .await?;
    }
    let id = server
        .send_raw_request(
            "thread/resume",
            Some(json!({
                "threadId": thread_id, "model": "deepseek-public"
            })),
        )
        .await?;
    if cold {
        let error: JSONRPCError = timeout(
            WAIT,
            server.read_stream_until_error_message(RequestId::Integer(id)),
        )
        .await??;
        assert_eq!(error.error.code, -32600);
        assert_eq!(
            error.error.data,
            Some(json!({
                "type": "modelProviderMismatch", "modelProvider": "qwen", "currentModelProvider": "deepseek"
            }))
        );
    } else {
        // Native rejoin ignores overrides while a loaded thread still has subscribers.
        // Its response must retain the original route, never migrate its history.
        let rejoined: ThreadResumeResponse = timeout(WAIT, server.read_response(id)).await??;
        assert_eq!(rejoined.model_provider, "qwen");
        assert_eq!(rejoined.model, "qwen-api");
    }
    let resumed: ThreadResumeResponse = server
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: thread_id.clone(),
                ..Default::default()
            },
        })
        .await?;
    assert_eq!(resumed.model_provider, "qwen");
    finish_turn(
        &mut server,
        TurnStartParams {
            thread_id: thread_id.clone(),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        inference_models(&external).await?,
        vec!["qwen-api", "qwen-api"]
    );
    let fresh = server
        .start_thread(ThreadStartParams {
            model: Some("deepseek-public".into()),
            history_mode: Some(history_mode),
            ..Default::default()
        })
        .await?;
    assert_eq!(fresh.model_provider, "deepseek");
    assert_ne!(fresh.thread.id, thread_id);
    assert!(fresh.thread.turns.is_empty());
    assert!(inference_models(&other_provider).await?.is_empty());
    assert!(inference_models(&unused_default).await?.is_empty());
    assert!(
        timeout(WAIT, server.shutdown_gracefully())
            .await??
            .success()
    );
    Ok(())
}
