use std::sync::Arc;
use teloxide::macros::BotCommands;
use teloxide::prelude::*;
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
                 Use /settings to configure TTS engine.\n\
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
            let convs = state.db.list_conversations(user_id, 10).await?;
            if convs.is_empty() {
                bot.send_message(msg.chat.id, "No conversations yet. Send a message to start!")
                    .await?;
            } else {
                let mut buttons = Vec::new();
                for conv in &convs {
                    let label = if conv.title == "New Chat" {
                        let preview = if conv.summary.is_empty() {
                            conv.created_at.format("%b %d, %H:%M").to_string()
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
            let settings = state.db.get_user_settings(user_id).await?;
            let current_engine = settings
                .get("tts_engine")
                .and_then(|v| v.as_str())
                .unwrap_or("piper");

            let keyboard = InlineKeyboardMarkup::new(vec![
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
            ]);

            bot.send_message(
                msg.chat.id,
                format!(
                    "⚙️ *Settings*\n\nCurrent TTS: *{}*\nSelect your preferred engine:",
                    TtsEngine::from_str_loose(current_engine).display_name()
                ),
            )
            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
            .reply_markup(keyboard)
            .await?;
        }

        BotCommand::Help => {
            bot.send_message(msg.chat.id, BotCommand::descriptions().to_string())
                .await?;
        }
    }

    Ok(())
}
