//! Selects persistent root threads for managed daemon recovery using normal resume semantics.
//! Only threads successfully persisted through the existing thread store become candidates.

use super::ThreadRequestProcessor;
use codex_app_server_transport::daemon_recovery::InterruptedTurn;
use codex_app_server_transport::daemon_recovery::RecoverySnapshot;
use codex_app_server_transport::daemon_recovery::ThreadRecoveryConfig;
use codex_config::ConfigLayerSource;
use codex_core::config::Config;
use codex_thread_store::PersistContext;
use tracing::warn;

impl ThreadRequestProcessor {
    #[expect(
        clippy::await_holding_invalid_type,
        reason = "snapshot selection must be serialized against pending unloads"
    )]
    pub(crate) async fn daemon_recovery_snapshot(&self) -> RecoverySnapshot {
        let mut snapshot = RecoverySnapshot::default();
        for thread_id in self.thread_manager.list_thread_ids().await {
            let pending_unloads = self.pending_thread_unloads.lock().await;
            if pending_unloads.contains_key(&thread_id) {
                continue;
            }
            let Ok(thread) = self.thread_manager.get_thread(thread_id).await else {
                continue;
            };
            let config = thread.config_snapshot().await;
            if !config.ephemeral
                && config.parent_thread_id.is_none()
                && !config.session_source.is_non_root_agent()
            {
                let interrupted = thread.interrupted_turn().await;
                if let Err(err) = self
                    .thread_store
                    .persist_thread(thread_id, PersistContext::Standard)
                    .await
                {
                    warn!(%thread_id, %err, "skipping daemon restore for thread that could not be persisted");
                    continue;
                }
                let thread_config = thread.config().await;
                let (Ok(mut overrides), Ok(baseline)) =
                    (session_flags(&thread_config), session_flags(&self.config))
                else {
                    // Restore only when the private settings can accompany the thread.
                    // The encoding error can contain private values, so do not log it.
                    warn!(%thread_id, "skipping daemon restore for thread whose private configuration could not be encoded");
                    continue;
                };
                snapshot.loaded.insert(thread_id.to_string());
                overrides.retain(|key, value| baseline.get(key) != Some(value));
                let extensions = thread
                    .client_mcp_extensions()
                    .iter()
                    .map(|(name, value)| (name.to_owned(), value.clone()))
                    .collect::<std::collections::HashMap<_, _>>();
                if !overrides.is_empty() || !extensions.is_empty() {
                    snapshot.configs.insert(
                        thread_id.to_string(),
                        ThreadRecoveryConfig {
                            overrides,
                            extensions,
                        },
                    );
                }
                if let Some((turn_id, options, environment)) = interrupted {
                    snapshot.interrupted.insert(
                        thread_id.to_string(),
                        InterruptedTurn {
                            turn_id,
                            output_schema: options.final_output_json_schema,
                            service_tier: options.service_tier,
                            cyber_access_program: options.cyber_access_program,
                            local_environment: Some((&environment).into()),
                        },
                    );
                }
            }
        }
        snapshot
    }
}

fn session_flags(
    config: &Config,
) -> Result<std::collections::HashMap<String, serde_json::Value>, serde_json::Error> {
    let mut flags = toml::Value::Table(Default::default());
    for layer in config.config_layer_stack.layers_low_to_high() {
        if matches!(layer.name, ConfigLayerSource::SessionFlags) {
            codex_config::merge_toml_values(&mut flags, &layer.config);
        }
    }
    // Config-layer TOML values have JSON representations, including datetime strings.
    serde_json::from_value(serde_json::to_value(flags)?)
}
