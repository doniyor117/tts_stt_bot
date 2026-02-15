use std::collections::HashSet;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

use crate::db::Database;

/// Safe commands that can be executed without admin approval.
const SAFE_COMMANDS: &[&str] = &[
    "date", "whoami", "hostname", "uptime", "uname", "echo", "cat", "ls", "pwd", "df", "free",
    "wc", "head", "tail", "which", "env", "printenv",
];

/// Commands that are ALWAYS blocked, even with approval.
const BLOCKED_COMMANDS: &[&str] = &[
    "rm -rf /",
    "rm -rf /*",
    "mkfs",
    "dd if=/dev/zero",
    ":(){ :|:& };:",
    "chmod -R 777 /",
];

pub struct CommandExecutor;

#[derive(Debug)]
pub enum ExecutionResult {
    /// Command was safe and executed immediately.
    Immediate(String),
    /// Command requires admin approval. Contains the approval request UUID.
    PendingApproval(Uuid),
    /// Command is permanently blocked.
    Blocked,
}

impl CommandExecutor {
    /// Classify a command and either run it, request approval, or block it.
    pub async fn execute(
        db: &Database,
        command: &str,
        user_id: i64,
        chat_id: i64,
    ) -> anyhow::Result<ExecutionResult> {
        let cmd_trimmed = command.trim();

        // Check blocked list
        for blocked in BLOCKED_COMMANDS {
            if cmd_trimmed.contains(blocked) {
                tracing::warn!(
                    "BLOCKED command from user {}: {}",
                    user_id,
                    cmd_trimmed
                );
                return Ok(ExecutionResult::Blocked);
            }
        }

        // Check if the base command is in the safe list
        let base_cmd = cmd_trimmed
            .split_whitespace()
            .next()
            .unwrap_or("");

        if SAFE_COMMANDS.contains(&base_cmd) {
            let output = Self::run_command(cmd_trimmed).await?;
            return Ok(ExecutionResult::Immediate(output));
        }

        // Risky: create approval request
        let approval = db
            .create_approval(cmd_trimmed, user_id, chat_id)
            .await?;

        tracing::info!(
            "Approval request {} created for command '{}' by user {}",
            approval.id,
            cmd_trimmed,
            user_id
        );

        Ok(ExecutionResult::PendingApproval(approval.id))
    }

    /// Actually run a shell command and capture output (with timeout).
    pub async fn run_command(command: &str) -> anyhow::Result<String> {
        let output = tokio::time::timeout(
            Duration::from_secs(30),
            Command::new("bash")
                .args(["-c", command])
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Command timed out after 30s"))?
        .map_err(|e| anyhow::anyhow!("Failed to execute command: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("STDERR: ");
            result.push_str(&stderr);
        }

        // Truncate very long output
        if result.len() > 4000 {
            result.truncate(4000);
            result.push_str("\n... (output truncated)");
        }

        Ok(result)
    }
}
