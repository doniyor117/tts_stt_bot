use std::process::Stdio;
use tokio::process::Command;

use crate::config::AppConfig;

/// Supported TTS engines
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TtsEngine {
    Piper,
    Xtts,
}

impl TtsEngine {
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "xtts" | "xtts-v2" => Self::Xtts,
            _ => Self::Piper,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Piper => "Piper (Fast/CPU)",
            Self::Xtts => "XTTS-v2 (Quality/GPU)",
        }
    }
}

pub struct TtsManager {
    piper_model_path: String,
    xtts_url: String,
}

impl TtsManager {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            piper_model_path: config.piper_model_path.clone(),
            xtts_url: config.xtts_sidecar_url.clone(),
        }
    }

    /// Generate speech audio (WAV bytes) from text using the specified engine.
    pub async fn speak(&self, text: &str, engine: &TtsEngine) -> anyhow::Result<Vec<u8>> {
        match engine {
            TtsEngine::Piper => self.speak_piper(text).await,
            TtsEngine::Xtts => self.speak_xtts(text).await,
        }
    }

    /// Piper TTS: pipes text into the `piper` CLI and captures WAV output from stdout.
    async fn speak_piper(&self, text: &str) -> anyhow::Result<Vec<u8>> {
        let output = Command::new("piper")
            .args(["--model", &self.piper_model_path, "--output-raw"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Write text to stdin
        let mut child = output;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(text.as_bytes()).await?;
            drop(stdin); // close stdin to signal EOF
        }

        let output = child.wait_with_output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Piper TTS failed: {}", stderr);
        }

        // Piper with --output-raw outputs raw PCM s16le 22050Hz mono.
        // We need to wrap it in a WAV header for Telegram.
        let wav = pcm_to_wav(&output.stdout, 22050, 1, 16);
        Ok(wav)
    }

    /// XTTS Sidecar: HTTP POST to the Python server.
    async fn speak_xtts(&self, text: &str) -> anyhow::Result<Vec<u8>> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/tts", self.xtts_url))
            .json(&serde_json::json!({
                "text": text,
                "language": "en"
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("XTTS sidecar error: {}", err_text);
        }

        let wav_bytes = resp.bytes().await?.to_vec();
        Ok(wav_bytes)
    }
}

/// Convert raw PCM (s16le) bytes into a proper WAV file in memory.
fn pcm_to_wav(pcm: &[u8], sample_rate: u32, channels: u16, bits_per_sample: u16) -> Vec<u8> {
    let data_size = pcm.len() as u32;
    let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    let block_align = channels * bits_per_sample / 8;
    let file_size = 36 + data_size;

    let mut wav = Vec::with_capacity(44 + pcm.len());
    // RIFF header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&file_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    // fmt chunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    // data chunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(pcm);

    wav
}
