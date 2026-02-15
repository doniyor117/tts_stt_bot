pub mod models;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct Database {
    pub pool: PgPool,
}

impl Database {
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        // Each CREATE TABLE must be a separate query (Postgres doesn't allow
        // multiple commands in a single prepared statement).

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS users (
                id BIGINT PRIMARY KEY,
                username TEXT,
                profile_summary TEXT NOT NULL DEFAULT '',
                settings JSONB NOT NULL DEFAULT '{}',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS conversations (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                user_id BIGINT NOT NULL REFERENCES users(id),
                title TEXT NOT NULL DEFAULT 'New Chat',
                summary TEXT NOT NULL DEFAULT '',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS messages (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                token_count INT NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS approval_requests (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                command TEXT NOT NULL,
                requester_id BIGINT NOT NULL,
                requester_chat_id BIGINT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                result TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_conv ON messages(conversation_id, created_at)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_conversations_user ON conversations(user_id, updated_at DESC)")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    // ── User Operations ────────────────────────────────────────────

    pub async fn get_or_create_user(
        &self,
        user_id: i64,
        username: Option<&str>,
    ) -> anyhow::Result<models::User> {
        let user = sqlx::query_as::<_, models::User>(
            r#"
            INSERT INTO users (id, username)
            VALUES ($1, $2)
            ON CONFLICT (id) DO UPDATE SET username = COALESCE($2, users.username)
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(username)
        .fetch_one(&self.pool)
        .await?;

        Ok(user)
    }

    pub async fn update_user_profile(
        &self,
        user_id: i64,
        profile_summary: &str,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE users SET profile_summary = $2 WHERE id = $1")
            .bind(user_id)
            .bind(profile_summary)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_user_settings(
        &self,
        user_id: i64,
    ) -> anyhow::Result<serde_json::Value> {
        let row: (serde_json::Value,) =
            sqlx::query_as("SELECT settings FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(row.0)
    }

    pub async fn update_user_settings(
        &self,
        user_id: i64,
        settings: &serde_json::Value,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE users SET settings = $2 WHERE id = $1")
            .bind(user_id)
            .bind(settings)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Conversation Operations ────────────────────────────────────

    pub async fn create_conversation(
        &self,
        user_id: i64,
    ) -> anyhow::Result<models::Conversation> {
        let conv = sqlx::query_as::<_, models::Conversation>(
            "INSERT INTO conversations (user_id) VALUES ($1) RETURNING *",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(conv)
    }

    pub async fn list_conversations(
        &self,
        user_id: i64,
        limit: i64,
    ) -> anyhow::Result<Vec<models::Conversation>> {
        let convs = sqlx::query_as::<_, models::Conversation>(
            "SELECT * FROM conversations WHERE user_id = $1 ORDER BY updated_at DESC LIMIT $2",
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(convs)
    }

    pub async fn update_conversation_summary(
        &self,
        conv_id: uuid::Uuid,
        summary: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE conversations SET summary = $2, updated_at = NOW() WHERE id = $1",
        )
        .bind(conv_id)
        .bind(summary)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Message Operations ─────────────────────────────────────────

    pub async fn save_message(
        &self,
        conversation_id: uuid::Uuid,
        role: &str,
        content: &str,
        token_count: i32,
    ) -> anyhow::Result<models::Message> {
        let msg = sqlx::query_as::<_, models::Message>(
            r#"
            INSERT INTO messages (conversation_id, role, content, token_count)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(conversation_id)
        .bind(role)
        .bind(content)
        .bind(token_count)
        .fetch_one(&self.pool)
        .await?;

        // Touch the conversation's updated_at
        sqlx::query("UPDATE conversations SET updated_at = NOW() WHERE id = $1")
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;

        Ok(msg)
    }

    pub async fn get_messages(
        &self,
        conversation_id: uuid::Uuid,
    ) -> anyhow::Result<Vec<models::Message>> {
        let msgs = sqlx::query_as::<_, models::Message>(
            "SELECT * FROM messages WHERE conversation_id = $1 ORDER BY created_at ASC",
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(msgs)
    }

    pub async fn get_total_tokens(
        &self,
        conversation_id: uuid::Uuid,
    ) -> anyhow::Result<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(token_count), 0) FROM messages WHERE conversation_id = $1",
        )
        .bind(conversation_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn delete_oldest_messages(
        &self,
        conversation_id: uuid::Uuid,
        keep_count: i64,
    ) -> anyhow::Result<i64> {
        // Delete all but the newest `keep_count` messages
        let result = sqlx::query(
            r#"
            DELETE FROM messages
            WHERE id IN (
                SELECT id FROM messages
                WHERE conversation_id = $1
                ORDER BY created_at ASC
                LIMIT (
                    SELECT GREATEST(COUNT(*) - $2, 0)
                    FROM messages WHERE conversation_id = $1
                )
            )
            "#,
        )
        .bind(conversation_id)
        .bind(keep_count)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() as i64)
    }

    // ── Approval Operations ────────────────────────────────────────

    pub async fn create_approval(
        &self,
        command: &str,
        requester_id: i64,
        requester_chat_id: i64,
    ) -> anyhow::Result<models::ApprovalRequest> {
        let req = sqlx::query_as::<_, models::ApprovalRequest>(
            r#"
            INSERT INTO approval_requests (command, requester_id, requester_chat_id)
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .bind(command)
        .bind(requester_id)
        .bind(requester_chat_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(req)
    }

    pub async fn get_approval(
        &self,
        id: uuid::Uuid,
    ) -> anyhow::Result<Option<models::ApprovalRequest>> {
        let req = sqlx::query_as::<_, models::ApprovalRequest>(
            "SELECT * FROM approval_requests WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(req)
    }

    pub async fn update_approval_status(
        &self,
        id: uuid::Uuid,
        status: &str,
        result: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE approval_requests SET status = $2, result = $3 WHERE id = $1")
            .bind(id)
            .bind(status)
            .bind(result)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
