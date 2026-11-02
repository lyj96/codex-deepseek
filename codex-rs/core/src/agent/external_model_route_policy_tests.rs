//! Managed provider requirements apply to direct child routing as well as config loading.

use super::*;
use crate::config::ConfigBuilder;
use anyhow::Result;
use codex_config::test_support::CloudConfigBundleFixture;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tempfile::tempdir;
use test_case::test_case;

fn routed_home() -> Result<TempDir> {
    let home = tempdir()?;
    std::fs::write(
        home.path().join("config.toml"),
        r#"
model_provider = "openai"
[model_providers.qwen]
name = "Qwen"
base_url = "https://qwen.invalid/v1"
wire_api = "responses"
"#,
    )?;
    let directory = home.path().join("model-catalogs");
    std::fs::create_dir(&directory)?;
    let mut catalog = codex_models_manager::bundled_models_response()?;
    catalog.models.truncate(/*len*/ 1);
    catalog.models[0].slug = "qwen-api".into();
    std::fs::write(directory.join("qwen.json"), serde_json::to_vec(&catalog)?)?;
    std::fs::write(
        directory.join("routes.json"),
        r#"{
        "version": 1,
        "models": {"qwen-public": {"provider": "qwen", "api_model": "qwen-api"}}
    }"#,
    )?;
    Ok(home)
}

#[test_case("qwen-public"; "public_route")]
#[test_case("qwen-api"; "legacy_prefix_route")]
#[tokio::test]
async fn child_route_cannot_escape_required_provider(model: &str) -> Result<()> {
    let home = routed_home()?;
    let mut config = ConfigBuilder::without_managed_config_for_tests()
        .codex_home(home.path().to_path_buf())
        .cloud_config_bundle(
            CloudConfigBundleFixture::loader_with_enterprise_requirement(
                "model_provider = 'openai'",
            ),
        )
        .build()
        .await?;
    let provider = config.model_provider.clone();
    let error = apply_external_model_route(&mut config, model).unwrap_err();
    assert!(
        matches!(error, ExternalModelRouteError::RequiredProvider(conflict)
        if conflict.model_provider == "qwen" && conflict.required_model_provider == "openai")
    );
    assert_eq!(config.model_provider_id, "openai");
    assert_eq!(config.model_provider, provider);
    assert!(config.model_catalog.is_none());
    Ok(())
}

#[tokio::test]
async fn child_route_accepts_the_required_external_provider() -> Result<()> {
    let home = routed_home()?;
    let mut config = ConfigBuilder::without_managed_config_for_tests()
        .codex_home(home.path().to_path_buf())
        .cloud_config_bundle(
            CloudConfigBundleFixture::loader_with_enterprise_requirement("model_provider = 'qwen'"),
        )
        .build()
        .await?;
    let route = apply_external_model_route(&mut config, "qwen-public")?;
    assert!(matches!(route, ExternalModelRoute::Applied { model, .. } if model == "qwen-api"));
    assert_eq!(config.model_provider_id, "qwen");
    Ok(())
}
