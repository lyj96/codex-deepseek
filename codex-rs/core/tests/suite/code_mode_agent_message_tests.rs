//! Exec-spawned tasks reach the child model as attributed plaintext.

use anyhow::Result;
use codex_features::Feature;
use codex_protocol::openai_models::ToolMode;
use codex_protocol::protocol::EventMsg;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_custom_tool_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::responses::strip_metadata_from_json;
use core_test_support::responses::strip_response_item_ids_from_json;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use wiremock::matchers::header;

#[test_case::test_case(false; "native_openai")]
#[test_case::test_case(true; "external_provider")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn code_mode_spawn_delivers_plaintext(external_provider: bool) -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let test = test_codex()
        .with_model("gpt-5.6-sol")
        .with_model_info_override("gpt-5.6-sol", |model| {
            model.tool_mode = Some(ToolMode::CodeMode);
            model.use_responses_lite = true;
        })
        .with_config(move |config| {
            // Isolate task encoding from the fresh child's own environment context.
            config.include_environment_context = false;
            for feature in [
                Feature::Collab,
                Feature::MultiAgentV2,
                Feature::CodeMode,
                Feature::CodeModeHostGrpc,
            ] {
                config
                    .features
                    .enable(feature)
                    .expect("enable test feature");
            }
            config.multi_agent_v2.non_code_mode_only = false;
            if external_provider {
                let mut provider = config.model_provider.clone();
                provider.requires_openai_auth = false;
                provider.env_key = Some("PATH".to_string());
                config
                    .model_providers
                    .insert("deepseek".to_string(), provider);
                let directory = config.codex_home.join("model-catalogs");
                std::fs::create_dir_all(&directory).expect("test catalog directory");
                let mut catalog =
                    codex_models_manager::bundled_models_response().expect("bundled test catalog");
                catalog.models.truncate(1);
                let model = &mut catalog.models[0];
                model.slug = "deepseek-code-mode-test".to_string();
                model.display_name = model.slug.clone();
                model.use_responses_lite = false;
                model.tool_mode = None;
                std::fs::write(
                    directory.join("deepseek.json"),
                    serde_json::to_vec(&catalog).expect("serialize test catalog"),
                )
                .expect("write test catalog");
            }
        })
        .build_with_auto_env(&server)
        .await?;
    let root_header = test.session_configured.thread_id.to_string();
    let mut created = test.thread_manager.subscribe_thread_created();

    mount_sse_once_match(
        &server,
        header("thread-id", root_header.as_str()),
        sse(vec![
            ev_response_created("spawn"),
            ev_custom_tool_call(
                "spawn-worker",
                "exec",
                if external_provider {
                    r#"text(await tools.spawn_external_agent({task_name: "worker", model: "deepseek-code-mode-test", message: "Summarize the three stories.", fork_turns: "none"}));"#
                } else {
                    r#"text(await tools.collaboration__spawn_agent({task_name: "worker", message: "Summarize the three stories.", fork_turns: "all"}));"#
                },
            ),
            ev_completed("spawn"),
        ]),
    )
    .await;
    mount_sse_once_match(
        &server,
        header("thread-id", root_header.as_str()),
        sse(vec![
            ev_assistant_message("delegated", "The worker started."),
            ev_completed("delegated"),
        ]),
    )
    .await;
    let child_requests = mount_sse_once_match(
        &server,
        move |request: &wiremock::Request| {
            request
                .headers
                .get("thread-id")
                .is_some_and(|id| id != root_header.as_str())
        },
        sse(vec![ev_response_created("child"), ev_completed("child")]),
    )
    .await;
    test.submit_turn("Delegate the summary task.").await?;
    let child_id = created.try_recv()?;
    let child = test.thread_manager.get_thread(child_id).await?;
    wait_for_event(&child, |event| matches!(event, EventMsg::TurnComplete(_))).await;

    let child_request = child_requests
        .requests()
        .into_iter()
        .find(|request| request.header("thread-id") == Some(child_id.to_string()))
        .expect("child receives its task");
    let task = "Message Type: NEW_TASK\nTask name: /root/worker\nSender: /root\nPayload:\nSummarize the three stories.";
    if external_provider {
        assert!(child_request.inputs_of_type("agent_message").is_empty());
        let input = child_request.body_json()["input"].to_string();
        assert!(!input.contains("encrypted_content"));
        assert!(!input.contains("Delegate the summary task."));
        let messages = child_request
            .inputs_of_type("message")
            .into_iter()
            .filter(|item| item["role"] == "user")
            .collect();
        assert_eq!(
            strip_response_item_ids_from_json(strip_metadata_from_json(Value::Array(messages))),
            json!([{"type": "message", "role": "user", "content": [{"type": "input_text", "text": task}]}]),
        );
        let config = child.config_snapshot().await;
        assert_eq!(
            (config.model_provider_id.as_str(), config.model.as_str()),
            ("deepseek", "deepseek-code-mode-test"),
        );
    } else {
        assert_eq!(
            strip_response_item_ids_from_json(strip_metadata_from_json(Value::Array(
                child_request.inputs_of_type("agent_message"),
            ))),
            json!([{
                "type": "agent_message",
                "author": "/root",
                "recipient": "/root/worker",
                "content": [{"type": "input_text", "text": task}],
            }]),
        );
    }
    assert_eq!(test.thread_manager.list_thread_ids().await.len(), 2);
    Ok(())
}
