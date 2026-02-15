use std::path::{Path, PathBuf};
use tokio::fs;

/// Manages the bot's persona files (SOUL.md, IDENTITY.md, SECURITY.md).
/// These files define the bot's behavior and are loaded into the system prompt.
pub struct IdentityManager {
    persona_dir: PathBuf,
}

impl IdentityManager {
    pub fn new(persona_dir: &str) -> Self {
        Self {
            persona_dir: PathBuf::from(persona_dir),
        }
    }

    /// Load a persona file by name (e.g., "SOUL", "IDENTITY", "SECURITY").
    pub async fn load_file(&self, name: &str) -> anyhow::Result<String> {
        let path = self.persona_dir.join(format!("{}.md", name));
        if !path.exists() {
            tracing::warn!("Persona file not found: {:?}", path);
            return Ok(String::new());
        }
        let content = fs::read_to_string(&path).await?;
        Ok(content)
    }

    /// Update a persona file. Only admins should call this.
    pub async fn update_file(&self, name: &str, new_content: &str) -> anyhow::Result<()> {
        let path = self.persona_dir.join(format!("{}.md", name));

        // Create backup before overwriting
        if path.exists() {
            let backup = self
                .persona_dir
                .join(format!("{}.md.bak", name));
            fs::copy(&path, &backup).await?;
            tracing::info!("Backed up {:?} to {:?}", path, backup);
        }

        fs::write(&path, new_content).await?;
        tracing::info!("Updated persona file: {:?}", path);
        Ok(())
    }

    /// Build the full system prompt by combining all persona files + user profile.
    pub async fn build_system_prompt(
        &self,
        user_profile: &str,
        available_tools_desc: &str,
    ) -> anyhow::Result<String> {
        let soul = self.load_file("SOUL").await.unwrap_or_default();
        let identity = self.load_file("IDENTITY").await.unwrap_or_default();
        let security = self.load_file("SECURITY").await.unwrap_or_default();

        let mut prompt = String::with_capacity(2048);

        if !soul.is_empty() {
            prompt.push_str("## Core Philosophy\n");
            prompt.push_str(&soul);
            prompt.push_str("\n\n");
        }

        if !identity.is_empty() {
            prompt.push_str("## Personality\n");
            prompt.push_str(&identity);
            prompt.push_str("\n\n");
        }

        if !security.is_empty() {
            prompt.push_str("## Security Rules\n");
            prompt.push_str(&security);
            prompt.push_str("\n\n");
        }

        if !user_profile.is_empty() {
            prompt.push_str("## About the User\n");
            prompt.push_str(user_profile);
            prompt.push_str("\n\n");
        }

        if !available_tools_desc.is_empty() {
            prompt.push_str("## Available Tools\n");
            prompt.push_str(available_tools_desc);
            prompt.push_str("\n\n");
        }

        prompt.push_str("## Response Guidelines\n");
        prompt.push_str(
            "- If the user sends a voice message, it has been transcribed for you. \
             Respond naturally.\n\
             - Keep responses concise for voice output (they will be spoken aloud via TTS).\n\
             - You can use tools by responding with a JSON tool call.\n",
        );

        Ok(prompt)
    }

    /// Ensure default persona files exist.
    pub async fn ensure_defaults(&self) -> anyhow::Result<()> {
        let dir = &self.persona_dir;
        if !dir.exists() {
            fs::create_dir_all(dir).await?;
        }

        let defaults = [
            ("SOUL", include_str!("../../persona/SOUL.md")),
            ("IDENTITY", include_str!("../../persona/IDENTITY.md")),
            ("SECURITY", include_str!("../../persona/SECURITY.md")),
        ];

        for (name, default_content) in &defaults {
            let path = dir.join(format!("{}.md", name));
            if !path.exists() {
                fs::write(&path, default_content).await?;
                tracing::info!("Created default persona file: {:?}", path);
            }
        }

        Ok(())
    }
}
