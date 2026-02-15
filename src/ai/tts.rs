use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
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
    piper_binary_path: String,
    piper_lib_path: String,
    piper_model_path: String,
    xtts_url: String,
    /// Tracks whether XTTS sidecar is known to be available.
    /// Reset to true on each /settings change; set to false on connection failure.
    xtts_available: AtomicBool,
}

impl TtsManager {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            piper_binary_path: config.piper_binary_path.clone(),
            piper_lib_path: config.piper_lib_path.clone(),
            piper_model_path: config.piper_model_path.clone(),
            xtts_url: config.xtts_sidecar_url.clone(),
            xtts_available: AtomicBool::new(true),
        }
    }

    /// Reset XTTS availability flag (e.g., when user switches to XTTS in settings).
    pub fn reset_xtts_availability(&self) {
        self.xtts_available.store(true, Ordering::Relaxed);
    }

    /// Generate speech audio (WAV bytes) from text using the specified engine.
    /// Falls back to Piper if XTTS is unavailable.
    pub async fn speak(&self, text: &str, engine: &TtsEngine) -> anyhow::Result<Vec<u8>> {
        match engine {
            TtsEngine::Piper => self.speak_piper(text).await,
            TtsEngine::Xtts => {
                // Skip XTTS entirely if we already know it's down
                if !self.xtts_available.load(Ordering::Relaxed) {
                    tracing::debug!("XTTS known unavailable, using Piper directly");
                    return self.speak_piper(text).await;
                }

                match self.speak_xtts(text).await {
                    Ok(audio) => Ok(audio),
                    Err(e) => {
                        let err_str = e.to_string();
                        // Mark XTTS as unavailable if it's a connection error
                        if err_str.contains("not reachable") {
                            self.xtts_available.store(false, Ordering::Relaxed);
                            tracing::warn!(
                                "XTTS sidecar not reachable, disabling until /settings reset. \
                                 Falling back to Piper."
                            );
                        } else {
                            tracing::warn!("XTTS failed ({}), falling back to Piper", e);
                        }
                        self.speak_piper(text).await
                    }
                }
            }
        }
    }

    /// Piper TTS: uses the standalone piper binary to generate speech.
    async fn speak_piper(&self, text: &str) -> anyhow::Result<Vec<u8>> {
        let child = Command::new(&self.piper_binary_path)
            .args(["--model", &self.piper_model_path, "--output-raw"])
            .env("LD_LIBRARY_PATH", &self.piper_lib_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!(
                "Failed to start piper binary at '{}': {}. \
                 Make sure the binary exists.",
                self.piper_binary_path, e
            ))?;

        // Write text to stdin
        let mut child = child;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(text.as_bytes()).await?;
            drop(stdin); // close stdin to signal EOF
        }

        let output = child.wait_with_output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Piper TTS failed (exit {}): {}", output.status, stderr);
        }

        if output.stdout.is_empty() {
            anyhow::bail!("Piper TTS produced no audio output");
        }

        // Piper with --output-raw outputs raw PCM s16le 22050Hz mono.
        // We need to wrap it in a WAV header for Telegram.
        let wav = pcm_to_wav(&output.stdout, 22050, 1, 16);
        Ok(wav)
    }

    /// XTTS Sidecar: HTTP POST to the Python server.
    /// Uses a short connection timeout (2s) so we fail fast if sidecar isn't running,
    /// but a long response timeout (90s) to allow CPU inference.
    async fn speak_xtts(&self, text: &str) -> anyhow::Result<Vec<u8>> {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(90))
            .build()?;

        let resp = client
            .post(format!("{}/tts", self.xtts_url))
            .json(&serde_json::json!({
                "text": text,
                "language": "en"
            }))
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() || e.is_timeout() {
                    anyhow::anyhow!("XTTS sidecar not reachable at {}", self.xtts_url)
                } else {
                    anyhow::anyhow!("XTTS request error: {}", e)
                }
            })?;

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
