use codex_protocol::ThreadId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::InterAgentCommunication;

const AGENT_COMMUNICATION_TARGET: &str = "codex_otel.agent_communication";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentCommunicationKind {
    Spawn,
    Message,
    Followup,
    Result,
}

impl AgentCommunicationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Spawn => "spawn",
            Self::Message => "message",
            Self::Followup => "followup",
            Self::Result => "result",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentCommunicationContext {
    kind: AgentCommunicationKind,
    sender_thread_id: ThreadId,
}

impl AgentCommunicationContext {
    pub(crate) fn new(kind: AgentCommunicationKind, sender_thread_id: ThreadId) -> Self {
        Self {
            kind,
            sender_thread_id,
        }
    }
}

pub(crate) fn logging_enabled() -> bool {
    tracing::enabled!(target: AGENT_COMMUNICATION_TARGET, tracing::Level::INFO)
}

pub(crate) fn emit_agent_communication_send(
    communication_id: &str,
    context: &AgentCommunicationContext,
    communication: &InterAgentCommunication,
    receiver_thread_id: ThreadId,
) {
    tracing::info!(
        target: AGENT_COMMUNICATION_TARGET,
        {
            event.name = "codex.agent_communication",
            communication_id,
            kind = context.kind.as_str(),
            state = "send",
            sender_thread_id = %context.sender_thread_id,
            receiver_thread_id = %receiver_thread_id,
            content = communication
                .encrypted_content
                .as_deref()
                .unwrap_or("[plaintext]"),
        },
        "agent communication"
    );
}

pub(crate) fn emit_agent_communication_receive(communication_id: &str) {
    tracing::info!(
        target: AGENT_COMMUNICATION_TARGET,
        {
            event.name = "codex.agent_communication",
            communication_id,
            state = "receive",
        },
        "agent communication"
    );
}

/// Converts a queued inter-agent communication into input accepted by the recipient provider.
/// OpenAI understands the richer `agent_message` item, including encrypted payloads. Other
/// Responses-compatible providers receive an ordinary user message and never see opaque OpenAI
/// encrypted content.
pub(crate) fn model_input_item_for_provider(
    communication: &InterAgentCommunication,
    recipient_provider_id: &str,
) -> ResponseItem {
    if recipient_provider_id == codex_model_provider_info::OPENAI_PROVIDER_ID {
        return communication.to_model_input_item();
    }

    let text = if communication.content.trim().is_empty() {
        let message_type = if communication.trigger_turn {
            "NEW_TASK"
        } else {
            "MESSAGE"
        };
        format!(
            "Message Type: {message_type}\nTask name: {}\nSender: {}\nPayload:\n[Unavailable encrypted cross-provider payload]",
            communication.recipient, communication.author
        )
    } else {
        communication.content.clone()
    };
    ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText { text }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::AgentPath;
    use codex_protocol::models::AgentMessageInputContent;

    fn plaintext_communication() -> InterAgentCommunication {
        InterAgentCommunication::new(
            AgentPath::root(),
            AgentPath::root().join("worker").expect("valid agent path"),
            Vec::new(),
            "Message Type: NEW_TASK\nTask name: /root/worker\nSender: /root\nPayload:\ninspect"
                .to_string(),
            true,
        )
    }

    #[test]
    fn external_provider_receives_standard_user_message() {
        let item = model_input_item_for_provider(&plaintext_communication(), "deepseek");
        assert!(matches!(
            item,
            ResponseItem::Message {
                role,
                content,
                ..
            } if role == "user"
                && matches!(content.as_slice(), [ContentItem::InputText { text }] if text.ends_with("inspect"))
        ));
    }

    #[test]
    fn openai_provider_keeps_agent_message() {
        let item = model_input_item_for_provider(&plaintext_communication(), "openai");
        assert!(matches!(
            item,
            ResponseItem::AgentMessage { content, .. }
                if matches!(content.as_slice(), [AgentMessageInputContent::InputText { text }] if text.ends_with("inspect"))
        ));
    }
}
