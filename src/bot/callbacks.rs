use std::sync::Arc;
use teloxide::prelude::*;
use uuid::Uuid;

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

        let display = crate::ai::tts::TtsEngine::from_str_loose(engine).display_name();
        bot.answer_callback_query(&q.id)
            .text(format!("TTS set to: {}", display))
            .await?;

        return Ok(());
    }

    // ── Conversation Selection ─────────────────────────────────────
    if let Some(conv_id_str) = data.strip_prefix("conv:") {
        if let Ok(conv_id) = Uuid::parse_str(conv_id_str) {
            let mut settings = state.db.get_user_settings(user_id).await?;
            settings["active_conversation"] = serde_json::json!(conv_id.to_string());
            state.db.update_user_settings(user_id, &settings).await?;

            bot.answer_callback_query(&q.id)
                .text("Conversation loaded!")
                .await?;

            // Send the last few messages as context
            let messages = state.db.get_messages(conv_id).await?;
            let last_msgs: Vec<_> = messages.iter().rev().take(5).collect();
            if !last_msgs.is_empty() {
                let mut recap = String::from("📖 Last messages:\n\n");
                for msg in last_msgs.iter().rev() {
                    let role_emoji = match msg.role.as_str() {
                        "user" => "👤",
                        "assistant" => "🤖",
                        _ => "📝",
                    };
                    let preview: String = msg.content.chars().take(100).collect();
                    recap.push_str(&format!("{} {}\n", role_emoji, preview));
                }
                if let Some(chat_msg) = q.message {
                    bot.send_message(chat_msg.chat().id, recap).await?;
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
