use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct SttEngine {
    ctx: WhisperContext,
}

impl SttEngine {
    pub fn new(model_path: &str) -> anyhow::Result<Self> {
        if !Path::new(model_path).exists() {
            anyhow::bail!(
                "Whisper model not found at '{}'. Download it from: \
                 https://huggingface.co/ggerganov/whisper.cpp/tree/main",
                model_path
            );
        }

        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .map_err(|e| anyhow::anyhow!("Failed to load whisper model: {}", e))?;

        tracing::info!("Whisper STT model loaded from '{}'", model_path);
        Ok(Self { ctx })
    }

    /// Transcribe raw PCM f32 audio data (16kHz mono) to text.
    pub fn transcribe(&self, pcm_data: &[f32]) -> anyhow::Result<String> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // Single-threaded for predictable performance on CPU
        params.set_n_threads(2);

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| anyhow::anyhow!("Failed to create whisper state: {}", e))?;

        state
            .full(params, pcm_data)
            .map_err(|e| anyhow::anyhow!("Whisper transcription failed: {}", e))?;

        let num_segments = state.full_n_segments()
            .map_err(|e| anyhow::anyhow!("Failed to get segments: {}", e))?;

        let mut text = String::new();
        for i in 0..num_segments {
            if let Ok(segment) = state.full_get_segment_text(i) {
                text.push_str(&segment);
                text.push(' ');
            }
        }

        Ok(text.trim().to_string())
    }
}
