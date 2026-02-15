use std::sync::Arc;

use teloxide::prelude::*;
use tracing_subscriber::EnvFilter;

mod agent;
mod ai;
mod bot;
mod config;
mod db;

use config::AppConfig;
use db::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env
    dotenvy::dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("🤖 Starting TTS/STT Bot...");

    // Load config
    let config = AppConfig::from_env()?;
    tracing::info!("Config loaded. Model: {}", config.groq_model);

    // Initialize database
    let db = Database::connect(&config.database_url).await?;
    db.run_migrations().await?;
    tracing::info!("Database connected and migrations applied.");

    // Initialize AI modules
    let stt_engine = ai::stt::SttEngine::new(&config.whisper_model_path)?;
    let tts_manager = ai::tts::TtsManager::new(&config);
    let llm_client = ai::llm::LlmClient::new(&config);

    // Build shared application state
    let state = Arc::new(bot::AppState {
        config: config.clone(),
        db,
        stt: stt_engine,
        tts: tts_manager,
        llm: llm_client,
    });

    // Create the Telegram bot
    let bot = Bot::new(&config.telegram_bot_token);

    // Build the dispatcher
    let handler = bot::build_handler();

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}
