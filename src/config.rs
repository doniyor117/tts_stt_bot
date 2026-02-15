use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub telegram_bot_token: String,
    pub groq_api_key: String,
    pub groq_model: String,
    pub database_url: String,

    /// Comma-separated Telegram user IDs of admins
    pub admin_ids: Vec<i64>,
    /// Telegram chat ID of the admin approval group
    pub admin_group_id: i64,

    /// Default TTS engine: "piper" or "xtts"
    pub default_tts_engine: String,
    pub piper_model_path: String,
    pub xtts_sidecar_url: String,

    /// Path to the GGML whisper model file
    pub whisper_model_path: String,

    /// Max tokens in conversation context before pruning
    pub max_context_tokens: usize,
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let admin_ids_str = std::env::var("ADMIN_IDS").unwrap_or_default();
        let admin_ids: Vec<i64> = admin_ids_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        Ok(Self {
            telegram_bot_token: std::env::var("TELEGRAM_BOT_TOKEN")?,
            groq_api_key: std::env::var("GROQ_API_KEY")?,
            groq_model: std::env::var("GROQ_MODEL")
                .unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string()),
            database_url: std::env::var("DATABASE_URL")?,
            admin_ids,
            admin_group_id: std::env::var("ADMIN_GROUP_ID")
                .unwrap_or_else(|_| "0".to_string())
                .parse()?,
            default_tts_engine: std::env::var("DEFAULT_TTS_ENGINE")
                .unwrap_or_else(|_| "piper".to_string()),
            piper_model_path: std::env::var("PIPER_MODEL_PATH")
                .unwrap_or_else(|_| "./data/models/piper/en_US-amy-medium.onnx".to_string()),
            xtts_sidecar_url: std::env::var("XTTS_SIDECAR_URL")
                .unwrap_or_else(|_| "http://localhost:8020".to_string()),
            whisper_model_path: std::env::var("WHISPER_MODEL_PATH")
                .unwrap_or_else(|_| "./data/models/whisper/ggml-base.en.bin".to_string()),
            max_context_tokens: std::env::var("MAX_CONTEXT_TOKENS")
                .unwrap_or_else(|_| "4000".to_string())
                .parse()
                .unwrap_or(4000),
        })
    }

    pub fn is_admin(&self, user_id: i64) -> bool {
        self.admin_ids.contains(&user_id)
    }
}
