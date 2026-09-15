use crate::models::user::User;
use sqlx::PgPool;

pub struct UserRepository<'a> {
    pool: &'a PgPool,
}

impl<'a> UserRepository<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_email(&self, email: &str) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM overseas_users WHERE email = $1")
            .bind(email)
            .fetch_optional(self.pool)
            .await
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM overseas_users WHERE id = $1")
            .bind(id)
            .fetch_optional(self.pool)
            .await
    }

    pub async fn create(
        &self,
        uid: &str,
        email: &str,
        nickname: &str,
        password_hash: &str,
    ) -> Result<User, sqlx::Error> {
        sqlx::query_as::<_, User>(
            "INSERT INTO overseas_users (uid, email, nickname, password_hash)
             VALUES ($1, $2, $3, $4)
             RETURNING *",
        )
        .bind(uid)
        .bind(email)
        .bind(nickname)
        .bind(password_hash)
        .fetch_one(self.pool)
        .await
    }

    pub async fn update_last_login(&self, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE overseas_users SET last_login_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(self.pool)
            .await?;
        Ok(())
    }
}
