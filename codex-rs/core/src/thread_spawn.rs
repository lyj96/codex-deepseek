//! Shared native MAv2 child preparation and spawning.
//!
//! Callers supply captured step settings; the configured agent backend owns child lifetimes.

use crate::agent::api::AgentInput;
use crate::agent::api::SpawnRequest;
use crate::agent::child_config::SpawnConfigOptions;
use crate::agent::child_config::SpawnConfigVersion;
use crate::agent::child_config::SpawnModelRoute;
use crate::agent::child_config::prepare_agent_spawn_config;
use crate::agent::next_thread_spawn_depth;
use crate::agent::types::AgentMessage;
use crate::agent::types::LiveAgent;
use crate::agent::types::MessageDeliveryMode;
use crate::agent::types::SpawnAgentForkMode;
use crate::agent::types::SpawnAgentOptions;
use crate::codex_thread::ThreadConfigSnapshot;
use crate::config::Config;
use crate::session::multi_agents::resolve_usage_hints;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use codex_prompts::ResolvedModelMessages;
use codex_protocol::AgentPath;
use codex_protocol::error::CodexErr;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;

/// Whether a message came from a plaintext host/tool or the parent's model transport.
pub(crate) enum ThreadSpawnMessage {
    Plaintext(String),
    ProviderEncoded(String),
}

/// Child creation options; omitted overrides retain native MAv2 inheritance and role rules.
pub struct ThreadCreateRequest {
    pub task_name: String,
    /// `None` starts fresh; `FullHistory` uses native fork preparation.
    pub fork_mode: Option<SpawnAgentForkMode>,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
}

pub(crate) struct ThreadSpawnRequest {
    pub task_name: String,
    /// Preserve parent transport encoding for the controller's recipient admission.
    pub message: ThreadSpawnMessage,
    /// `None` starts fresh; `FullHistory` uses native fork preparation.
    pub fork_mode: Option<SpawnAgentForkMode>,
    pub fork_turns_was_explicit: bool,
    pub model_route: SpawnModelRoute,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
}

impl ThreadSpawnRequest {
    pub(crate) fn into_parts(
        self,
    ) -> Result<(ThreadCreateRequest, ThreadSpawnMessage), ThreadSpawnError> {
        let message_text = match &self.message {
            ThreadSpawnMessage::Plaintext(text) | ThreadSpawnMessage::ProviderEncoded(text) => text,
        };
        if message_text.trim().is_empty() {
            return Err(ThreadSpawnError::Preparation(
                "Empty message can't be sent to an agent".to_string(),
            ));
        }
        Ok((
            ThreadCreateRequest {
                task_name: self.task_name,
                fork_mode: self.fork_mode,
                agent_type: self.agent_type,
                model: self.model,
                reasoning_effort: self.reasoning_effort,
            },
            self.message,
        ))
    }
}

pub(crate) struct ChildSpawnSettings {
    pub(crate) config: Config,
    pub(crate) source: SessionSource,
    pub(crate) options: SpawnAgentOptions,
}

/// Child runtime and its effective configuration.
pub struct ThreadSpawnResult {
    pub agent: LiveAgent,
    pub config: ThreadConfigSnapshot,
    pub task_name: AgentPath,
}

/// Separates preparation errors from backend failures to preserve native tool error reporting.
#[derive(Debug, thiserror::Error)]
pub enum ThreadSpawnError {
    #[error("{0}")]
    Preparation(String),
    #[error(transparent)]
    Backend(#[from] CodexErr),
}

pub(crate) async fn spawn(
    session: &Session,
    step_context: &StepContext,
    call_id: &str,
    request: ThreadSpawnRequest,
) -> Result<ThreadSpawnResult, ThreadSpawnError> {
    let model_route = request.model_route;
    let fork_turns_was_explicit = request.fork_turns_was_explicit;
    let (request, message) = request.into_parts()?;
    let (settings, task_name) = prepare_routed(
        session,
        step_context,
        call_id,
        request,
        model_route,
        fork_turns_was_explicit,
    )
    .await?;
    let parent_provider = &step_context.turn.config.model_provider_id;
    let message = match message {
        ThreadSpawnMessage::ProviderEncoded(text)
            if parent_provider == codex_model_provider_info::OPENAI_PROVIDER_ID =>
        {
            AgentMessage::Encrypted(text)
        }
        ThreadSpawnMessage::Plaintext(text) | ThreadSpawnMessage::ProviderEncoded(text) => {
            AgentMessage::Plaintext(text)
        }
    };
    let request = SpawnRequest {
        caller: session.thread_id,
        config: settings.config,
        input: AgentInput::Message {
            message,
            mode: MessageDeliveryMode::TriggerTurn,
        },
        source: settings.source,
        options: settings.options,
    };
    let (agent, config) = session.services.agent_control.spawn(request).await?;
    Ok(ThreadSpawnResult {
        agent,
        config,
        task_name,
    })
}

/// Prepare the native child settings without choosing its coordination backend.
pub(crate) async fn prepare(
    session: &Session,
    step_context: &StepContext,
    call_id: &str,
    request: ThreadCreateRequest,
) -> Result<(ChildSpawnSettings, AgentPath), ThreadSpawnError> {
    prepare_routed(
        session,
        step_context,
        call_id,
        request,
        SpawnModelRoute::SessionCatalog,
        /*fork_turns_was_explicit*/ true,
    )
    .await
}

async fn prepare_routed(
    session: &Session,
    step_context: &StepContext,
    call_id: &str,
    request: ThreadCreateRequest,
    model_route: SpawnModelRoute,
    fork_turns_was_explicit: bool,
) -> Result<(ChildSpawnSettings, AgentPath), ThreadSpawnError> {
    let turn = &step_context.turn;
    let ThreadCreateRequest {
        task_name,
        mut fork_mode,
        agent_type,
        model,
        reasoning_effort,
    } = request;
    let role_name = agent_type
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty());
    let prepared = prepare_agent_spawn_config(
        session,
        step_context,
        SpawnConfigOptions {
            version: SpawnConfigVersion::V2,
            full_history_fork: matches!(fork_mode, Some(SpawnAgentForkMode::FullHistory)),
            role_name,
            model: model.as_deref(),
            reasoning_effort,
            model_route,
        },
    )
    .await
    .map_err(ThreadSpawnError::Preparation)?;
    let config = prepared.config;
    let crosses_model_providers = config.model_provider_id != turn.config.model_provider_id;
    if turn.config.ephemeral && fork_mode.is_some() {
        return Err(ThreadSpawnError::Preparation(
            "Forked subagents require persisted parent history; use `fork_turns=\"none\"` or run without `--ephemeral`."
                .to_string(),
        ));
    }
    if crosses_model_providers
        && turn.config.model_provider_id == codex_model_provider_info::OPENAI_PROVIDER_ID
        && fork_mode.is_some()
    {
        if fork_turns_was_explicit {
            return Err(ThreadSpawnError::Preparation(
                "OpenAI-backed agents require `fork_turns` to be `none` when spawning across providers because encrypted history cannot be forwarded; put the needed context in `message`."
                    .to_string(),
            ));
        }
        fork_mode = None;
    }
    let task_name = turn
        .session_source
        .get_agent_path()
        .unwrap_or_else(AgentPath::root)
        .join(&task_name)
        .map_err(ThreadSpawnError::Preparation)?;
    let spawn_source = SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id: session.thread_id,
        depth: next_thread_spawn_depth(&turn.session_source),
        agent_path: Some(task_name.clone()),
        agent_nickname: None,
        agent_role: prepared.role_name,
    });
    let multi_agent_v2_usage_hints = if matches!(fork_mode, Some(SpawnAgentForkMode::FullHistory))
        && turn.multi_agent_version == MultiAgentVersion::V2
    {
        let child_model_info = match config.model.as_deref() {
            Some(model) if model != turn.model_info().slug => Some(
                session
                    .services
                    .models_manager
                    .get_model_info(model, &config.to_models_manager_config())
                    .await,
            ),
            _ => None,
        };
        let child_multi_agent_messages = ResolvedModelMessages::from_model(
            child_model_info.as_ref().unwrap_or(turn.model_info()),
        )
        .multi_agent();
        Some(resolve_usage_hints(
            &config.multi_agent_v2,
            child_multi_agent_messages,
            !config.update_plan_enabled && config.model_catalog.is_none(),
        ))
    } else {
        None
    };
    Ok((
        ChildSpawnSettings {
            config,
            source: spawn_source,
            options: SpawnAgentOptions {
                fork_parent_spawn_call_id: fork_mode.as_ref().map(|_| call_id.to_string()),
                fork_mode,
                parent_thread_id: Some(session.thread_id),
                parent_turn_id: Some(turn.sub_id.clone()),
                root_turn_id: turn.turn_metadata_state.root_turn_id(),
                turn_trigger: turn.turn_metadata_state.current_turn_trigger(),
                environments: Some(step_context.inherited_environments.clone()),
                disabled_plugin_ids: Some(turn.disabled_plugin_ids.clone()),
                multi_agent_v2_usage_hints,
                cyber_access_program: turn.cyber_access_program,
            },
        },
        task_name,
    ))
}

#[cfg(test)]
#[path = "thread_spawn_tests.rs"]
mod tests;
