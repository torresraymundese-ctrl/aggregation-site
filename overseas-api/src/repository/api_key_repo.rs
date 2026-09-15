use crate::models::api_key::ApiKey;
use sqlx::PgPool;

pub struct ApiKeyRepository<'a> {
    pool: &'a PgPool,
}

impl<'a> ApiKeyRepository<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        uid: &str,
        user_id: i64,
        key_prefix: &str,
        key_hash: &str,
        name: &str,
        rate_limit: i32,
        models_allowed: serde_json::Value,
        daily_spend_limit: Option<f64>,
    ) -> Result<ApiKey, sqlx::Error> {
        sqlx::query_as::<_, ApiKey>(
            "INSERT INTO api_keys
             (uid, user_id, key_prefix, key_hash, name, rate_limit, models_allowed,
              daily_spend_limit, workspace_id, project_id)
             SELECT $1, $2, $3, $4, $5, $6, $7, $8::numeric, w.id, p.id
             FROM overseas_users u
             JOIN workspaces w ON w.uid = 'ws_' || u.uid
             JOIN projects p ON p.workspace_id = w.id AND p.name = 'Default'
             WHERE u.id = $2
             RETURNING *",
        )
        .bind(uid)
        .bind(user_id)
        .bind(key_prefix)
        .bind(key_hash)
        .bind(name)
        .bind(rate_limit)
        .bind(models_allowed)
        .bind(daily_spend_limit)
        .fetch_one(self.pool)
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_for_workspace(
        &self,
        uid: &str,
        user_id: i64,
        workspace_id: i64,
        project_id: i64,
        key_prefix: &str,
        key_hash: &str,
        name: &str,
        rate_limit: i32,
        models_allowed: serde_json::Value,
        daily_spend_limit: Option<f64>,
        monthly_spend_limit: Option<f64>,
        total_spend_limit: Option<f64>,
        expires_at: Option<chrono::NaiveDateTime>,
        ip_allowlist: &[String],
    ) -> Result<ApiKey, sqlx::Error> {
        sqlx::query_as::<_, ApiKey>(
            "INSERT INTO api_keys
             (uid, user_id, workspace_id, project_id, key_prefix, key_hash, name, rate_limit,
              models_allowed, daily_spend_limit, monthly_spend_limit, total_spend_limit,
              expires_at, ip_allowlist)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::NUMERIC, $11::NUMERIC, $12::NUMERIC,
                     $13, $14::TEXT[]::CIDR[])
             RETURNING *",
        )
        .bind(uid)
        .bind(user_id)
        .bind(workspace_id)
        .bind(project_id)
        .bind(key_prefix)
        .bind(key_hash)
        .bind(name)
        .bind(rate_limit)
        .bind(models_allowed)
        .bind(daily_spend_limit)
        .bind(monthly_spend_limit)
        .bind(total_spend_limit)
        .bind(expires_at)
        .bind(ip_allowlist)
        .fetch_one(self.pool)
        .await
    }

    pub async fn find_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>, sqlx::Error> {
        sqlx::query_as::<_, ApiKey>("SELECT * FROM api_keys WHERE key_hash = $1 AND status = 0")
            .bind(key_hash)
            .fetch_optional(self.pool)
            .await
    }

    pub async fn find_by_user(&self, user_id: i64) -> Result<Vec<ApiKey>, sqlx::Error> {
        sqlx::query_as::<_, ApiKey>(
            "SELECT * FROM api_keys WHERE user_id = $1 AND status IN (0, 2) ORDER BY created_at DESC"
        )
        .bind(user_id)
        .fetch_all(self.pool)
        .await
    }

    pub async fn find_by_uid(
        &self,
        uid: &str,
        user_id: i64,
    ) -> Result<Option<ApiKey>, sqlx::Error> {
        sqlx::query_as::<_, ApiKey>(
            "SELECT * FROM api_keys WHERE uid = $1 AND user_id = $2 AND status IN (0, 2)",
        )
        .bind(uid)
        .bind(user_id)
        .fetch_optional(self.pool)
        .await
    }

    pub async fn find_by_uid_in_workspace(
        &self,
        uid: &str,
        user_id: i64,
        workspace_id: i64,
    ) -> Result<Option<ApiKey>, sqlx::Error> {
        sqlx::query_as::<_, ApiKey>(
            "SELECT * FROM api_keys
             WHERE uid = $1 AND user_id = $2 AND workspace_id = $3 AND status IN (0, 2)",
        )
        .bind(uid)
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(self.pool)
        .await
    }

    pub async fn update(&self, id: i64, name: &str, rate_limit: i32) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE api_keys SET name = $1, rate_limit = $2, updated_at = NOW() WHERE id = $3",
        )
        .bind(name)
        .bind(rate_limit)
        .bind(id)
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn soft_delete(&self, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE api_keys SET status = 1, updated_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_status(&self, id: i64, status: i16) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE api_keys SET status = $1, updated_at = NOW() WHERE id = $2")
            .bind(status)
            .bind(id)
            .execute(self.pool)
            .await?;
        Ok(())
    }
}
