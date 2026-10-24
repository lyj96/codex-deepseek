//! Exercises live catalog routing through the public model/list RPC.

use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::LoginAccountResponse;
use codex_app_server_protocol::LogoutAccountResponse;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::RequestId;
use codex_config::types::AuthCredentialsStoreMode;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelsResponse;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use tempfile::TempDir;
use test_case::test_case;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

fn catalog(slug: &str) -> Result<ModelsResponse> {
    Ok(ModelsResponse {
        models: vec![serde_json::from_value::<ModelInfo>(json!({
            "slug": slug,
            "display_name": slug,
            "supported_reasoning_levels": [],
            "shell_type": "shell_command",
            "visibility": "list",
            "supported_in_api": true,
            "priority": 0,
            "support_verbosity": false,
            "truncation_policy": {"mode": "bytes", "limit": 10000},
            "experimental_supported_tools": []
        }))?],
    })
}

async fn list_models(server: &mut TestAppServer) -> Result<ModelListResponse> {
    server
        .request(|request_id| ClientRequest::ModelList {
            request_id,
            params: ModelListParams::default(),
        })
        .await
}

async fn sign_in(server: &mut TestAppServer) -> Result<()> {
    let id = server
        .send_login_account_api_key_request("sk-catalog-test")
        .await?;
    let _: LoginAccountResponse = server.read_response(id).await?;
    Ok(())
}

#[test_case("other"; "selection_changes")]
#[test_case("gateway"; "definition_changes")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// Switching managed provider selection or definition refreshes the catalog in
/// the same server, while invalid requirements still block catalog access.
async fn model_list_follows_changed_provider_without_restart(
    required_selection: &str,
) -> Result<()> {
    let gateway = MockServer::start().await;
    let other = MockServer::start().await;
    for (server, slug) in [(&gateway, "gateway-model"), (&other, "other-model")] {
        Mock::given(method("GET"))
            .and(path_regex(".*/models$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(catalog(slug)?))
            .mount(server)
            .await;
    }
    let home = TempDir::new()?;
    let providers = format!(
        r#"
[model_providers.gateway]
name = "Gateway"
base_url = "{}/v1"
model_catalog_url = "{}/v1/models"
requires_openai_auth = true
[model_providers.other]
name = "Other"
base_url = "{}/v1"
model_catalog_url = "{}/v1/models"
requires_openai_auth = true
"#,
        gateway.uri(),
        gateway.uri(),
        other.uri(),
        other.uri(),
    );
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "model_provider = 'gateway'\n[features]\napi_key_model_discovery = true\n{providers}"
        ),
    )?;
    codex_login::login_with_api_key(
        home.path(),
        "sk-initial-catalog-test",
        AuthCredentialsStoreMode::File,
        codex_login::AuthKeyringBackendKind::default(),
    )?;
    let requirements_path = home.path().join("requirements.toml");
    std::fs::write(
        &requirements_path,
        format!("model_provider = 'gateway'\n{providers}"),
    )?;
    let mut server = TestAppServer::builder()
        .with_mock_chatgpt_backend()
        .with_codex_home(home.path())
        .with_env_overrides(&[("OPENAI_API_KEY", None)])
        .build_initialized_with_timeout(Duration::from_secs(/*secs*/ 60))
        .await?;

    let initial = list_models(&mut server).await?;
    assert_eq!(
        initial
            .data
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["gateway-model"]
    );
    let updated_providers = if required_selection == "gateway" {
        providers.replace(&gateway.uri(), &other.uri())
    } else {
        providers.clone()
    };
    let updated = format!("model_provider = '{required_selection}'\n{updated_providers}");
    std::fs::write(&requirements_path, &updated)?;
    sign_in(&mut server).await?;
    let switched = list_models(&mut server).await?;
    assert_eq!(
        switched
            .data
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["other-model"]
    );
    let requests_before = [
        gateway
            .received_requests()
            .await
            .expect("recorded provider requests")
            .len(),
        other
            .received_requests()
            .await
            .expect("recorded provider requests")
            .len(),
    ];
    // A warm catalog must not hide a policy load failure or send to either provider.
    std::fs::write(&requirements_path, "model_provider = []")?;
    let id = server
        .send_raw_request("model/list", Some(json!({})))
        .await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert_eq!(error.error.code, -32600);
    assert!(error.error.message.contains("failed to load configuration"));
    assert_eq!(
        [
            gateway
                .received_requests()
                .await
                .expect("recorded provider requests")
                .len(),
            other
                .received_requests()
                .await
                .expect("recorded provider requests")
                .len(),
        ],
        requests_before,
    );
    // Removing the requirement restores the user-configured provider in this process.
    std::fs::remove_file(requirements_path)?;
    sign_in(&mut server).await?;
    assert_eq!(list_models(&mut server).await?, initial);
    Ok(())
}

/// Warm lists reuse their configuration; a completed login reloads the catalog once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_list_reuses_catalog_until_account_change() -> Result<()> {
    let home = TempDir::new()?;
    let catalog_path = home.path().join("catalog.json");
    std::fs::write(&catalog_path, serde_json::to_vec(&catalog("first-model")?)?)?;
    let mut config = toml::map::Map::new();
    config.insert(
        "model_catalog_json".to_string(),
        toml::Value::String(catalog_path.to_string_lossy().into_owned()),
    );
    std::fs::write(home.path().join("config.toml"), toml::to_string(&config)?)?;
    let mut server = TestAppServer::builder()
        .with_mock_chatgpt_backend()
        .with_codex_home(home.path())
        .with_env_overrides(&[("OPENAI_API_KEY", None)])
        .build_initialized()
        .await?;
    let initial = list_models(&mut server).await?;
    assert_eq!(
        initial
            .data
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["first-model"]
    );
    // A full config reload would try to parse this invalid catalog.
    std::fs::write(&catalog_path, "not JSON")?;
    assert_eq!(list_models(&mut server).await?, initial);
    std::fs::write(
        &catalog_path,
        serde_json::to_vec(&catalog("second-model")?)?,
    )?;
    assert_eq!(list_models(&mut server).await?, initial);
    sign_in(&mut server).await?;
    let changed = list_models(&mut server).await?;
    assert_eq!(
        changed
            .data
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["second-model"]
    );
    std::fs::write(
        &catalog_path,
        serde_json::to_vec(&catalog("logged-out-model")?)?,
    )?;
    let id = server.send_logout_account_request().await?;
    let _: LogoutAccountResponse = server.read_response(id).await?;
    assert_eq!(
        list_models(&mut server)
            .await?
            .data
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["logged-out-model"]
    );
    Ok(())
}

/// Account transitions preserve the non-strict startup fallback for malformed user config.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_list_preserves_invalid_startup_config_fallback() -> Result<()> {
    let home = TempDir::new()?;
    std::fs::write(home.path().join("config.toml"), "model = [")?;
    let mut server = TestAppServer::builder()
        .with_mock_chatgpt_backend()
        .with_codex_home(home.path())
        .with_env_overrides(&[("OPENAI_API_KEY", None)])
        .build_initialized_with_timeout(Duration::from_secs(/*secs*/ 60))
        .await?;
    let initial = list_models(&mut server).await?;
    assert!(!initial.data.is_empty());
    sign_in(&mut server).await?;
    assert!(!list_models(&mut server).await?.data.is_empty());
    let id = server.send_logout_account_request().await?;
    let _: LogoutAccountResponse = server.read_response(id).await?;
    assert!(!list_models(&mut server).await?.data.is_empty());
    Ok(())
}

/// Managed aliases keep one default on the current provider, even when another
/// provider exposes the same API model. Existing catalog IDs are not duplicated.
#[test_case("gateway-model"; "existing_catalog_id")]
#[test_case("gateway-public"; "public_alias")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_list_defaults_to_current_provider_public_model(public_model: &str) -> Result<()> {
    let home = TempDir::new()?;
    let mock = MockServer::start().await;
    let catalog_dir = home.path().join("model-catalogs");
    std::fs::create_dir(&catalog_dir)?;
    let catalog_bytes = serde_json::to_vec(&catalog("gateway-model")?)?;
    for provider in ["gateway", "other"] {
        std::fs::write(catalog_dir.join(format!("{provider}.json")), &catalog_bytes)?;
    }
    std::fs::write(
        catalog_dir.join("routes.json"),
        serde_json::to_vec(&json!({
            "version": 1,
            "models": {
                (public_model): {"provider": "gateway", "api_model": "gateway-model"},
                "other-public": {"provider": "other", "api_model": "gateway-model"}
            }
        }))?,
    )?;
    let uri = mock.uri();
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            r#"
model_provider = "gateway"
model = "gateway-model"
[model_providers.gateway]
name = "Gateway"
base_url = "{uri}/v1"
requires_openai_auth = false
[model_providers.other]
name = "Other"
base_url = "{uri}/v1"
requires_openai_auth = false
"#
        ),
    )?;
    let mut server = TestAppServer::builder()
        .with_mock_chatgpt_backend()
        .with_codex_home(home.path())
        .with_env_overrides(&[("OPENAI_API_KEY", None), ("CODEX_API_KEY", None)])
        .build_initialized()
        .await?;
    let response = list_models(&mut server).await?;
    let mut expected = vec![("gateway-model", public_model == "gateway-model")];
    if public_model != "gateway-model" {
        expected.push((public_model, true));
    }
    expected.push(("other-public", false));
    assert_eq!(
        response
            .data
            .iter()
            .map(|model| (model.id.as_str(), model.is_default))
            .collect::<Vec<_>>(),
        expected,
    );
    assert_eq!(response.next_cursor, None);
    assert!(
        mock.received_requests()
            .await
            .expect("recorded requests")
            .is_empty()
    );
    Ok(())
}
