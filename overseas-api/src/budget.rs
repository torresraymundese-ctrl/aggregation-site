use chrono::{Datelike, NaiveDate, Utc};
use sqlx::{PgPool, Postgres, Row, Transaction};

const RESERVATION_TTL_MINUTES: i32 = 15;
const TOTAL_PERIOD_START: &str = "1970-01-01";

#[derive(Clone, Debug)]
pub(crate) struct RequestBudgetContext {
    pub request_id: String,
    pub api_key_id: i64,
    pub user_id: i64,
    pub workspace_id: i64,
    pub project_id: i64,
    pub logical_model_id: i64,
    pub provider_model_id: i64,
    pub provider_id: i64,
    pub provider_credential_id: Option<i64>,
    pub model: String,
    pub routing_strategy: String,
    pub credential_source: String,
}

#[derive(Clone, Debug)]
pub(crate) struct CompletedRequest {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub user_cost_usd: f64,
    pub provider_cost_usd: f64,
    pub latency_ms: i32,
    pub status_code: i32,
    pub error_message: Option<String>,
    pub error_code: Option<String>,
    pub retryable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReserveFailureKind {
    DailyBudget,
    MonthlyBudget,
    TotalBudget,
    InsufficientBalance,
    InvalidAmount,
    Database,
}

#[derive(Clone, Debug)]
pub(crate) struct ReserveFailure {
    pub kind: ReserveFailureKind,
    pub message: String,
}

impl ReserveFailure {
    fn database(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "Atomic budget reservation failed");
        Self {
            kind: ReserveFailureKind::Database,
            message: "Budget state unavailable".to_string(),
        }
    }
}

fn money(value: f64) -> Result<String, ReserveFailure> {
    if !value.is_finite() || value < 0.0 {
        return Err(ReserveFailure {
            kind: ReserveFailureKind::InvalidAmount,
            message: "Reservation amount is invalid".to_string(),
        });
    }
    Ok(format!("{value:.8}"))
}

fn would_exceed_budget(spent: f64, reserved: f64, requested: f64, limit: f64) -> bool {
    spent + reserved + requested > limit + 0.000000001
}

async fn upsert_period(
    tx: &mut Transaction<'_, Postgres>,
    api_key_id: i64,
    period_type: &str,
    period_start: NaiveDate,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO api_key_budget_periods
         (api_key_id, period_type, period_start, spent_usd, reserved_usd)
         VALUES ($1, $2, $3, 0, 0)
         ON CONFLICT (api_key_id, period_type, period_start) DO NOTHING",
    )
    .bind(api_key_id)
    .bind(period_type)
    .bind(period_start)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn release_expired_reservations(
    tx: &mut Transaction<'_, Postgres>,
    api_key_id: i64,
) -> Result<(), sqlx::Error> {
    let expired = sqlx::query(
        "SELECT sr.id, sr.api_call_id, CAST(sr.reserved_usd AS VARCHAR),
                sr.day_bucket, sr.month_bucket, ac.workspace_id
         FROM spend_reservations sr
         JOIN api_calls ac ON ac.id = sr.api_call_id
         WHERE sr.api_key_id = $1 AND sr.state = 'reserved' AND sr.expires_at <= NOW()
         FOR UPDATE OF sr",
    )
    .bind(api_key_id)
    .fetch_all(&mut **tx)
    .await?;

    for row in expired {
        let reservation_id: i64 = row.get(0);
        let api_call_id: i64 = row.get(1);
        let reserved: String = row.get(2);
        let day_bucket: NaiveDate = row.get(3);
        let month_bucket: NaiveDate = row.get(4);
        let workspace_id: i64 = row.get(5);

        for (period_type, period_start) in [
            ("day", day_bucket),
            ("month", month_bucket),
            (
                "total",
                NaiveDate::parse_from_str(TOTAL_PERIOD_START, "%Y-%m-%d")
                    .expect("fixed total period date"),
            ),
        ] {
            sqlx::query(
                "UPDATE api_key_budget_periods
                 SET reserved_usd = GREATEST(reserved_usd - $1::NUMERIC, 0), updated_at = NOW()
                 WHERE api_key_id = $2 AND period_type = $3 AND period_start = $4",
            )
            .bind(&reserved)
            .bind(api_key_id)
            .bind(period_type)
            .bind(period_start)
            .execute(&mut **tx)
            .await?;
        }

        let released_balance = sqlx::query(
            "UPDATE balances
             SET balance = balance + $1::NUMERIC,
                 frozen_balance = frozen_balance - $1::NUMERIC,
                 updated_at = NOW()
             WHERE workspace_id = $2 AND frozen_balance >= $1::NUMERIC",
        )
        .bind(&reserved)
        .bind(workspace_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();
        if released_balance != 1 {
            return Err(sqlx::Error::RowNotFound);
        }

        sqlx::query(
            "UPDATE spend_reservations
             SET state = 'released', actual_usd = 0, settled_at = NOW()
             WHERE id = $1 AND state = 'reserved'",
        )
        .bind(reservation_id)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            "UPDATE api_calls
             SET status_code = 504, error_msg = 'Budget reservation expired',
                 error_code = 'BUDGET_RESERVATION_EXPIRED', retryable = TRUE,
                 state = 'failed', completed_at = NOW()
             WHERE id = $1 AND state = 'pending'",
        )
        .bind(api_call_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub(crate) async fn reserve_request_budget(
    pool: &PgPool,
    context: &RequestBudgetContext,
    reserved_usd: f64,
) -> Result<(), ReserveFailure> {
    let reserved = money(reserved_usd)?;
    let mut tx = pool.begin().await.map_err(ReserveFailure::database)?;

    let key = sqlx::query(
        "SELECT CAST(daily_spend_limit AS VARCHAR),
                CAST(monthly_spend_limit AS VARCHAR),
                CAST(total_spend_limit AS VARCHAR)
         FROM api_keys
         WHERE id = $1 AND user_id = $2 AND workspace_id = $3 AND project_id = $4
           AND status = 0 AND (expires_at IS NULL OR expires_at > NOW())
         FOR UPDATE",
    )
    .bind(context.api_key_id)
    .bind(context.user_id)
    .bind(context.workspace_id)
    .bind(context.project_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ReserveFailure::database)?
    .ok_or_else(|| ReserveFailure {
        kind: ReserveFailureKind::Database,
        message: "API key is no longer active".to_string(),
    })?;

    release_expired_reservations(&mut tx, context.api_key_id)
        .await
        .map_err(ReserveFailure::database)?;

    let today = Utc::now().date_naive();
    let month = today.with_day(1).expect("valid first day of month");
    let total =
        NaiveDate::parse_from_str(TOTAL_PERIOD_START, "%Y-%m-%d").expect("fixed total period date");
    let periods = [("day", today), ("month", month), ("total", total)];
    for (period_type, period_start) in periods {
        upsert_period(&mut tx, context.api_key_id, period_type, period_start)
            .await
            .map_err(ReserveFailure::database)?;
    }

    let limits = [
        (
            "day",
            today,
            key.get::<Option<String>, _>(0),
            ReserveFailureKind::DailyBudget,
        ),
        (
            "month",
            month,
            key.get::<Option<String>, _>(1),
            ReserveFailureKind::MonthlyBudget,
        ),
        (
            "total",
            total,
            key.get::<Option<String>, _>(2),
            ReserveFailureKind::TotalBudget,
        ),
    ];
    for (period_type, period_start, limit, kind) in limits {
        let Some(limit) = limit else { continue };
        let row = sqlx::query(
            "SELECT CAST(spent_usd AS VARCHAR), CAST(reserved_usd AS VARCHAR)
             FROM api_key_budget_periods
             WHERE api_key_id = $1 AND period_type = $2 AND period_start = $3
             FOR UPDATE",
        )
        .bind(context.api_key_id)
        .bind(period_type)
        .bind(period_start)
        .fetch_one(&mut *tx)
        .await
        .map_err(ReserveFailure::database)?;
        let spent = row
            .get::<String, _>(0)
            .parse::<f64>()
            .map_err(ReserveFailure::database)?;
        let pending = row
            .get::<String, _>(1)
            .parse::<f64>()
            .map_err(ReserveFailure::database)?;
        let limit_value = limit.parse::<f64>().map_err(ReserveFailure::database)?;
        if would_exceed_budget(spent, pending, reserved_usd, limit_value) {
            return Err(ReserveFailure {
                kind,
                message: format!(
                    "API key {period_type} budget would be exceeded: limit {limit_value:.8}, used {spent:.8}, pending {pending:.8}, requested {reserved_usd:.8}"
                ),
            });
        }
    }

    let api_call_id: i64 = sqlx::query(
        "INSERT INTO api_calls
         (request_id, api_key_id, user_id, model_id, provider_id,
          workspace_id, project_id, logical_model_id, provider_model_id,
          provider_credential_id, routing_strategy, credential_source, state)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'pending')
         RETURNING id",
    )
    .bind(&context.request_id)
    .bind(context.api_key_id)
    .bind(context.user_id)
    .bind(&context.model)
    .bind(context.provider_id)
    .bind(context.workspace_id)
    .bind(context.project_id)
    .bind(context.logical_model_id)
    .bind(context.provider_model_id)
    .bind(context.provider_credential_id)
    .bind(&context.routing_strategy)
    .bind(&context.credential_source)
    .fetch_one(&mut *tx)
    .await
    .map_err(ReserveFailure::database)?
    .get(0);

    if reserved_usd > 0.0 {
        let changed = sqlx::query(
            "UPDATE balances
             SET balance = balance - $1::NUMERIC,
                 frozen_balance = frozen_balance + $1::NUMERIC,
                 updated_at = NOW()
             WHERE workspace_id = $2 AND balance >= $1::NUMERIC",
        )
        .bind(&reserved)
        .bind(context.workspace_id)
        .execute(&mut *tx)
        .await
        .map_err(ReserveFailure::database)?
        .rows_affected();
        if changed != 1 {
            return Err(ReserveFailure {
                kind: ReserveFailureKind::InsufficientBalance,
                message: format!(
                    "Insufficient available balance for reservation {reserved_usd:.8}"
                ),
            });
        }

        for (period_type, period_start) in periods {
            sqlx::query(
                "UPDATE api_key_budget_periods
                 SET reserved_usd = reserved_usd + $1::NUMERIC, updated_at = NOW()
                 WHERE api_key_id = $2 AND period_type = $3 AND period_start = $4",
            )
            .bind(&reserved)
            .bind(context.api_key_id)
            .bind(period_type)
            .bind(period_start)
            .execute(&mut *tx)
            .await
            .map_err(ReserveFailure::database)?;
        }
    }

    sqlx::query(
        "INSERT INTO spend_reservations
         (api_call_id, api_key_id, reserved_usd, state, expires_at, day_bucket, month_bucket)
         VALUES ($1, $2, $3::NUMERIC, 'reserved', NOW() + ($4 * INTERVAL '1 minute'), $5, $6)",
    )
    .bind(api_call_id)
    .bind(context.api_key_id)
    .bind(&reserved)
    .bind(RESERVATION_TTL_MINUTES)
    .bind(today)
    .bind(month)
    .execute(&mut *tx)
    .await
    .map_err(ReserveFailure::database)?;

    sqlx::query(
        "INSERT INTO request_attempts
         (api_call_id, attempt_no, provider_model_id, provider_credential_id)
         VALUES ($1, 1, $2, $3)
         ON CONFLICT (api_call_id) DO NOTHING",
    )
    .bind(api_call_id)
    .bind(context.provider_model_id)
    .bind(context.provider_credential_id)
    .execute(&mut *tx)
    .await
    .map_err(ReserveFailure::database)?;

    tx.commit().await.map_err(ReserveFailure::database)?;
    Ok(())
}

async fn move_reservation(
    tx: &mut Transaction<'_, Postgres>,
    context: &RequestBudgetContext,
    api_call_id: i64,
    reservation_id: i64,
    reserved: &str,
    actual: &str,
    actual_value: f64,
    day_bucket: NaiveDate,
    month_bucket: NaiveDate,
    succeeded: bool,
) -> Result<(), sqlx::Error> {
    let total =
        NaiveDate::parse_from_str(TOTAL_PERIOD_START, "%Y-%m-%d").expect("fixed total period date");
    for (period_type, period_start) in [
        ("day", day_bucket),
        ("month", month_bucket),
        ("total", total),
    ] {
        let changed = sqlx::query(
            "UPDATE api_key_budget_periods
             SET reserved_usd = reserved_usd - $1::NUMERIC,
                 spent_usd = spent_usd + $2::NUMERIC,
                 updated_at = NOW()
             WHERE api_key_id = $3 AND period_type = $4 AND period_start = $5
               AND reserved_usd >= $1::NUMERIC",
        )
        .bind(reserved)
        .bind(actual)
        .bind(context.api_key_id)
        .bind(period_type)
        .bind(period_start)
        .execute(&mut **tx)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(sqlx::Error::RowNotFound);
        }
    }

    let balance = sqlx::query(
        "UPDATE balances
         SET balance = balance + $1::NUMERIC - $2::NUMERIC,
             frozen_balance = frozen_balance - $1::NUMERIC,
             total_consumed = total_consumed + $2::NUMERIC,
             updated_at = NOW()
         WHERE workspace_id = $3 AND frozen_balance >= $1::NUMERIC
         RETURNING CAST(balance AS VARCHAR)",
    )
    .bind(reserved)
    .bind(actual)
    .bind(context.workspace_id)
    .fetch_one(&mut **tx)
    .await?;
    let balance_after: String = balance.get(0);

    sqlx::query(
        "UPDATE spend_reservations
         SET actual_usd = $1::NUMERIC, state = $2, settled_at = NOW()
         WHERE id = $3 AND state = 'reserved'",
    )
    .bind(actual)
    .bind(if succeeded { "settled" } else { "released" })
    .bind(reservation_id)
    .execute(&mut **tx)
    .await?;

    if actual_value > 0.0 {
        sqlx::query(
            "INSERT INTO balance_logs
             (user_id, workspace_id, project_id, api_call_id, change_amount,
              balance_before, balance_after, change_type, note)
             VALUES ($1, $2, $3, $4, -$5::NUMERIC,
                     $6::NUMERIC + $5::NUMERIC, $6::NUMERIC, 'api_usage', 'API request charge')
             ON CONFLICT (api_call_id) WHERE api_call_id IS NOT NULL DO NOTHING",
        )
        .bind(context.user_id)
        .bind(context.workspace_id)
        .bind(context.project_id)
        .bind(api_call_id)
        .bind(actual)
        .bind(balance_after)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub(crate) async fn complete_request(
    pool: &PgPool,
    context: &RequestBudgetContext,
    completed: &CompletedRequest,
) -> Result<(), String> {
    let succeeded = (200..300).contains(&completed.status_code);
    let actual_value = if succeeded {
        completed.user_cost_usd
    } else {
        0.0
    };
    let actual = money(actual_value).map_err(|error| error.message)?;
    let provider_cost = money(completed.provider_cost_usd).map_err(|error| error.message)?;
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;

    // 与预留保持同一锁顺序，避免“新预留”和“旧请求结算”互相等待。
    sqlx::query("SELECT id FROM api_keys WHERE id = $1 FOR UPDATE")
        .bind(context.api_key_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;

    let reservation = sqlx::query(
        "SELECT sr.id, sr.api_call_id, CAST(sr.reserved_usd AS VARCHAR),
                CAST(sr.actual_usd AS VARCHAR), sr.state, sr.day_bucket, sr.month_bucket
         FROM spend_reservations sr
         JOIN api_calls ac ON ac.id = sr.api_call_id
         WHERE ac.request_id = $1
         FOR UPDATE OF sr",
    )
    .bind(&context.request_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;

    if let Some(row) = &reservation {
        let state: String = row.get(4);
        if state == "reserved" {
            let reserved: String = row.get(2);
            let reserved_value = reserved
                .parse::<f64>()
                .map_err(|_| "Reserved amount cannot be parsed".to_string())?;
            if actual_value > reserved_value + 0.000000001 {
                return Err(format!(
                    "Actual cost {actual_value:.8} exceeds reserved amount {reserved_value:.8}"
                ));
            }
            move_reservation(
                &mut tx,
                context,
                row.get(1),
                row.get(0),
                &reserved,
                &actual,
                actual_value,
                row.get(5),
                row.get(6),
                succeeded,
            )
            .await
            .map_err(|error| error.to_string())?;
        } else if state == "settled" && succeeded {
            let previous = row
                .get::<Option<String>, _>(3)
                .and_then(|value| value.parse::<f64>().ok())
                .unwrap_or(-1.0);
            if (previous - actual_value).abs() > 0.000000001 {
                return Err("Settled request cost does not match the previous result".to_string());
            }
        } else if state == "released" && !succeeded {
            // 重复失败完成是幂等操作。
        } else {
            return Err(format!("Reservation is already {state}"));
        }
    } else if succeeded {
        return Err("Successful request has no budget reservation".to_string());
    }

    let state = if succeeded { "succeeded" } else { "failed" };
    let cost = if succeeded { &actual } else { "0.00000000" };
    let api_call_id: i64 = sqlx::query(
        "INSERT INTO api_calls
         (request_id, api_key_id, user_id, model_id, provider_id, input_tokens,
          output_tokens, cost_points, latency_ms, status_code, error_msg,
          workspace_id, project_id, logical_model_id, provider_model_id,
          provider_credential_id, routing_strategy, credential_source,
          provider_cost_usd, user_cost_usd, error_code, retryable, state, completed_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::NUMERIC, $9, $10, $11,
                 $12, $13, $14, $15, $16, $17, $18, $19::NUMERIC, $8::NUMERIC,
                 $20, $21, $22, NOW())
         ON CONFLICT (request_id) DO UPDATE SET
          input_tokens = EXCLUDED.input_tokens,
          output_tokens = EXCLUDED.output_tokens,
          cost_points = EXCLUDED.cost_points,
          latency_ms = EXCLUDED.latency_ms,
          status_code = EXCLUDED.status_code,
          error_msg = EXCLUDED.error_msg,
          provider_cost_usd = EXCLUDED.provider_cost_usd,
          user_cost_usd = EXCLUDED.user_cost_usd,
          error_code = EXCLUDED.error_code,
          retryable = EXCLUDED.retryable,
          state = EXCLUDED.state,
          completed_at = NOW()
         RETURNING id",
    )
    .bind(&context.request_id)
    .bind(context.api_key_id)
    .bind(context.user_id)
    .bind(&context.model)
    .bind(context.provider_id)
    .bind(completed.input_tokens)
    .bind(completed.output_tokens)
    .bind(cost)
    .bind(completed.latency_ms)
    .bind(completed.status_code)
    .bind(&completed.error_message)
    .bind(context.workspace_id)
    .bind(context.project_id)
    .bind(context.logical_model_id)
    .bind(context.provider_model_id)
    .bind(context.provider_credential_id)
    .bind(&context.routing_strategy)
    .bind(&context.credential_source)
    .bind(&provider_cost)
    .bind(&completed.error_code)
    .bind(completed.retryable)
    .bind(state)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| error.to_string())?
    .get(0);

    sqlx::query(
        "INSERT INTO request_attempts
         (api_call_id, attempt_no, provider_model_id, provider_credential_id,
          completed_at, status_code, error_code, retryable, latency_ms,
          input_tokens, output_tokens, provider_cost_usd)
         VALUES ($1, 1, $2, $3, NOW(), $4, $5, $6, $7, $8, $9, $10::NUMERIC)
         ON CONFLICT (api_call_id) DO UPDATE SET
          completed_at = NOW(), status_code = EXCLUDED.status_code,
          error_code = EXCLUDED.error_code, retryable = EXCLUDED.retryable,
          latency_ms = EXCLUDED.latency_ms, input_tokens = EXCLUDED.input_tokens,
          output_tokens = EXCLUDED.output_tokens,
          provider_cost_usd = EXCLUDED.provider_cost_usd",
    )
    .bind(api_call_id)
    .bind(context.provider_model_id)
    .bind(context.provider_credential_id)
    .bind(completed.status_code)
    .bind(&completed.error_code)
    .bind(completed.retryable)
    .bind(completed.latency_ms)
    .bind(completed.input_tokens)
    .bind(completed.output_tokens)
    .bind(provider_cost)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;

    tx.commit().await.map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{money, would_exceed_budget};

    #[test]
    fn money_rejects_invalid_values() {
        assert!(money(-0.01).is_err());
        assert!(money(f64::NAN).is_err());
        assert!(money(f64::INFINITY).is_err());
    }

    #[test]
    fn money_uses_database_precision() {
        assert_eq!(money(0.123456789).unwrap(), "0.12345679");
        assert_eq!(money(0.0).unwrap(), "0.00000000");
    }

    #[test]
    fn pending_reservations_count_toward_the_limit() {
        assert!(would_exceed_budget(0.20, 0.25, 0.10, 0.50));
        assert!(!would_exceed_budget(0.20, 0.20, 0.10, 0.50));
    }

    #[test]
    fn zero_cost_byok_never_consumes_budget() {
        assert!(!would_exceed_budget(0.50, 0.0, 0.0, 0.50));
    }
}
