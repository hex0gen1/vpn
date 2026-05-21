// ---- Imports ----

use axum::Json;
use axum::{extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use thiserror::Error;

// ---- Structures ----

#[derive(Serialize, Deserialize)]
pub struct GenerateRequest {
    chat_id: i32,
    region: String,
    //protocol: String,
    //security: String,
}
#[derive(Serialize, Deserialize)]
pub struct GenerateResponse {
    vless_link: String,
    expires_at: String,
    server_name: String,
    max_devices: u8,
}
#[derive(sqlx::FromRow, Serialize)]
pub struct PublicServerInformation {
    pub id: i64,
    pub name: String,
    pub region: String,
    pub active_peers: Option<i64>,
    pub max_peers: Option<i64>,
    //pub load_percent: f64,
}

// ---- Functions ----

pub async fn generate_config(
    pool: State<sqlx::SqlitePool>,
    Json(req): Json<GenerateRequest>,
) -> Result<Json<GenerateResponse>, ApiError> {
    tracing::info!(
        "API request: chat_id: {}, region: {}",
        req.chat_id,
        req.region,
    );
    let mut tx = pool.begin().await?;
    let tg_id = req.chat_id;
    let region = req.region;
    let permission = check_user_permission(tg_id, pool).await?;

    if !permission {
        return Err(ApiError::AccessDenied);
    }
    let data = sqlx::query!(
        "UPDATE servers
                SET active_peers = active_peers + 1
                WHERE id = ( SELECT id FROM servers 
                WHERE region = ?AND active_peers < max_peers AND is_active = 1
                ORDER BY active_peers ASC LIMIT 1)

                RETURNING id, host, pbk, sid, sni",
        region
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NoServersAvailable)?;

    let uuid = uuid::Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + chrono::Duration::days(5);
    let exp = expires_at.to_rfc3339();

    let _ = sqlx::query!(
        "INSERT INTO configs (uuid, tg_id, server_id, expires_at) VALUES (?,?,?,?)",
        uuid,
        tg_id,
        data.id,
        exp
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!("UPDATE users SET trial_used = 1 WHERE tg_id = ?", tg_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    let link = format!(
        "vless://{}@{}:{}?security=reality&sni={}&pbk={}&sid={}&type=tcp&flow=xtls-rprx-vision#XTVPN",
        uuid, data.host, 443, data.sni, data.pbk, data.sid
    );
    Ok(Json(GenerateResponse {
        expires_at: expires_at.to_string(),
        vless_link: link,
        server_name: data.sni,
        max_devices: 2,
    }))
}
pub async fn revoke_config(
    pool: State<SqlitePool>,
    Json(body): Json<serde_json::Value>,
) -> Result<(), ApiError> {
    let uuid = body["uuid"].to_string();
    let mut tx = pool.begin().await?;

    sqlx::query!("DELETE FROM configs WHERE uuid = ?", uuid)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Database(e))?;

    Ok(())
}
pub async fn init_pool(db_path: &str) -> anyhow::Result<sqlx::SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(db_path)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(10))
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    Ok(pool)
}
async fn check_user_permission(tg_id: i32, pool: State<SqlitePool>) -> Result<bool, ApiError> {
    let mut tx = pool.begin().await?;
    let user = sqlx::query!(
        "SELECT sub_expiry, trial_used FROM users WHERE tg_id = ?",
        tg_id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::UserNotFound)?;

    let now = chrono::Utc::now().to_rfc3339();
    let has_access = user
        .sub_expiry
        .as_ref()
        .map(|d| d > &now)
        .expect("Invalid text")
        || user.trial_used == Some(false);

    Ok(has_access)
}
pub async fn insert_server(
    pool: State<SqlitePool>,
    Json(body): Json<serde_json::Value>,
) -> Result<(), ApiError> {
    let mut tx = pool.begin().await?;
    let host = body["host"].to_string();
    let name = body["name"].to_string();
    let port = body["port"]
        .as_i64()
        .ok_or(ApiError::BadRequest(body.to_string()))?;
    let region = body["region"].to_string();
    let pbk = body["pbk"].to_string();
    let sid = body["sid"].to_string();
    let sni = body["sni"].to_string();
    let max_peers = body["max_peers"]
        .as_i64()
        .ok_or(ApiError::BadRequest(body.to_string()))?;

    sqlx::query!(
        "INSERT INTO servers (host, name, region, port, pbk, sid, sni, max_peers, is_active) VALUES (?,?,?,?,?,?,?,?,?)",
        host,
        name,
        region,
        port,
        pbk,
        sid,
        sni,
        max_peers,
        1
    ).execute(&mut *tx).await?;

    tx.commit().await?;
    Ok(())
}
pub async fn list_public_servers(
    pool: State<SqlitePool>,
) -> Result<Json<Vec<PublicServerInformation>>, ApiError> {
    let mut tx = pool.begin().await?;
    let servers = sqlx::query_as!(PublicServerInformation, "SELECT id,name,region,active_peers,max_peers FROM servers WHERE is_active = 1 ORDER BY region ASC").fetch_all(&mut *tx).await.map_err(|e| ApiError::Database(e))?;
    Ok(Json(servers))
}
#[derive(Error, Debug)]
pub enum ApiError {
    #[error("User not found")]
    UserNotFound,

    #[error("Database error {0}")]
    Database(#[from] sqlx::Error),

    #[error("Access denied")]
    AccessDenied,

    #[error("No servers available")]
    NoServersAvailable,

    #[error("Bad request: {0}")]
    BadRequest(String),
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::http::Response<axum::body::Body> {
        let (status, client_msg) = match &self {
            ApiError::Database(e) => {
                tracing::debug!("Internal db error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database operation failed",
                )
            }
            ApiError::UserNotFound => (StatusCode::NOT_FOUND, "User not found"),
            ApiError::AccessDenied => (StatusCode::FORBIDDEN, "Access denied"),
            ApiError::NoServersAvailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Currently no servers available",
            ),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.as_str()),
        };
        (
            status,
            Json(serde_json::json!({"success": false, "error": client_msg})),
        )
            .into_response()
    }
}
