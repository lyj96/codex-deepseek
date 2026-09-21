use codex_core::config::Config;
use codex_login::AuthManager;
use codex_model_provider_info::ModelProviderInfo;

/// Keep the desktop account visible when a managed external model becomes the default.
/// This provider is used only for account metadata, never for model requests.
pub(super) fn account_provider(config: &Config, auth_manager: &AuthManager) -> ModelProviderInfo {
    let has_codex_account = auth_manager
        .auth_cached()
        .is_some_and(|auth| auth.uses_codex_backend() || auth.is_api_key_auth());
    if !config.model_provider.requires_openai_auth
        && has_codex_account
        && config
            .configured_external_models()
            .iter()
            .any(|model| model.provider_id == config.model_provider_id)
    {
        ModelProviderInfo::create_openai_provider(/*base_url*/ None)
    } else {
        config.model_provider.clone()
    }
}
