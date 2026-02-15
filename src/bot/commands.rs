use std::sync::Arc;
use teloxide::macros::BotCommands;
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands as _;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

use crate::ai::tts::TtsEngine;
use crate::bot::AppState;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
pub enum BotCommand {
    #[command(description = "Start / restart the bot")]
    Start,
    #[command(description = "Start a new conversation")]
    New,
    #[command(description = "List recent conversations")]
    History,
    #[command(description = "Open settings menu")]
    Settings,
    #[command(description = "Show token & context usage")]
    Usage,
    #[command(description = "Change model (admin only)")]
    Model(String),
    #[command(description = "Show help")]
    Help,
}

pub async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: BotCommand,
    state: Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
    let username = msg
        .from
        .as_ref()
        .and_then(|u| u.username.as_deref());

    // Ensure user exists in DB
    state.db.get_or_create_user(user_id, username).await?;

    match cmd {
        BotCommand::Start => {
            bot.send_message(
                msg.chat.id,
                "👋 Hey! I'm your AI assistant.\n\n\
                 🎙 Send me a voice message or just type.\n\
                 Use /new to start a fresh conversation.\n\
                 Use /settings to configure TTS and response mode.\n\
                 Use /usage to check context usage.\n\
                 Use /help for all commands.",
            )
            .await?;
        }

        BotCommand::New => {
            let conv = state.db.create_conversation(user_id).await?;

            // Store active conversation ID in user settings
            let mut settings = state.db.get_user_settings(user_id).await?;
            settings["active_conversation"] = serde_json::json!(conv.id.to_string());
            state.db.update_user_settings(user_id, &settings).await?;

            bot.send_message(msg.chat.id, "🆕 New conversation started!")
                .await?;
        }

        BotCommand::History => {
            let convs: Vec<crate::db::models::Conversation> = state.db.list_conversations(user_id, 10).await?;
            if convs.is_empty() {
                bot.send_message(msg.chat.id, "No conversations yet. Send a message to start!")
                    .await?;
            } else {
                // Display times in Tashkent timezone (UTC+5)
                let tashkent = chrono::FixedOffset::east_opt(5 * 3600).unwrap();

                let mut buttons = Vec::new();
                for conv in &convs {
                    let local_time = conv.created_at.with_timezone(&tashkent);
                    let label = if conv.title == "New Chat" {
                        let preview = if conv.summary.is_empty() {
                            local_time.format("%b %d, %H:%M").to_string()
                        } else {
                            conv.summary.chars().take(40).collect::<String>()
                        };
                        preview
                    } else {
                        conv.title.chars().take(40).collect()
                    };
                    buttons.push(vec![InlineKeyboardButton::callback(
                        label,
                        format!("conv:{}", conv.id),
                    )]);
                }
                let keyboard = InlineKeyboardMarkup::new(buttons);
                bot.send_message(msg.chat.id, "📜 Recent conversations:")
                    .reply_markup(keyboard)
                    .await?;
            }
        }

        BotCommand::Settings => {
            let settings: serde_json::Value = state.db.get_user_settings(user_id).await?;
            let current_engine = settings
                .get("tts_engine")
                .and_then(|v| v.as_str())
                .unwrap_or("piper");
            let current_mode = settings
                .get("response_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("auto");

            let display_name = TtsEngine::from_str_loose(current_engine).display_name();

            let keyboard = InlineKeyboardMarkup::new(vec![
                // Row 1: TTS Engine
                vec![
                    InlineKeyboardButton::callback(
                        format!(
                            "{} Piper (Fast)",
                            if current_engine == "piper" { "✅" } else { "⬜" }
                        ),
                        "set_tts:piper",
                    ),
                    InlineKeyboardButton::callback(
                        format!(
                            "{} XTTS (Quality)",
                            if current_engine == "xtts" { "✅" } else { "⬜" }
                        ),
                        "set_tts:xtts",
                    ),
                ],
                // Row 2: Response Mode
                vec![
                    InlineKeyboardButton::callback(
                        format!(
                            "{} Text Only",
                            if current_mode == "text" { "✅" } else { "⬜" }
                        ),
                        "set_mode:text",
                    ),
                    InlineKeyboardButton::callback(
                        format!(
                            "{} Voice Only",
                            if current_mode == "voice" { "✅" } else { "⬜" }
                        ),
                        "set_mode:voice",
                    ),
                    InlineKeyboardButton::callback(
                        format!(
                            "{} Auto",
                            if current_mode == "auto" { "✅" } else { "⬜" }
                        ),
                        "set_mode:auto",
                    ),
                ],
            ]);

            bot.send_message(
                msg.chat.id,
                format!(
                    "⚙️ Settings\n\n\
                     🎵 TTS Engine: {}\n\
                     📨 Response Mode: {}\n\n\
                     Select your preferences:",
                    display_name,
                    response_mode_label(current_mode),
                ),
            )
            .reply_markup(keyboard)
            .await?;
        }

        BotCommand::Usage => {
            let settings: serde_json::Value = state.db.get_user_settings(user_id).await?;

            // Get current model
            let current_model = state.model_override.read().await;

            // Get active conversation info
            let conv_info = if let Some(conv_id_str) = settings
                .get("active_conversation")
                .and_then(|v| v.as_str())
            {
                if let Ok(conv_id) = uuid::Uuid::parse_str(conv_id_str) {
                    let total_tokens = state.db.get_total_tokens(conv_id).await.unwrap_or(0);
                    let messages: Vec<crate::db::models::Message> = state.db.get_messages(conv_id).await.unwrap_or_default();
                    let msg_count = messages.len();
                    format!(
                        "💬 Messages: {}\n🧠 Context: {} / {} tokens",
                        msg_count, total_tokens, state.config.max_context_tokens
                    )
                } else {
                    "💬 No active conversation".to_string()
                }
            } else {
                "💬 No active conversation".to_string()
            };

            let response_mode = settings
                .get("response_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("auto");
            let tts_engine = settings
                .get("tts_engine")
                .and_then(|v| v.as_str())
                .unwrap_or("piper");

            let usage_text = format!(
                "📊 Usage & Context\n\n\
                 🤖 Model: {}\n\
                 {}\n\
                 📨 Response mode: {}\n\
                 🎵 TTS engine: {}",
                current_model,
                conv_info,
                response_mode_label(response_mode),
                TtsEngine::from_str_loose(tts_engine).display_name(),
            );

            bot.send_message(msg.chat.id, usage_text).await?;
        }

        BotCommand::Model(model_name) => {
            if !state.config.is_admin(user_id) {
                bot.send_message(msg.chat.id, "❌ Only admins can change the model.")
                    .await?;
            } else if model_name.trim().is_empty() {
                let current = state.model_override.read().await;
                bot.send_message(
                    msg.chat.id,
                    format!("Current model: {}\n\nUsage: /model <model_name>", *current),
                )
                .await?;
            } else {
                let new_model = model_name.trim().to_string();
                let mut model = state.model_override.write().await;
                *model = new_model.clone();
                bot.send_message(
                    msg.chat.id,
                    format!("✅ Model changed to: {}", new_model),
                )
                .await?;
                tracing::info!("Admin {} changed model to: {}", user_id, new_model);
            }
        }

        BotCommand::Help => {
            bot.send_message(msg.chat.id, BotCommand::descriptions().to_string())
                .await?;
        }
    }

    Ok(())
}

/// Human-readable label for response mode.
fn response_mode_label(mode: &str) -> &str {
    match mode {
        "text" => "🔤 Text Only",
        "voice" => "🎙 Voice Only",
        _ => "🤖 Auto (match input)",
    }
}
