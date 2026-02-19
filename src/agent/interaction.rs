use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::config::InteractionAgentConfig;
use crate::llm::LlmProvider;

use super::SimpleAgent;

/// Classification of the type of input the CLI is waiting for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputType {
    Permission,
    Clarification,
    PlanReview,
    Confirmation,
    FreeformInput,
}

impl std::fmt::Display for InputType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputType::Permission => write!(f, "permission"),
            InputType::Clarification => write!(f, "clarification"),
            InputType::PlanReview => write!(f, "plan review"),
            InputType::Confirmation => write!(f, "confirmation"),
            InputType::FreeformInput => write!(f, "input"),
        }
    }
}

/// A request from the interaction agent to the router, indicating the CLI
/// needs user feedback.
#[derive(Debug, Clone)]
pub struct FeedbackRequest {
    pub session_id: u64,
    pub request_id: String,
    pub question: String,
    pub raw_context: String,
    pub input_type: InputType,
}

/// A response from the user, formatted for CLI input.
#[derive(Debug, Clone)]
pub struct FeedbackResponse {
    pub request_id: String,
    pub user_message: String,
    pub formatted_input: String,
}

/// Data needed to perform an LLM classification call outside the lock.
pub struct PendingClassify {
    pub agent: Arc<SimpleAgent>,
    pub system: String,
    pub prompt: String,
    pub context: String,
    pub buffer: String,
}

/// LLM classification response.
#[derive(Deserialize)]
struct ClassifyResult {
    needs_input: bool,
    #[serde(default)]
    question: String,
    #[serde(default = "default_input_type")]
    input_type: String,
}

fn default_input_type() -> String {
    "freeform".to_string()
}

fn parse_input_type(s: &str) -> InputType {
    match s {
        "permission" => InputType::Permission,
        "clarification" => InputType::Clarification,
        "plan_review" => InputType::PlanReview,
        "confirmation" => InputType::Confirmation,
        _ => InputType::FreeformInput,
    }
}

/// The InteractionAgent monitors CLI output and detects when the CLI
/// is waiting for user input. It uses an LLM to classify output and
/// format user responses.
pub struct InteractionAgent {
    agent: Arc<SimpleAgent>,
    quiet_timeout: Duration,
    /// Accumulated output since last classification check.
    output_buffer: String,
    /// Timestamp of last received output.
    last_output_time: Option<Instant>,
    /// Channel to send feedback requests to the router.
    feedback_tx: mpsc::Sender<FeedbackRequest>,
    /// Whether we're currently waiting for user feedback.
    awaiting_feedback: bool,
    /// Whether an LLM classification call is in-flight (prevents overlapping calls).
    classifying: bool,
    /// Session ID for this agent.
    session_id: u64,
    /// Tool label for context ("Claude Code" or "Gemini CLI").
    tool_label: String,
}

impl InteractionAgent {
    /// Create a new InteractionAgent if the config is valid (enabled + API key present).
    pub fn try_new(
        config: &InteractionAgentConfig,
        session_id: u64,
        tool_label: &str,
        feedback_tx: mpsc::Sender<FeedbackRequest>,
    ) -> Option<Self> {
        if !config.enabled {
            tracing::debug!("Interaction agent disabled by config");
            return None;
        }

        let llm = match LlmProvider::from_config(&config.llm) {
            Ok(llm) => llm,
            Err(e) => {
                tracing::warn!("Interaction agent enabled but LLM provider failed: {}", e);
                return None;
            }
        };

        let model = config.llm.resolved_model();
        let provider = &config.llm.provider;

        tracing::info!(
            "Interaction agent enabled for session #{} (provider: {}, model: {})",
            session_id,
            provider,
            model,
        );

        Some(Self {
            agent: Arc::new(SimpleAgent::new("interaction".to_string(), llm)),
            quiet_timeout: Duration::from_millis(config.quiet_timeout_ms),
            output_buffer: String::new(),
            last_output_time: None,
            feedback_tx,
            awaiting_feedback: false,
            classifying: false,
            session_id,
            tool_label: tool_label.to_string(),
        })
    }

    /// Process a chunk of PTY output. Returns the text to forward downstream
    /// (or None if output should be suppressed while awaiting feedback).
    pub fn process_output(&mut self, text: &str) -> Option<String> {
        if self.awaiting_feedback {
            // While waiting for feedback, accumulate but don't forward
            self.output_buffer.push_str(text);
            return None;
        }

        self.output_buffer.push_str(text);
        self.last_output_time = Some(Instant::now());

        // Forward text immediately so the user sees output in real-time
        Some(text.to_string())
    }

    /// Check if the output has been quiet long enough to warrant classification.
    /// Returns a `PendingClassify` with everything needed to make the LLM call
    /// outside the lock. Returns None if no classification is needed.
    pub fn prepare_tick(&mut self) -> Option<PendingClassify> {
        if self.awaiting_feedback || self.classifying {
            return None;
        }

        let last = self.last_output_time?;
        if last.elapsed() < self.quiet_timeout || self.output_buffer.is_empty() {
            return None;
        }

        let buffer = self.output_buffer.clone();

        // If no heuristic match, skip classification
        if !Self::heuristic_match(&buffer) {
            self.output_buffer.clear();
            self.last_output_time = None;
            return None;
        }

        // Truncate context for the LLM to last ~2000 chars
        let context = if buffer.len() > 2000 {
            buffer[buffer.len() - 2000..].to_string()
        } else {
            buffer.clone()
        };

        let system = format!(
            "You monitor terminal output from {}. \
             Determine if the tool is waiting for user input. \
             Respond with JSON only: \
             {{\"needs_input\": bool, \"question\": \"extracted question for the user\", \
             \"input_type\": \"permission|clarification|plan_review|confirmation|freeform\"}} \
             \
             If the output is just normal text/code output and not waiting for input, \
             set needs_input to false. Only set needs_input to true if the tool is clearly \
             paused and waiting for the user to type something.",
            self.tool_label
        );

        let prompt = format!("Terminal output:\n```\n{}\n```", context);

        self.classifying = true;

        Some(PendingClassify {
            agent: self.agent.clone(),
            system,
            prompt,
            context,
            buffer,
        })
    }

    /// Apply the result of an LLM classification call. Called after the LLM
    /// call completes, with the lock re-acquired.
    pub fn complete_classify(&mut self, pending: &PendingClassify, llm_result: Result<String>) {
        self.classifying = false;

        match llm_result {
            Ok(response) => {
                let result = Self::parse_classify_response(&response);
                match result {
                    Some(classify) if classify.needs_input => {
                        let request_id = uuid::Uuid::new_v4().to_string();
                        let question = if classify.question.is_empty() {
                            pending
                                .buffer
                                .lines()
                                .rev()
                                .take(3)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .collect::<Vec<_>>()
                                .join("\n")
                        } else {
                            classify.question
                        };

                        let feedback = FeedbackRequest {
                            session_id: self.session_id,
                            request_id,
                            question,
                            raw_context: pending.context.clone(),
                            input_type: parse_input_type(&classify.input_type),
                        };

                        tracing::info!(
                            "Session #{} needs {} input: {}",
                            self.session_id,
                            feedback.input_type,
                            feedback.question
                        );

                        if let Err(e) = self.feedback_tx.try_send(feedback) {
                            tracing::warn!(
                                "Failed to send feedback request for session #{}: {}",
                                self.session_id,
                                e
                            );
                        }
                        self.awaiting_feedback = true;
                    }
                    _ => {
                        tracing::trace!(
                            "Session #{} classified as not waiting for input",
                            self.session_id
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Interaction agent LLM call failed for session #{}: {}",
                    self.session_id,
                    e
                );
            }
        }

        self.output_buffer.clear();
        self.last_output_time = None;
    }

    /// Heuristic pre-filter: does the output look like it might be waiting for input?
    fn heuristic_match(text: &str) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return false;
        }

        // Check last line for prompt indicators
        let last_line = trimmed.lines().last().unwrap_or("");
        let last_trimmed = last_line.trim();

        if last_trimmed.ends_with('?')
            || last_trimmed.ends_with("[y/n]")
            || last_trimmed.ends_with("[Y/n]")
            || last_trimmed.ends_with("[y/N]")
            || last_trimmed.ends_with("[Y/N]")
            || last_trimmed.ends_with(':')
            || last_trimmed.ends_with('>')
        {
            return true;
        }

        // Check for common interactive prompt keywords anywhere in buffer
        let lower = trimmed.to_lowercase();
        if lower.contains("allow")
            || lower.contains("refuse")
            || lower.contains("approve")
            || lower.contains("permission")
            || lower.contains("proceed")
            || lower.contains("(y/n)")
            || lower.contains("yes/no")
            || lower.contains("do you want")
            || lower.contains("would you like")
            || lower.contains("should i")
        {
            return true;
        }

        false
    }

    /// Parse the LLM classify response, handling potential non-JSON wrapping.
    fn parse_classify_response(response: &str) -> Option<ClassifyResult> {
        // Try direct parse
        if let Ok(result) = serde_json::from_str::<ClassifyResult>(response) {
            return Some(result);
        }

        // Try to extract JSON from markdown code block
        if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                let json_str = &response[start..=end];
                if let Ok(result) = serde_json::from_str::<ClassifyResult>(json_str) {
                    return Some(result);
                }
            }
        }

        tracing::warn!(
            "Failed to parse classify response: {}",
            &response[..response.len().min(200)]
        );
        None
    }

    /// Format a user's response for the CLI using the LLM.
    pub async fn format_response(
        &self,
        raw_context: &str,
        user_message: &str,
    ) -> Result<String> {
        let system = format!(
            "{} is showing this in its terminal:\n\
             ```\n{}\n```\n\n\
             The user responded via chat: {}\n\n\
             What exact text should be typed into the terminal? Consider the prompt type.\n\
             For yes/no prompts: respond with just \"y\" or \"n\".\n\
             For option selection (e.g., Refuse/Once/Always): respond with the option word.\n\
             For text input: respond with the user's message as-is.\n\
             Return ONLY the text to type, nothing else. No quotes, no explanation.",
            self.tool_label, raw_context, user_message
        );

        let result = self.agent.generate(&system, "What should be typed?").await?;
        Ok(result.trim().to_string())
    }

    /// Clear the awaiting feedback state, allowing output processing to resume.
    pub fn clear_awaiting_feedback(&mut self) {
        self.awaiting_feedback = false;
        self.output_buffer.clear();
        self.last_output_time = None;
    }

    pub fn is_awaiting_feedback(&self) -> bool {
        self.awaiting_feedback
    }

    /// How long since the last output was received.
    pub fn last_output_time_elapsed(&self) -> Option<Duration> {
        self.last_output_time.map(|t| t.elapsed())
    }

    /// The configured quiet timeout.
    pub fn quiet_timeout(&self) -> Duration {
        self.quiet_timeout
    }

    /// Whether the output buffer has content.
    pub fn has_buffered_output(&self) -> bool {
        !self.output_buffer.is_empty()
    }
}
