//! Admits a retained route from the configured model directory without changing its identity.

use codex_app_server_protocol::ModelProviderError;
use codex_core::config::Config;
use codex_core::config::ModelProviderRequirementError;
use codex_model_provider_info::OPENAI_PROVIDER_ID;
use std::collections::BTreeSet;
use std::io;

pub(super) struct ProviderAdmission<'a> {
    configured: &'a Config,
    catalog_providers: BTreeSet<String>,
}

impl<'a> ProviderAdmission<'a> {
    pub(super) fn new(configured: &'a Config) -> Self {
        let catalog_providers = configured
            .configured_external_models()
            .into_iter()
            .map(|model| model.provider_id)
            .collect();
        Self {
            configured,
            catalog_providers,
        }
    }

    pub(super) fn check_selection(&self, provider: &str) -> io::Result<()> {
        if !self.configured.model_providers.contains_key(provider) {
            return Err(not_configured(provider));
        }
        if provider != self.configured.model_provider_id
            && !self.catalog_providers.contains(provider)
            && (provider != OPENAI_PROVIDER_ID || self.catalog_providers.is_empty())
        {
            return Err(mismatch(provider, &self.configured.model_provider_id));
        }
        Ok(())
    }

    pub(super) fn check_config(&self, retained: &Config) -> io::Result<()> {
        self.check_selection(&retained.model_provider_id)?;
        let catalog_route = self.catalog_providers.contains(&retained.model_provider_id)
            || (retained.model_provider_id == OPENAI_PROVIDER_ID
                && !self.catalog_providers.is_empty());
        if catalog_route
            && self
                .configured
                .model_providers
                .get(&retained.model_provider_id)
                != Some(&retained.model_provider)
        {
            // Provider identity includes its configured endpoint and auth definition.
            // An existing chat must not silently adopt a replacement definition.
            return Err(not_configured(&retained.model_provider_id));
        }
        Ok(())
    }
}

pub(super) fn mismatch(retained: &str, selected: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        ModelProviderError::ModelProviderMismatch {
            model_provider: retained.to_owned(),
            current_model_provider: selected.to_owned(),
        },
    )
}

fn not_configured(provider: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        ModelProviderError::ModelProviderNotConfigured {
            model_provider: provider.to_owned(),
        },
    )
}

pub(super) fn map_config_error(error: io::Error) -> io::Error {
    let Some(conflict) = error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<ModelProviderRequirementError>())
    else {
        return error;
    };
    io::Error::new(
        error.kind(),
        ModelProviderError::ModelProviderPolicyConflict {
            model_provider: conflict.model_provider.clone(),
            required_model_provider: conflict.required_model_provider.clone(),
        },
    )
}
