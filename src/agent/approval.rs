use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use uuid::Uuid;

use crate::db::Database;

/// Send an approval request to the admin group with Approve/Deny buttons.
pub async fn request_approval(
    bot: &Bot,
    admin_group_id: i64,
    command: &str,
    user_id: i64,
    approval_id: Uuid,
) -> anyhow::Result<()> {
    let text = format!(
        "⚠️ *Action Required*\n\n\
         👤 User: `{}`\n\
         💻 Command: `{}`\n\
         🆔 Request: `{}`\n\n\
         Please approve or deny this action.",
        user_id, command, approval_id
    );

    let keyboard = InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback("✅ Approve", format!("approve:{}", approval_id)),
        InlineKeyboardButton::callback("❌ Deny", format!("deny:{}", approval_id)),
    ]]);

    bot.send_message(ChatId(admin_group_id), text)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .reply_markup(keyboard)
        .await?;

    Ok(())
}

/// Handle an approval callback (approve or deny).
pub async fn handle_approval_callback(
    bot: &Bot,
    db: &Database,
    approval_id: Uuid,
    approved: bool,
    admin_user_id: i64,
    admin_ids: &[i64],
) -> anyhow::Result<String> {
    // Verify clicker is an admin
    if !admin_ids.contains(&admin_user_id) {
        return Ok("❌ You are not an admin.".to_string());
    }

    let approval = db.get_approval(approval_id).await?;
    let approval = match approval {
        Some(a) => a,
        None => return Ok("❌ Approval request not found.".to_string()),
    };

    if approval.status != "pending" {
        return Ok(format!("ℹ️ This request was already {}.", approval.status));
    }

    if approved {
        // Execute the command
        let output = crate::agent::executor::CommandExecutor::run_command(&approval.command).await?;

        db.update_approval_status(approval_id, "approved", Some(&output))
            .await?;

        // Notify the original user
        let user_msg = format!(
            "✅ Your command was approved and executed:\n```\n{}\n```\nOutput:\n```\n{}\n```",
            approval.command,
            if output.is_empty() { "(no output)" } else { &output }
        );
        bot.send_message(ChatId(approval.requester_chat_id), user_msg)
            .await?;

        Ok(format!(
            "✅ Approved and executed. Output:\n```\n{}\n```",
            if output.is_empty() { "(no output)" } else { &output }
        ))
    } else {
        db.update_approval_status(approval_id, "denied", None)
            .await?;

        // Notify the original user
        bot.send_message(
            ChatId(approval.requester_chat_id),
            format!("❌ Your command `{}` was denied by an admin.", approval.command),
        )
        .await?;

        Ok("❌ Denied.".to_string())
    }
}
