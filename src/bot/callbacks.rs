use std::sync::Arc;
use teloxide::prelude::*;
use uuid::Uuid;

use crate::ai::llm::{ChatMessage, LlmClient};
use crate::bot::AppState;

pub async fn handle_callback(
    bot: Bot,
    q: CallbackQuery,
    state: Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = match q.data.as_deref() {
        Some(d) => d,
        None => return Ok(()),
    };

    let user_id = q.from.id.0 as i64;

    // ── TTS Engine Selection ───────────────────────────────────────
    if let Some(engine) = data.strip_prefix("set_tts:") {
        let mut settings = state.db.get_user_settings(user_id).await?;
        settings["tts_engine"] = serde_json::json!(engine);
        state.db.update_user_settings(user_id, &settings).await?;

        // Reset XTTS availability cache so it gets retried
        if engine == "xtts" {
            state.tts.reset_xtts_availability();
        }

        let display = crate::ai::tts::TtsEngine::from_str_loose(engine).display_name();
        bot.answer_callback_query(&q.id)
            .text(format!("TTS set to: {}", display))
            .await?;

        return Ok(());
    }

    // ── Response Mode Selection ────────────────────────────────────
    if let Some(mode) = data.strip_prefix("set_mode:") {
        let mut settings = state.db.get_user_settings(user_id).await?;
        settings["response_mode"] = serde_json::json!(mode);
        state.db.update_user_settings(user_id, &settings).await?;

        let label = match mode {
            "text" => "🔤 Text Only",
            "voice" => "🎙 Voice Only",
            _ => "🤖 Auto",
        };
        bot.answer_callback_query(&q.id)
            .text(format!("Response mode: {}", label))
            .await?;

        return Ok(());
    }

    // ── Conversation Selection ─────────────────────────────────────
    if let Some(conv_id_str) = data.strip_prefix("conv:") {
        if let Ok(conv_id) = Uuid::parse_str(conv_id_str) {
            // Switch to the selected conversation
            let mut settings = state.db.get_user_settings(user_id).await?;
            settings["active_conversation"] = serde_json::json!(conv_id.to_string());
            state.db.update_user_settings(user_id, &settings).await?;

            bot.answer_callback_query(&q.id)
                .text("Conversation loaded!")
                .await?;

            // Show a brief summary of the conversation
            if let Some(chat_msg) = q.message {
                let messages: Vec<crate::db::models::Message> = state.db.get_messages(conv_id).await?;
                let msg_count = messages.len();

                if msg_count == 0 {
                    bot.send_message(
                        chat_msg.chat().id,
                        "📂 Switched to this conversation. It's empty — send a message to start!",
                    )
                    .await?;
                } else {
                    // Check if summary exists in DB
                    let convs: Vec<crate::db::models::Conversation> = state.db.list_conversations(user_id, 50).await?;
                    let existing_summary = convs.iter()
                        .find(|c| c.id == conv_id)
                        .map(|c| c.summary.clone())
                        .unwrap_or_default();

                    let summary_text = if existing_summary.is_empty() {
                        // Auto-generate summary from recent messages using LLM
                        match generate_conversation_summary(&state.llm, &messages).await {
                            Ok(generated) => {
                                // Save it for future use
                                let _ = state.db.update_conversation_summary(conv_id, &generated).await;
                                generated
                            }
                            Err(e) => {
                                tracing::warn!("Failed to generate summary: {}", e);
                                format!("{} messages in this conversation", msg_count)
                            }
                        }
                    } else {
                        existing_summary
                    };

                    bot.send_message(
                        chat_msg.chat().id,
                        format!(
                            "📂 Switched to conversation ({} messages)\n\n📝 {}\n\nContinue where you left off!",
                            msg_count, summary_text
                        ),
                    )
                    .await?;
                }
            }
        }
        return Ok(());
    }

    // ── Approval Callbacks ─────────────────────────────────────────
    if let Some(approval_id_str) = data.strip_prefix("approve:") {
        if let Ok(approval_id) = Uuid::parse_str(approval_id_str) {
            let result = crate::agent::approval::handle_approval_callback(
                &bot,
                &state.db,
                approval_id,
                true,
                user_id,
                &state.config.admin_ids,
            )
            .await?;
            bot.answer_callback_query(&q.id).text(&result).await?;
        }
        return Ok(());
    }

    if let Some(approval_id_str) = data.strip_prefix("deny:") {
        if let Ok(approval_id) = Uuid::parse_str(approval_id_str) {
            let result = crate::agent::approval::handle_approval_callback(
                &bot,
                &state.db,
                approval_id,
                false,
                user_id,
                &state.config.admin_ids,
            )
            .await?;
            bot.answer_callback_query(&q.id).text(&result).await?;
        }
        return Ok(());
    }

    Ok(())
}

/// Generate a brief conversation summary using the LLM.
async fn generate_conversation_summary(
    llm: &LlmClient,
    messages: &[crate::db::models::Message],
) -> anyhow::Result<String> {
    // Take last ~10 messages for summary
    let recent: Vec<&crate::db::models::Message> = messages.iter().rev().take(10).collect();

    let mut conversation_text = String::new();
    for msg in recent.iter().rev() {
        let role_label = match msg.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            _ => "System",
        };
        // Truncate very long messages
        let content: String = msg.content.chars().take(200).collect();
        conversation_text.push_str(&format!("{}: {}\n", role_label, content));
    }

    let prompt = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "Summarize this conversation in 1-2 short sentences. Be concise and capture the key topic.".to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: conversation_text,
        },
    ];

    let response = llm.chat(&prompt).await?;
    Ok(response.text.trim().to_string())
}
