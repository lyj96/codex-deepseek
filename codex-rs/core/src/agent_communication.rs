//! Direct-message diagnostics and delivery ownership across queues and eviction.
//! Only history commitment consumes mail; dropping its last owner releases its attachments.

use codex_extension_api::ExtensionData;
use codex_extension_api::MessageLifecycleContributor;
use codex_extension_api::MessageLifecycleInput;
use codex_protocol::ResponseItemId;
use codex_protocol::ThreadId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::InterAgentCommunication;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

pub(crate) static PENDING_MAILBOX_MESSAGES: codex_diagnostics::Gauge =
    codex_diagnostics::Gauge::new("core.mailbox.pending");

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
    pub(crate) sender_thread_id: ThreadId,
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
        encrypted_content: None,
        status: None,
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
            /*trigger_turn*/ true,
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

/// Pending mail with shared ownership of its captured publication state.
/// Serialization preserves the protocol shape; restored historical mail is not republished.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PendingAgentMessage {
    communication: InterAgentCommunication,
    #[serde(skip)]
    pub(crate) delivery: Option<MessageDelivery>,
}

impl From<InterAgentCommunication> for PendingAgentMessage {
    fn from(communication: InterAgentCommunication) -> Self {
        Self::new(communication, /*delivery*/ None)
    }
}

impl Deref for PendingAgentMessage {
    type Target = InterAgentCommunication;
    fn deref(&self) -> &Self::Target {
        &self.communication
    }
}

impl PartialEq for PendingAgentMessage {
    fn eq(&self, other: &Self) -> bool {
        self.communication == other.communication
    }
}

impl PendingAgentMessage {
    pub(crate) fn new(
        communication: InterAgentCommunication,
        delivery: Option<MessageDelivery>,
    ) -> Self {
        Self {
            communication,
            delivery,
        }
    }
}

/// The publication state retained until this delivery is consumed or abandoned.
struct MessagePublication {
    input: MessageLifecycleInput,
    observers: Box<[Arc<dyn MessageLifecycleContributor>]>,
}

/// Clones follow one logical delivery through queues and share its consumption state.
#[derive(Clone)]
pub(crate) struct MessageDelivery(Arc<Mutex<Option<MessagePublication>>>);

impl MessageDelivery {
    pub(crate) fn publish(
        sender: ThreadId,
        message_id: ResponseItemId,
        observers: &[Arc<dyn MessageLifecycleContributor>],
    ) -> Self {
        let input = MessageLifecycleInput {
            sender,
            data: ExtensionData::new(message_id.to_string()),
            message_id,
        };
        for observer in observers {
            observer.on_publish(&input);
        }
        let publication = MessagePublication {
            input,
            observers: observers.into(),
        };
        Self(Arc::new(Mutex::new(Some(publication))))
    }

    /// Commit this delivery and release its publication state, even if clones remain.
    pub(crate) fn consumed(&self, recipient: ThreadId) {
        let publication = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(publication) = publication {
            for observer in &publication.observers {
                observer.on_consumed(&publication.input, recipient);
            }
        }
    }
}

impl fmt::Debug for MessageDelivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessageDelivery").finish_non_exhaustive()
    }
}
