//! Resolves explicit sub-agent model names onto configured external providers.
//!
//! External routing is opt-in: a model slug must begin with a configured non-OpenAI provider id.
//! Personal agent roles remain the trusted place for provider-specific catalogs and settings, but
//! callers do not need to know or select the routing role themselves.

use super::role::apply_role_to_config;
use crate::config::Config;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelsResponse;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use toml::Value as TomlValue;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExternalModelRoute {
    NotMatched,
    Applied { role_name: Option<String> },
}

/// Routes a provider-prefixed model to that configured external provider.
///
/// When possible, this applies a personal role for the provider so its model catalog and other
/// provider-specific settings remain available when the child is later resumed. If no such role
/// exists, the configured provider itself is still sufficient for an explicitly named model.
pub(crate) async fn apply_provider_prefix_route(
    config: &mut Config,
    requested_model: &str,
) -> Result<ExternalModelRoute, String> {
    let Some(provider_id) = matching_external_provider_id(config, requested_model) else {
        return Ok(ExternalModelRoute::NotMatched);
    };

    if config.model_provider_id == provider_id {
        return Ok(ExternalModelRoute::Applied { role_name: None });
    }

    let mut selected_role: Option<(u8, String, Config)> = None;
    for role_name in config.agent_roles.keys() {
        let mut candidate = config.clone();
        if apply_role_to_config(&mut candidate, Some(role_name))
            .await
            .is_err()
            || candidate.model_provider_id != provider_id
        {
            continue;
        }

        let catalog_contains_model = candidate.model_catalog.as_ref().is_some_and(|catalog| {
            catalog
                .models
                .iter()
                .any(|model| model.slug == requested_model)
        });
        let role_model_matches = candidate.model.as_deref() == Some(requested_model);
        let score = u8::from(catalog_contains_model) * 2 + u8::from(role_model_matches);
        if selected_role
            .as_ref()
            .is_none_or(|(selected_score, _, _)| score > *selected_score)
        {
            selected_role = Some((score, role_name.clone(), candidate));
        }
    }

    if let Some((_, role_name, candidate)) = selected_role {
        *config = candidate;
        return Ok(ExternalModelRoute::Applied {
            role_name: Some(role_name),
        });
    }

    let provider = config
        .model_providers
        .get(&provider_id)
        .cloned()
        .ok_or_else(|| format!("model provider `{provider_id}` is not configured"))?;
    config.model_provider_id = provider_id;
    config.model_provider = provider;
    Ok(ExternalModelRoute::Applied { role_name: None })
}

/// Returns picker metadata from personal external-provider role catalogs.
pub(crate) fn configured_model_presets(config: &Config) -> Vec<ModelPreset> {
    let Ok(personal_agents_dir) = std::fs::canonicalize(config.codex_home.join("agents")) else {
        return Vec::new();
    };
    let mut models = BTreeMap::new();

    for role in config.agent_roles.values() {
        let Some(config_file) = role.config_file.as_ref() else {
            continue;
        };
        let Ok(canonical_config_file) = std::fs::canonicalize(config_file) else {
            continue;
        };
        if !canonical_config_file.starts_with(&personal_agents_dir) {
            continue;
        }
        let Some((provider_id, catalog_path)) = external_catalog_from_role(&canonical_config_file)
        else {
            continue;
        };
        if provider_id == codex_model_provider_info::OPENAI_PROVIDER_ID
            || !config.model_providers.contains_key(&provider_id)
        {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(catalog_path) else {
            continue;
        };
        let Ok(catalog) = serde_json::from_str::<ModelsResponse>(&contents) else {
            continue;
        };
        for model in catalog
            .models
            .into_iter()
            .filter(|model| model.slug.starts_with(&provider_id))
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

fn external_catalog_from_role(config_file: &Path) -> Option<(String, PathBuf)> {
    let contents = std::fs::read_to_string(config_file).ok()?;
    let role: TomlValue = toml::from_str(&contents).ok()?;
    let provider_id = role.get("model_provider")?.as_str()?.to_string();
    let catalog_path = PathBuf::from(role.get("model_catalog_json")?.as_str()?);
    let catalog_path = if catalog_path.is_absolute() {
        catalog_path
    } else {
        config_file.parent()?.join(catalog_path)
    };
    Some((provider_id, catalog_path))
}
