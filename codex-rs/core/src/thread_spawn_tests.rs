use super::*;
use crate::ThreadManager;
use crate::config::ConfigBuilder;
use crate::thread_manager::StartThreadOptions;
use codex_features::Feature;
use codex_login::CodexAuth;
use codex_models_manager::bundled_models_response;
use codex_protocol::error::CodexErrorDetails;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

async fn routed_parent() -> anyhow::Result<(
    tempfile::TempDir,
    ThreadManager,
    Arc<crate::codex_thread::CodexThread>,
)> {
    let home = tempfile::tempdir()?;
    let mut config = ConfigBuilder::without_managed_config_for_tests()
        .codex_home(home.path().to_path_buf())
        .build()
        .await?;
    config.features.enable(Feature::Collab)?;
    config.features.enable(Feature::MultiAgentV2)?;
    let mut provider = config.model_provider.clone();
    provider.name = "External test provider".into();
    provider.base_url = Some("http://127.0.0.1:9/v1".into());
    provider.env_key = None;
    provider.requires_openai_auth = false;
    provider.supports_websockets = false;
    config
        .model_providers
        .insert("external-test".into(), provider);
    let directory = home.path().join("model-catalogs");
    std::fs::create_dir(&directory)?;
    let mut catalog = bundled_models_response()?;
    catalog.models.truncate(/*len*/ 1);
    catalog.models[0].slug = "api-worker".into();
    std::fs::write(
        directory.join("external-test.json"),
        serde_json::to_vec(&catalog)?,
    )?;
    std::fs::write(
        directory.join("routes.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "models": {"public-worker": {"provider": "external-test", "api_model": "api-worker"}}
        }))?,
    )?;
    let manager = ThreadManager::with_models_provider_and_home_for_tests(
        CodexAuth::from_api_key("dummy"),
        config.model_provider.clone(),
        home.path().to_path_buf(),
        Arc::new(codex_exec_server::EnvironmentManager::default_for_tests()),
    );
    let parent = manager
        .start_thread(StartThreadOptions::new(config))
        .await?
        .thread;
    Ok((home, manager, parent))
}

#[tokio::test]
async fn routed_creation_preserves_api_model_and_openai_history_boundary() -> anyhow::Result<()> {
    for (fork_mode, fork_turns_was_explicit) in [
        (None, true),
        (Some(SpawnAgentForkMode::FullHistory), false),
        (Some(SpawnAgentForkMode::FullHistory), true),
    ] {
        let (_home, manager, parent) = routed_parent().await?;
        let session = &parent.session;
        let step = session
            .capture_step_context(session.new_default_turn().await, &CancellationToken::new())
            .await?;
        let reject_history = fork_mode.is_some() && fork_turns_was_explicit;
        let result = prepare_routed(
            session,
            &step,
            "external-call",
            ThreadCreateRequest {
                task_name: "worker".into(),
                fork_mode,
                agent_type: None,
                model: Some("public-worker".into()),
                reasoning_effort: None,
            },
            SpawnModelRoute::ExternalProvider,
            fork_turns_was_explicit,
        )
        .await;
        if reject_history {
            assert!(matches!(result, Err(ThreadSpawnError::Preparation(message))
                if message.contains("encrypted history cannot be forwarded")));
        } else {
            let (settings, task_name) = result?;
            assert_eq!(
                (
                    settings.config.model_provider_id.as_str(),
                    settings.config.model.as_deref()
                ),
                ("external-test", Some("api-worker")),
            );
            assert!(settings.options.fork_mode.is_none());
            assert_eq!(task_name.to_string(), "/root/worker");
        }
        assert_eq!(manager.list_thread_ids().await, vec![session.thread_id]);
        parent.shutdown_and_wait().await?;
    }
    Ok(())
}

#[tokio::test]
async fn host_creation_retains_the_native_public_request() -> anyhow::Result<()> {
    let (_home, manager, parent) = routed_parent().await?;
    let session = &parent.session;
    let step = session
        .capture_step_context(session.new_default_turn().await, &CancellationToken::new())
        .await?;
    let (settings, _) = prepare(
        session,
        &step,
        "host-call",
        ThreadCreateRequest {
            task_name: "host_worker".into(),
            fork_mode: None,
            agent_type: None,
            model: None,
            reasoning_effort: None,
        },
    )
    .await?;
    assert_eq!(settings.config.model_provider_id, "openai");
    assert!(settings.options.fork_mode.is_none());
    assert_eq!(manager.list_thread_ids().await, vec![session.thread_id]);
    parent.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn routed_spawn_never_downgrades_openai_encoded_messages() -> anyhow::Result<()> {
    let (_home, manager, parent) = routed_parent().await?;
    let session = &parent.session;
    let step = session
        .capture_step_context(session.new_default_turn().await, &CancellationToken::new())
        .await?;
    let result = spawn(
        session,
        &step,
        "opaque-parent-call",
        ThreadSpawnRequest {
            task_name: "external_worker".into(),
            message: ThreadSpawnMessage::ProviderEncoded("opaque-parent-ciphertext".into()),
            fork_mode: None,
            fork_turns_was_explicit: true,
            model_route: SpawnModelRoute::ExternalProvider,
            agent_type: None,
            model: Some("public-worker".into()),
            reasoning_effort: None,
        },
    )
    .await;
    let observed = manager.list_thread_ids().await;
    // Always finish any unexpected child before reporting an assertion failure.
    for id in observed.iter().filter(|id| **id != session.thread_id) {
        manager.get_thread(*id).await?.shutdown_and_wait().await?;
    }
    parent.shutdown_and_wait().await?;
    let actual_error = result.as_ref().err().map(ToString::to_string);
    assert!(
        matches!(result,
            Err(ThreadSpawnError::Backend(error))
                if matches!(error.details(), CodexErrorDetails::UnsupportedOperation(message)
                    if message.contains("OpenAI encrypted agent messages cannot be sent to an external provider"))
        ),
        "expected rejection before allocation, error={actual_error:?}, observed={observed:?}"
    );
    assert_eq!(observed, vec![session.thread_id]);
    Ok(())
}
