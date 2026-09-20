//! Resolves explicit sub-agent model names onto configured external providers.
//!
//! External routing is opt-in. Explicit routes live in
//! `~/.codex/model-catalogs/routes.json` and map a public model name to both a configured provider
//! and the model name sent to that provider. Provider-prefixed model names remain supported for
//! backwards compatibility.

use crate::config::Config;
use crate::config::ConfiguredExternalModel;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelsResponse;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

const EXTERNAL_MODEL_CATALOGS_DIR: &str = "model-catalogs";
const EXTERNAL_MODEL_ROUTES_FILE: &str = "routes.json";

#[derive(Debug, Deserialize)]
struct ExternalModelRoutes {
    #[allow(dead_code)]
    #[serde(default)]
    version: u32,
    #[serde(default)]
    models: BTreeMap<String, ExternalModelRouteConfig>,
}

#[derive(Clone, Debug, Deserialize)]
struct ExternalModelRouteConfig {
    provider: String,
    api_model: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExternalModelRoute {
    NotMatched,
    Applied {
        role_name: Option<String>,
        model: String,
    },
}

/// Routes a public model name to its configured external provider and API model name.
pub(crate) fn apply_external_model_route(
    config: &mut Config,
    requested_model: &str,
) -> Result<ExternalModelRoute, String> {
    let explicit_route = load_external_model_routes(config)?
        .and_then(|routes| routes.models.get(requested_model).cloned());
    let (provider_id, api_model) = if let Some(route) = explicit_route {
        validate_explicit_route(config, requested_model, &route)?;
        (route.provider, route.api_model)
    } else if current_external_provider_has_model(config, requested_model)? {
        (
            config.model_provider_id.clone(),
            requested_model.to_string(),
        )
    } else if let Some(provider_id) = matching_external_provider_id(config, requested_model) {
        (provider_id, requested_model.to_string())
    } else {
        return Ok(ExternalModelRoute::NotMatched);
    };

    if config.model_provider_id == provider_id {
        apply_external_provider_catalog(config, &provider_id)?;
        return Ok(ExternalModelRoute::Applied {
            role_name: None,
            model: api_model,
        });
    }

    let provider = config
        .model_providers
        .get(&provider_id)
        .cloned()
        .ok_or_else(|| format!("model provider `{provider_id}` is not configured"))?;
    config.model_provider_id = provider_id;
    config.model_provider = provider;
    let provider_id = config.model_provider_id.clone();
    apply_external_provider_catalog(config, &provider_id)?;
    Ok(ExternalModelRoute::Applied {
        role_name: None,
        model: api_model,
    })
}

fn current_external_provider_has_model(
    config: &Config,
    requested_model: &str,
) -> Result<bool, String> {
    if config.model_provider_id == codex_model_provider_info::OPENAI_PROVIDER_ID {
        return Ok(false);
    }
    Ok(
        load_external_provider_catalog(config, &config.model_provider_id)?.is_some_and(|catalog| {
            catalog
                .models
                .iter()
                .any(|model| model.slug == requested_model)
        }),
    )
}

/// Returns provider-aware picker metadata from configured external-provider catalogs.
pub(crate) fn configured_external_models(config: &Config) -> Vec<ConfiguredExternalModel> {
    let mut models = BTreeMap::new();

    if let Ok(Some(routes)) = load_external_model_routes(config) {
        for (public_model, route) in routes.models {
            if validate_explicit_route(config, &public_model, &route).is_err() {
                continue;
            }
            let Ok(Some(catalog)) = load_external_provider_catalog(config, &route.provider) else {
                continue;
            };
            let Some(model) = catalog
                .models
                .into_iter()
                .find(|model| model.slug == route.api_model)
            else {
                continue;
            };
            let mut preset: ModelPreset = model.into();
            preset.id.clone_from(&public_model);
            preset.model.clone_from(&public_model);
            let provider_name = external_provider_name(config, &route.provider);
            append_provider_label(&mut preset, &provider_name);
            models.insert(
                public_model,
                ConfiguredExternalModel {
                    provider_id: route.provider,
                    provider_name,
                    api_model: route.api_model,
                    preset,
                },
            );
        }
    }

    for provider_id in config
        .model_providers
        .keys()
        .filter(|provider_id| provider_id.as_str() != codex_model_provider_info::OPENAI_PROVIDER_ID)
    {
        let Ok(Some(catalog)) = load_external_provider_catalog(config, provider_id) else {
            continue;
        };
        for model in catalog
            .models
            .into_iter()
            .filter(|model| model.slug.starts_with(provider_id.as_str()))
        {
            let public_model = model.slug.clone();
            let api_model = model.slug.clone();
            if models.values().any(|configured| {
                configured.provider_id == provider_id.as_str()
                    && configured.api_model == api_model.as_str()
            }) {
                continue;
            }
            let provider_name = external_provider_name(config, provider_id);
            models.entry(public_model).or_insert_with(|| {
                let mut preset = model.into();
                append_provider_label(&mut preset, &provider_name);
                ConfiguredExternalModel {
                    provider_id: provider_id.clone(),
                    provider_name,
                    api_model,
                    preset,
                }
            });
        }
    }

    models.into_values().collect()
}

/// Returns picker metadata for the external-agent tool schema.
pub(crate) fn configured_model_presets(config: &Config) -> Vec<ModelPreset> {
    configured_external_models(config)
        .into_iter()
        .map(|model| model.preset)
        .collect()
}

pub(crate) fn configured_external_model(
    config: &Config,
    requested_model: &str,
) -> Option<ConfiguredExternalModel> {
    configured_external_models(config)
        .into_iter()
        .find(|model| model.preset.model == requested_model)
}

pub(crate) fn provider_id_for_model(config: &Config, requested_model: &str) -> String {
    if let Some(model) = configured_external_model(config, requested_model) {
        return model.provider_id;
    }
    if current_external_provider_has_model(config, requested_model).unwrap_or(false) {
        return config.model_provider_id.clone();
    }
    matching_external_provider_id(config, requested_model)
        .unwrap_or_else(|| codex_model_provider_info::OPENAI_PROVIDER_ID.to_string())
}

fn external_provider_name(config: &Config, provider_id: &str) -> String {
    config
        .model_providers
        .get(provider_id)
        .map(|provider| provider.name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or(provider_id)
        .to_string()
}

fn append_provider_label(preset: &mut ModelPreset, provider_name: &str) {
    let suffix = format!(" [{provider_name}]");
    if preset.display_name.trim().is_empty() {
        preset.display_name.clone_from(&preset.model);
    }
    if !preset.display_name.ends_with(&suffix) {
        preset.display_name.push_str(&suffix);
    }
}

fn validate_explicit_route(
    config: &Config,
    public_model: &str,
    route: &ExternalModelRouteConfig,
) -> Result<(), String> {
    if public_model.trim().is_empty() {
        return Err("external model route names must not be empty".to_string());
    }
    if route.provider.trim().is_empty() || route.api_model.trim().is_empty() {
        return Err(format!(
            "external model route `{public_model}` must specify both `provider` and `api_model`"
        ));
    }
    if route.provider == codex_model_provider_info::OPENAI_PROVIDER_ID {
        return Err(format!(
            "external model route `{public_model}` cannot target the built-in OpenAI provider"
        ));
    }
    if !config.model_providers.contains_key(&route.provider) {
        return Err(format!(
            "external model route `{public_model}` references provider `{}` which is not configured",
            route.provider
        ));
    }
    Ok(())
}

pub(crate) fn model_matches_effective_external_provider(
    config: &Config,
    requested_model: &str,
) -> bool {
    config.model_provider_id != codex_model_provider_info::OPENAI_PROVIDER_ID
        && requested_model.starts_with(&config.model_provider_id)
}

fn matching_external_provider_id(config: &Config, requested_model: &str) -> Option<String> {
    config
        .model_providers
        .keys()
        .filter(|provider_id| provider_id.as_str() != codex_model_provider_info::OPENAI_PROVIDER_ID)
        .filter(|provider_id| requested_model.starts_with(provider_id.as_str()))
        .max_by_key(|provider_id| provider_id.len())
        .cloned()
}

pub(crate) fn apply_external_provider_catalog(
    config: &mut Config,
    provider_id: &str,
) -> Result<(), String> {
    if let Some(catalog) = load_external_provider_catalog(config, provider_id)? {
        config.model_catalog = Some(catalog);
    }
    Ok(())
}

fn load_external_provider_catalog(
    config: &Config,
    provider_id: &str,
) -> Result<Option<ModelsResponse>, String> {
    let Some(path) = external_provider_catalog_path(config, provider_id) else {
        return Ok(None);
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(format!(
                "failed to read model catalog `{}`: {err}",
                path.display()
            ));
        }
    };
    let catalog = serde_json::from_str::<ModelsResponse>(&contents).map_err(|err| {
        format!(
            "failed to parse model catalog `{}` as JSON: {err}",
            path.display()
        )
    })?;
    if catalog.models.is_empty() {
        return Err(format!(
            "model catalog `{}` must contain at least one model",
            path.display()
        ));
    }
    Ok(Some(catalog))
}

fn load_external_model_routes(config: &Config) -> Result<Option<ExternalModelRoutes>, String> {
    let path = config
        .codex_home
        .as_path()
        .join(EXTERNAL_MODEL_CATALOGS_DIR)
        .join(EXTERNAL_MODEL_ROUTES_FILE);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(format!(
                "failed to read external model routes `{}`: {err}",
                path.display()
            ));
        }
    };
    serde_json::from_str::<ExternalModelRoutes>(&contents)
        .map(Some)
        .map_err(|err| {
            format!(
                "failed to parse external model routes `{}` as JSON: {err}",
                path.display()
            )
        })
}

fn external_provider_catalog_path(config: &Config, provider_id: &str) -> Option<PathBuf> {
    let safe_provider_id = !provider_id.is_empty()
        && provider_id != "."
        && provider_id != ".."
        && provider_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    safe_provider_id.then(|| {
        config
            .codex_home
            .as_path()
            .join(EXTERNAL_MODEL_CATALOGS_DIR)
            .join(format!("{provider_id}.json"))
    })
}
