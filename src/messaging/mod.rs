#[cfg(feature = "signal")]
pub mod signal;
pub mod slack;
pub mod telegram;
pub mod web;

/// Platform identifier for message routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Signal,
    Slack,
    Telegram,
    Web,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Signal => write!(f, "Signal"),
            Platform::Slack => write!(f, "Slack"),
            Platform::Telegram => write!(f, "Telegram"),
            Platform::Web => write!(f, "Web"),
        }
    }
}

/// Identifies a conversation thread (DM, channel thread, etc).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThreadId {
    pub platform: Platform,
    pub id: String,
}

impl std::fmt::Display for ThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.platform, self.id)
    }
}

/// An incoming message from a user on any platform.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    pub thread: ThreadId,
    pub sender: String,
    pub text: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// An outgoing message to send back to a user.
#[derive(Debug, Clone)]
pub struct OutgoingMessage {
    pub thread: ThreadId,
    pub text: String,
}
