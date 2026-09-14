//! Resolves explicit sub-agent model names onto configured external providers.
//!
//! External routing is opt-in: a model slug must begin with a configured non-OpenAI provider id.
//! Provider-specific model metadata lives under `~/.codex/model-catalogs/<provider>.json`, so an
//! external provider does not need a synthetic agent role merely to select its catalog.

use crate::config::Config;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelsResponse;
use std::collections::BTreeMap;
use std::path::PathBuf;

const EXTERNAL_MODEL_CATALOGS_DIR: &str = "model-catalogs";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExternalModelRoute {
    NotMatched,
    Applied { role_name: Option<String> },
}

/// Routes a provider-prefixed model to that configured external provider.
pub(crate) fn apply_provider_prefix_route(
    config: &mut Config,
    requested_model: &str,
) -> Result<ExternalModelRoute, String> {
    let Some(provider_id) = matching_external_provider_id(config, requested_model) else {
        return Ok(ExternalModelRoute::NotMatched);
    };

    if config.model_provider_id == provider_id {
        apply_external_provider_catalog(config, &provider_id)?;
        return Ok(ExternalModelRoute::Applied { role_name: None });
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
    Ok(ExternalModelRoute::Applied { role_name: None })
}

/// Returns picker metadata from configured external-provider catalogs.
pub(crate) fn configured_model_presets(config: &Config) -> Vec<ModelPreset> {
    let mut models = BTreeMap::new();

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
            models
                .entry(model.slug.clone())
                .or_insert_with(|| model.into());
        }
    }

    models.into_values().collect()
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
