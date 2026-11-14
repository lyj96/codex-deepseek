//! Admission of retained threads selected through the Fork's configured model catalog.

use super::*;
use anyhow::Result;
use codex_config::test_support::CloudConfigBundleFixture;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tempfile::tempdir;
use test_case::test_case;

fn catalog_home() -> Result<TempDir> {
    let home = tempdir()?;
    std::fs::write(
        home.path().join("config.toml"),
        r#"
model_provider = "openai"
model = "gpt-5.6-sol"
[model_providers.qwen]
name = "Qwen"
base_url = "https://qwen.invalid/v1"
wire_api = "responses"
supports_websockets = false
[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://deepseek.invalid/v1"
wire_api = "responses"
supports_websockets = false
"#,
    )?;
    let directory = home.path().join("model-catalogs");
    std::fs::create_dir(&directory)?;
    for provider in ["qwen", "deepseek"] {
        let mut catalog = codex_models_manager::bundled_models_response()?;
        catalog.models.truncate(/*len*/ 1);
        catalog.models[0].slug = format!("{provider}-api");
        std::fs::write(
            directory.join(format!("{provider}.json")),
            serde_json::to_vec(&catalog)?,
        )?;
    }
    std::fs::write(
        directory.join("routes.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "models": {
                "qwen-public": {"provider": "qwen", "api_model": "qwen-api"},
                "deepseek-public": {"provider": "deepseek", "api_model": "deepseek-api"}
            }
        }))?,
    )?;
    Ok(home)
}

#[tokio::test]
async fn routed_provider_accepts_input_while_default_is_openai() -> Result<()> {
    let home = catalog_home()?;
    let manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let current = manager
        .load_for_cwd(
            /*request_overrides*/ None,
            ConfigOverrides {
                model: Some("qwen-public".into()),
                ..Default::default()
            },
            Some(home.path().to_path_buf()),
        )
        .await?;
    assert_eq!(current.model_provider_id, "qwen");
    assert_eq!(current.model.as_deref(), Some("qwen-api"));
    manager
        .check_thread_model_provider_for_input(&current)
        .await?;
    assert_eq!(
        manager
            .load_latest_config(/*fallback_cwd*/ None)
            .await?
            .model_provider_id,
        "openai"
    );
    Ok(())
}

#[test_case("qwen-api"; "catalog_api_model")]
#[test_case("future-custom-api"; "unknown_current_provider_api_model")]
#[tokio::test]
async fn routed_provider_resumes_without_changing_default(model: &str) -> Result<()> {
    let home = catalog_home()?;
    let manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let config = manager
        .load_for_resume(
            Some("qwen"),
            /*request_overrides*/ None,
            ConfigOverrides {
                model_provider: Some("qwen".into()),
                model: Some(model.into()),
                ..Default::default()
            },
            Some(home.path().to_path_buf()),
        )
        .await?;
    assert_eq!(config.model_provider_id, "qwen");
    assert_eq!(config.model.as_deref(), Some(model));
    manager
        .check_thread_model_provider_for_input(&config)
        .await?;
    Ok(())
}

#[tokio::test]
async fn catalog_admission_retains_openai_threads_with_an_external_default() -> Result<()> {
    let home = catalog_home()?;
    let manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let current = manager.load_latest_config(/*fallback_cwd*/ None).await?;
    let path = home.path().join("config.toml");
    let config = std::fs::read_to_string(&path)?;
    std::fs::write(
        path,
        config.replace("model_provider = \"openai\"", "model_provider = \"qwen\""),
    )?;
    manager
        .check_thread_model_provider_for_input(&current)
        .await?;
    Ok(())
}

fn saved_provider_home() -> Result<TempDir> {
    let home = tempdir()?;
    std::fs::write(
        home.path().join("config.toml"),
        r#"
model_provider = "openai"
[model_providers.original]
name = "Original provider"
base_url = "https://original.invalid/v1"
wire_api = "responses"
supports_websockets = false
"#,
    )?;
    Ok(home)
}

#[test_case("gpt-5.2-codex"; "restored_provider_model")]
#[test_case("future-custom-api"; "unknown_custom_api_model")]
#[tokio::test]
async fn saved_provider_without_catalog_resumes_without_changing_default(
    model: &str,
) -> Result<()> {
    let home = saved_provider_home()?;
    let manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let config = manager
        .load_for_resume(
            Some("original"),
            /*request_overrides*/ None,
            ConfigOverrides {
                model: Some(model.into()),
                ..Default::default()
            },
            Some(home.path().to_path_buf()),
        )
        .await?;
    assert_eq!(config.model_provider_id, "original");
    assert_eq!(config.model.as_deref(), Some(model));
    manager
        .check_thread_model_provider_for_input(&config)
        .await?;
    assert_eq!(
        manager
            .load_latest_config(/*fallback_cwd*/ None)
            .await?
            .model_provider_id,
        "openai"
    );
    Ok(())
}

#[tokio::test]
async fn saved_provider_without_catalog_rejects_provider_switch() -> Result<()> {
    let home = saved_provider_home()?;
    let manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let error = manager
        .load_for_resume(
            Some("original"),
            /*request_overrides*/ None,
            ConfigOverrides {
                model_provider: Some("openai".into()),
                ..Default::default()
            },
            Some(home.path().to_path_buf()),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ModelProviderError>()),
        Some(&ModelProviderError::ModelProviderMismatch {
            model_provider: "original".into(),
            current_model_provider: "openai".into(),
        })
    );
    Ok(())
}

#[tokio::test]
async fn saved_provider_without_catalog_rejects_definition_override() -> Result<()> {
    let home = saved_provider_home()?;
    let manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let error = manager
        .load_for_resume(
            Some("original"),
            Some(HashMap::from([(
                "model_providers.original.base_url".into(),
                serde_json::json!("https://replacement.invalid/v1"),
            )])),
            ConfigOverrides::default(),
            Some(home.path().to_path_buf()),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ModelProviderError>()),
        Some(&ModelProviderError::ModelProviderNotConfigured {
            model_provider: "original".into()
        })
    );
    Ok(())
}

#[tokio::test]
async fn saved_provider_without_catalog_still_obeys_managed_requirements() -> Result<()> {
    let home = saved_provider_home()?;
    let manager = ConfigManager::new_for_tests(
        home.path().to_path_buf(),
        Vec::new(),
        LoaderOverrides::without_managed_config_for_tests(),
        CloudConfigBundleFixture::loader_with_enterprise_requirement("model_provider = 'openai'"),
    );
    let error = manager
        .load_for_resume(
            Some("original"),
            /*request_overrides*/ None,
            ConfigOverrides::default(),
            Some(home.path().to_path_buf()),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ModelProviderError>()),
        Some(&ModelProviderError::ModelProviderPolicyConflict {
            model_provider: "original".into(),
            required_model_provider: "openai".into(),
        })
    );
    Ok(())
}

#[test_case("qwen", "deepseek-public"; "public_model_changes_provider")]
#[test_case("deepseek", "deepseek-api"; "explicit_provider_changes_provider")]
#[tokio::test]
async fn catalog_resume_rejects_provider_changes(provider: &str, model: &str) -> Result<()> {
    let home = catalog_home()?;
    let manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let error = manager
        .load_for_resume(
            Some("qwen"),
            /*request_overrides*/ None,
            ConfigOverrides {
                model_provider: Some(provider.into()),
                model: Some(model.into()),
                ..Default::default()
            },
            Some(home.path().to_path_buf()),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ModelProviderError>()),
        Some(ModelProviderError::ModelProviderMismatch { .. })
    ));
    Ok(())
}

#[tokio::test]
async fn catalog_admission_rejects_redefined_provider() -> Result<()> {
    let home = catalog_home()?;
    let manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let current = manager
        .load_with_overrides(
            /*request_overrides*/ None,
            ConfigOverrides {
                model: Some("qwen-public".into()),
                ..Default::default()
            },
        )
        .await?;
    let path = home.path().join("config.toml");
    let config = std::fs::read_to_string(&path)?;
    std::fs::write(
        path,
        config.replace("https://qwen.invalid/v1", "https://redefined.invalid/v1"),
    )?;
    let error = manager
        .check_thread_model_provider_for_input(&current)
        .await
        .unwrap_err();
    assert_eq!(
        error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ModelProviderError>()),
        Some(&ModelProviderError::ModelProviderNotConfigured {
            model_provider: "qwen".into()
        })
    );
    Ok(())
}

#[tokio::test]
async fn catalog_resume_rejects_provider_definition_override() -> Result<()> {
    let home = catalog_home()?;
    let manager = ConfigManager::without_managed_config_for_tests(home.path().to_path_buf());
    let error = manager
        .load_for_resume(
            Some("qwen"),
            Some(HashMap::from([(
                "model_providers.qwen.base_url".into(),
                serde_json::json!("https://override.invalid/v1"),
            )])),
            ConfigOverrides {
                model_provider: Some("qwen".into()),
                model: Some("qwen-api".into()),
                ..Default::default()
            },
            Some(home.path().to_path_buf()),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ModelProviderError>()),
        Some(&ModelProviderError::ModelProviderNotConfigured {
            model_provider: "qwen".into()
        })
    );
    Ok(())
}

#[tokio::test]
async fn catalog_route_cannot_override_required_provider_at_startup() -> Result<()> {
    let home = catalog_home()?;
    let manager = ConfigManager::new_for_tests(
        home.path().to_path_buf(),
        Vec::new(),
        LoaderOverrides::without_managed_config_for_tests(),
        CloudConfigBundleFixture::loader_with_enterprise_requirement("model_provider = 'openai'"),
    );
    let error = manager
        .load_with_overrides(
            /*request_overrides*/ None,
            ConfigOverrides {
                model: Some("qwen-public".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ModelProviderError>()),
        Some(&ModelProviderError::ModelProviderPolicyConflict {
            model_provider: "qwen".into(),
            required_model_provider: "openai".into()
        })
    );
    Ok(())
}
