use uuid::Uuid;

use crate::ai::llm::{ChatMessage, LlmClient};
use crate::db::Database;
use std::path::{Path, PathBuf};

/// Manages conversation context: auto-pruning, summarization, and user profiling.
pub struct ContextManager {
    max_tokens: usize,
}

impl ContextManager {
    pub fn new(max_tokens: usize) -> Self {
        Self { max_tokens }
    }

    /// Check if the conversation has exceeded the token limit, and prune if needed.
    /// Returns true if pruning occurred.
    pub async fn check_and_prune(
        &self,
        db: &Database,
        llm: &LlmClient,
        conversation_id: Uuid,
    ) -> anyhow::Result<bool> {
        let total_tokens = db.get_total_tokens(conversation_id).await?;

        if (total_tokens as usize) < self.max_tokens {
            return Ok(false);
        }

        tracing::info!(
            "Context limit reached ({}/{}) for conv {}. Summarizing...",
            total_tokens,
            self.max_tokens,
            conversation_id
        );

        // Get all messages
        let messages: Vec<crate::db::models::Message> = db.get_messages(conversation_id).await?;
        if messages.len() <= 4 {
            // Too few messages to prune meaningfully
            return Ok(false);
        }

        // Take the first half of messages to summarize
        let half = messages.len() / 2;
        let to_summarize: Vec<&_> = messages[..half].iter().collect();

        // Build summarization prompt
        let mut summary_text = String::new();
        for msg in &to_summarize {
            summary_text.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        let summary_prompt = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "Summarize the following conversation into a concise paragraph. \
                          Preserve key facts, decisions, and any important user information."
                    .to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: summary_text,
            },
        ];

        let response = llm.chat(&summary_prompt).await?;
        let summary = response.text;

        // Delete the oldest messages
        let keep_count = (messages.len() - half) as i64;
        let deleted = db
            .delete_oldest_messages(conversation_id, keep_count)
            .await?;
        tracing::info!("Deleted {} old messages from conv {}", deleted, conversation_id);

        // Insert the summary as a "system" message at the start
        let token_count = LlmClient::estimate_tokens(&summary);
        db.save_message(
            conversation_id,
            "system",
            &format!("[Previous conversation summary]: {}", summary),
            token_count,
        )
        .await?;

        // Also update the conversation's global summary
        db.update_conversation_summary(conversation_id, &summary)
            .await?;

        Ok(true)
    }

    /// Analyze recent messages and update the user's profile if new info is found.
    /// This is called periodically (e.g., every 10 messages) by the LLM itself.
    pub async fn maybe_update_profile(
        &self,
        db: &Database,
        llm: &LlmClient,
        user_id: i64,
        conversation_id: Uuid,
    ) -> anyhow::Result<()> {
        let user = db.get_or_create_user(user_id, None).await?;
        let messages: Vec<crate::db::models::Message> = db.get_messages(conversation_id).await?;

        // Only analyze last 10 messages
        let recent: Vec<&_> = messages.iter().rev().take(10).collect();
        if recent.is_empty() {
            return Ok(());
        }

        let mut conversation_text = String::new();
        for msg in recent.iter().rev() {
            conversation_text.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        let prompt = vec![
            ChatMessage {
                role: "system".to_string(),
                content: format!(
                    "You are a profile updater. Given the current user profile and recent conversation, \
                     extract any NEW persistent facts about the user (name, preferences, demographics, \
                     interests, profession, etc.) and return an UPDATED profile summary.\n\n\
                     Current profile:\n{}\n\n\
                     If nothing new is found, respond with exactly: NO_UPDATE",
                    if user.profile_summary.is_empty() {
                        "(empty — no info yet)".to_string()
                    } else {
                        user.profile_summary.clone()
                    }
                ),
            },
            ChatMessage {
                role: "user".to_string(),
                content: conversation_text,
            },
        ];

        let response = llm.chat(&prompt).await?;

        if !response.text.contains("NO_UPDATE") && !response.text.is_empty() {
            db.update_user_profile(user_id, &response.text).await?;
            tracing::info!("Updated profile for user {}: {}", user_id, &response.text[..80.min(response.text.len())]);
        }

        Ok(())
    }
}
