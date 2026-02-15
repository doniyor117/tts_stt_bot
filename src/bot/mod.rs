pub mod callbacks;
pub mod commands;
pub mod handlers;

use std::sync::Arc;
use teloxide::dispatching::UpdateFilterExt;
use teloxide::dptree;
use teloxide::prelude::*;

use crate::ai::{llm::LlmClient, stt::SttEngine, tts::TtsManager};
use crate::config::AppConfig;
use crate::db::Database;

/// Shared application state, accessible from all handlers.
pub struct AppState {
    pub config: AppConfig,
    pub db: Database,
    pub stt: SttEngine,
    pub tts: TtsManager,
    pub llm: LlmClient,
}

/// Build the teloxide update handler tree.
pub fn build_handler() -> Handler<'static, DependencyMap, (), dptree::di::DependencySupplyError> {
    let command_handler = Update::filter_message()
        .filter_command::<commands::BotCommand>()
        .endpoint(commands::handle_command);

    let callback_handler = Update::filter_callback_query()
        .endpoint(callbacks::handle_callback);

    let message_handler = Update::filter_message()
        .endpoint(handlers::handle_message);

    dptree::entry()
        .branch(command_handler)
        .branch(callback_handler)
        .branch(message_handler)
}
