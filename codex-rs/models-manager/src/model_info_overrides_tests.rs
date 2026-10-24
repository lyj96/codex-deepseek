use crate::ModelsManagerConfig;
use crate::manager::ModelsManager;
use codex_protocol::openai_models::TruncationPolicyConfig;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::TestModelsEndpoint;
use super::openai_manager_for_tests;
use super::remote_model;
use crate::manager::OpenAiModelsManager;
use crate::manager::RefreshStrategy;
use codex_http_client::HttpClientFactory;
use codex_http_client::OutboundProxyPolicy;
use codex_protocol::error::CodexErr;
use codex_protocol::openai_models::ModelsResponse;

#[tokio::test]
async fn configured_catalog_keeps_exact_metadata_after_discovery_failures() {
    let mut configured = remote_model("provider-model", "Configured Model", /*priority*/ 0);
    configured.supports_image_detail_original = true;
    let config = ModelsManagerConfig {
        model_catalog: Some(ModelsResponse {
            models: vec![configured.clone()],
        }),
        model_context_window: Some(64_000),
        ..Default::default()
    };
    let endpoint = TestModelsEndpoint::with_command_auth(vec![
        Ok(vec![remote_model(
            "provider-model",
            "Discovered Model",
            /*priority*/ 0,
        )]),
        Err(CodexErr::RequestTimeout),
        Err(super::catalog_http_error(/*status*/ 403)),
    ]);
    let manager = OpenAiModelsManager::new_without_cache(endpoint, /*auth_manager*/ None)
        .with_provider_catalog();
    let expected = crate::model_info::with_config_overrides(
        codex_protocol::openai_models::ModelInfo {
            used_fallback_model_metadata: false,
            ..configured
        },
        &config,
    );
    for _ in 0..3 {
        manager
            .raw_model_catalog(
                RefreshStrategy::Online,
                HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
            )
            .await;
        assert_eq!(
            manager.get_model_info("provider-model", &config).await,
            expected
        );
        let unknown = "provider-model-unlisted";
        assert_eq!(
            manager.get_model_info(unknown, &config).await,
            crate::model_info::with_config_overrides(
                crate::model_info::model_info_from_slug(unknown),
                &config,
            )
        );
    }
    assert!(manager.get_remote_models().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_model_info_without_tool_output_override() {
    let codex_home = TempDir::new().expect("create temp dir");
    let config = ModelsManagerConfig::default();
    let manager = openai_manager_for_tests(
        codex_home.path().to_path_buf(),
        TestModelsEndpoint::new(Vec::new()),
    );

    let model_info = manager.get_model_info("unknown-model", &config).await;

    assert_eq!(
        model_info.truncation_policy,
        TruncationPolicyConfig::bytes(/*limit*/ 10_000)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_model_info_with_tool_output_override() {
    let codex_home = TempDir::new().expect("create temp dir");
    let config = ModelsManagerConfig {
        tool_output_token_limit: Some(123),
        ..Default::default()
    };
    let manager = openai_manager_for_tests(
        codex_home.path().to_path_buf(),
        TestModelsEndpoint::new(Vec::new()),
    );

    let model_info = manager.get_model_info("gpt-5.5", &config).await;

    assert_eq!(
        model_info.truncation_policy,
        TruncationPolicyConfig::tokens(/*limit*/ 123)
    );
}
