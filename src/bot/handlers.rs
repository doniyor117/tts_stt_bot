use std::io::Cursor;
use std::sync::Arc;
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::InputFile;
use uuid::Uuid;

use crate::agent::context::ContextManager;
use crate::agent::executor::{CommandExecutor, ExecutionResult};
use crate::agent::identity::IdentityManager;
use crate::agent::tools::ToolRegistry;
use crate::ai::llm::{ChatMessage, LlmClient};
use crate::ai::tts::TtsEngine;
use crate::bot::AppState;

/// Main message handler for both voice and text messages.
pub async fn handle_message(
    bot: Bot,
    msg: Message,
    state: Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
    let username = msg.from.as_ref().and_then(|u| u.username.as_deref());
    let chat_id = msg.chat.id.0;

    // Ensure user exists
    let user = state.db.get_or_create_user(user_id, username).await?;

    // ── 1. Extract text (from text message or voice transcription) ──

    let user_text = if let Some(voice) = msg.voice() {
        // Download voice message
        bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing)
            .await?;

        let file = bot.get_file(&voice.file.id).await?;
        let mut buf = Vec::new();
        bot.download_file(&file.path, &mut buf).await?;

        // Convert OGG to PCM using ffmpeg
        let pcm = ogg_to_pcm(&buf).await?;

        // Transcribe
        let text = state.stt.transcribe(&pcm)?;
        tracing::info!("Transcribed voice from user {}: {}", user_id, &text);

        if text.is_empty() {
            bot.send_message(msg.chat.id, "🤔 I couldn't understand that voice message.")
                .await?;
            return Ok(());
        }

        text
    } else if let Some(text) = msg.text() {
        text.to_string()
    } else {
        // Unsupported message type
        return Ok(());
    };

    // ── 2. Get or create active conversation ───────────────────────

    let settings = state.db.get_user_settings(user_id).await?;
    let conv_id = match settings
        .get("active_conversation")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
    {
        Some(id) => id,
        None => {
            let conv = state.db.create_conversation(user_id).await?;
            let mut s = settings.clone();
            s["active_conversation"] = serde_json::json!(conv.id.to_string());
            state.db.update_user_settings(user_id, &s).await?;
            conv.id
        }
    };

    // ── 3. Save user message to DB ─────────────────────────────────

    let token_count = LlmClient::estimate_tokens(&user_text);
    state
        .db
        .save_message(conv_id, "user", &user_text, token_count)
        .await?;

    // ── 4. Check context limits and prune if needed ────────────────

    let context_mgr = ContextManager::new(state.config.max_context_tokens);
    context_mgr
        .check_and_prune(&state.db, &state.llm, conv_id)
        .await?;

    // ── 5. Build system prompt ─────────────────────────────────────

    let identity_mgr = IdentityManager::new("persona");
    let tool_registry = ToolRegistry::new();

    let system_prompt = identity_mgr
        .build_system_prompt(&user.profile_summary, &tool_registry.describe_for_prompt())
        .await?;

    // ── 6. Build message history for LLM ───────────────────────────

    let db_messages = state.db.get_messages(conv_id).await?;
    let mut llm_messages = vec![ChatMessage {
        role: "system".to_string(),
        content: system_prompt,
    }];

    for m in &db_messages {
        llm_messages.push(ChatMessage {
            role: m.role.clone(),
            content: m.content.clone(),
        });
    }

    // ── 7. Call LLM ────────────────────────────────────────────────

    bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing)
        .await?;

    let response = state.llm.chat(&llm_messages).await?;
    let mut assistant_text = response.text.clone();

    // ── 8. Check for tool calls ────────────────────────────────────

    if let Some(tool_call) = ToolRegistry::parse_tool_call(&assistant_text) {
        tracing::info!("Tool call detected: {:?}", tool_call);

        match tool_call.name.as_str() {
            "run_command" => {
                if let Some(cmd) = tool_call.arguments.get("command").and_then(|v| v.as_str()) {
                    match CommandExecutor::execute(&state.db, cmd, user_id, chat_id).await? {
                        ExecutionResult::Immediate(output) => {
                            assistant_text = format!("Command output:\n```\n{}\n```", output);
                        }
                        ExecutionResult::PendingApproval(approval_id) => {
                            // Send to admin group
                            crate::agent::approval::request_approval(
                                &bot,
                                state.config.admin_group_id,
                                cmd,
                                user_id,
                                approval_id,
                            )
                            .await?;
                            assistant_text =
                                "⏳ That command needs admin approval. I've sent the request."
                                    .to_string();
                        }
                        ExecutionResult::Blocked => {
                            assistant_text =
                                "🚫 That command is blocked for safety reasons.".to_string();
                        }
                    }
                }
            }

            "update_persona" => {
                if !state.config.is_admin(user_id) {
                    assistant_text = "❌ Only admins can update persona files.".to_string();
                } else if let (Some(file_name), Some(new_content)) = (
                    tool_call
                        .arguments
                        .get("file_name")
                        .and_then(|v| v.as_str()),
                    tool_call
                        .arguments
                        .get("new_content")
                        .and_then(|v| v.as_str()),
                ) {
                    identity_mgr.update_file(file_name, new_content).await?;
                    assistant_text =
                        format!("✅ Updated persona file: {}.md", file_name);
                }
            }

            _ => {
                assistant_text = format!("Tool '{}' is not implemented yet.", tool_call.name);
            }
        }
    }

    // ── 9. Save assistant response ─────────────────────────────────

    let resp_tokens = LlmClient::estimate_tokens(&assistant_text);
    state
        .db
        .save_message(conv_id, "assistant", &assistant_text, resp_tokens)
        .await?;

    // ── 10. Determine response mode (text or voice) ────────────────

    if msg.voice().is_some() {
        // Reply with voice
        let tts_engine_str = settings
            .get("tts_engine")
            .and_then(|v| v.as_str())
            .unwrap_or(&state.config.default_tts_engine);
        let engine = TtsEngine::from_str_loose(tts_engine_str);

        match state.tts.speak(&assistant_text, &engine).await {
            Ok(wav_bytes) => {
                // Convert WAV to OGG for Telegram voice
                let ogg_bytes = wav_to_ogg(&wav_bytes).await.unwrap_or(wav_bytes);
                let voice = InputFile::memory(ogg_bytes).file_name("response.ogg");
                bot.send_voice(msg.chat.id, voice).await?;
            }
            Err(e) => {
                tracing::error!("TTS failed: {}", e);
                // Fallback to text
                bot.send_message(msg.chat.id, &assistant_text).await?;
            }
        }
    } else {
        // Reply with text
        bot.send_message(msg.chat.id, &assistant_text).await?;
    }

    // ── 11. Periodically update user profile (every ~10 messages) ──

    let msg_count = state.db.get_messages(conv_id).await?.len();
    if msg_count % 10 == 0 && msg_count > 0 {
        let db_clone = state.db.clone();
        let llm_clone_config = state.config.clone();
        let llm = crate::ai::llm::LlmClient::new(&llm_clone_config);
        let ctx = ContextManager::new(state.config.max_context_tokens);
        tokio::spawn(async move {
            if let Err(e) = ctx.maybe_update_profile(&db_clone, &llm, user_id, conv_id).await {
                tracing::error!("Profile update failed: {}", e);
            }
        });
    }

    Ok(())
}

/// Convert OGG/Opus audio to PCM f32 16kHz mono using ffmpeg.
async fn ogg_to_pcm(ogg_data: &[u8]) -> anyhow::Result<Vec<f32>> {
    use tokio::process::Command;
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;

    let mut child = Command::new("ffmpeg")
        .args([
            "-i", "pipe:0",
            "-f", "f32le",
            "-acodec", "pcm_f32le",
            "-ar", "16000",
            "-ac", "1",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(ogg_data).await?;
        drop(stdin);
    }

    let output = child.wait_with_output().await?;

    if !output.status.success() {
        anyhow::bail!("ffmpeg ogg-to-pcm conversion failed");
    }

    // Convert raw bytes to f32 samples
    let samples: Vec<f32> = output
        .stdout
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();

    Ok(samples)
}

/// Convert WAV to OGG/Opus for Telegram voice messages using ffmpeg.
async fn wav_to_ogg(wav_data: &[u8]) -> anyhow::Result<Vec<u8>> {
    use tokio::process::Command;
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;

    let mut child = Command::new("ffmpeg")
        .args([
            "-i", "pipe:0",
            "-acodec", "libopus",
            "-f", "ogg",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(wav_data).await?;
        drop(stdin);
    }

    let output = child.wait_with_output().await?;

    if !output.status.success() {
        anyhow::bail!("ffmpeg wav-to-ogg conversion failed");
    }

    Ok(output.stdout)
}
