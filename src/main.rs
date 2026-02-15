use std::sync::Arc;

use teloxide::prelude::*;
use anyhow::Context;
use tracing_subscriber::EnvFilter;

mod agent;
mod ai;
mod bot;
mod config;
mod db;

use config::AppConfig;
use db::Database;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("🔥 Fatal Error: {:?}", e);
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing::level_filters::LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    tracing::info!("🤖 Starting TTS/STT Bot...");

    // ── 1. Load Config ─────────────────────────────────────────────
    let config = AppConfig::from_env().context("Failed to load config")?;
    tracing::info!("Config loaded. Model: {}", config.groq_model);

    // ── 2. Initialize Database ─────────────────────────────────────
    let db = Database::connect(&config.database_url).await.context("Failed to connect to database")?;
    db.run_migrations().await.context("Failed to run migrations")?;
    tracing::info!("✅ Database connected and migrated.");

    // ── 3. Initialize AI Engines ───────────────────────────────────
    
    // STT (Whisper)
    let stt = ai::stt::SttEngine::new(&config.whisper_model_path).context("Failed to initialize STT engine")?;
    tracing::info!("✅ STT engine initialized.");
    
    // TTS (Piper + XTTS)
    let tts = ai::tts::TtsManager::new(&config);
    tracing::info!("✅ TTS engine initialized.");

    // LLM (Groq)
    let llm = ai::llm::LlmClient::new(&config);
    tracing::info!("✅ LLM client initialized.");

    // ── 4. Start Bot ───────────────────────────────────────────────
    
    let state = Arc::new(bot::AppState {
        model_override: tokio::sync::RwLock::new(config.groq_model.clone()),
        config: config.clone(),
        db,
        stt,
        tts,
        llm,
    });

    let bot = Bot::new(&config.telegram_bot_token);
    let handler = bot::build_handler();

    tracing::info!("🚀 Bot is running...");

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}
