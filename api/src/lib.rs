use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{Path, Query, Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, SecondsFormat, Utc};
use http::{HeaderName, HeaderValue, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower_service::Service;
use wasm_bindgen::JsValue;
use worker::{Context, D1Database, Env, Result as WorkerResult};

#[derive(Clone)]
struct AppState {
    env: Env,
    token_owners: Arc<HashMap<String, String>>,
}

impl AppState {
    fn new(env: Env) -> Self {
        let token_owners = env
            .secret("API_BEARER_TOKENS_JSON")
            .ok()
            .and_then(|secret| {
                serde_json::from_str::<HashMap<String, String>>(&secret.to_string()).ok()
            })
            .unwrap_or_default()
            .into_iter()
            .filter(|(token, owner)| token.len() >= 32 && valid_identifier(owner))
            .map(|(token, owner)| (sha256_hex(token.as_bytes()), owner))
            .collect();

        Self {
            env,
            token_owners: Arc::new(token_owners),
        }
    }

    fn database(&self) -> ApiResult<D1Database> {
        self.env.d1("DB").map_err(|_| ApiError::Unavailable)
    }

    fn allows_time_simulation(&self) -> bool {
        self.env
            .var("ALLOW_TIME_SIMULATION")
            .map(|value| value.to_string() == "true")
            .unwrap_or(false)
    }
}

#[derive(Clone, Debug)]
struct Principal {
    owner_id: String,
}

#[derive(Clone, Copy, Debug)]
enum ApiError {
    Unauthorized,
    BadRequest(&'static str),
    NotFound,
    Conflict(&'static str),
    Unavailable,
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct ErrorBody {
            error: &'static str,
            message: &'static str,
        }

        let (status, error, message) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "有効なBearer tokenが必要です。",
            ),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "対象が見つからないか、アクセスできません。",
            ),
            Self::Conflict(message) => (StatusCode::CONFLICT, "conflict", message),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                "永続ストレージを利用できません。",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "処理に失敗しました。",
            ),
        };

        (status, Json(ErrorBody { error, message })).into_response()
    }
}

type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    mode: &'static str,
    persistence: &'static str,
    auth_configured: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Credential {
    id: String,
    reservation_id: Option<String>,
    status: String,
    provider: String,
    mode: String,
    presentment_supported: bool,
    issued_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Reservation {
    id: String,
    gate_id: String,
    starts_at: String,
    ends_at: String,
    status: String,
    mode: String,
    created_at: String,
    session_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Registration {
    id: String,
    credential_id: String,
    reservation_id: String,
    gate_id: String,
    status: String,
    adapter_mode: String,
    gate_applied: bool,
    physical_unlock_confirmed: bool,
    created_at: String,
    updated_at: String,
    session_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AuditEvent {
    id: String,
    session_id: String,
    action: String,
    result: String,
    detail: String,
    mode: String,
    created_at: String,
}

#[derive(Serialize)]
struct EventEnvelope {
    events: Vec<AuditEvent>,
}

#[derive(Serialize)]
struct AuthorizationDecision {
    allowed: bool,
    reason: &'static str,
    mode: &'static str,
    registration_status: String,
    reservation_status: String,
    evaluated_at: String,
    gate_applied: bool,
    physical_unlock_confirmed: bool,
}

#[derive(Deserialize, Serialize)]
struct IssueCredentialRequest {
    request_id: String,
    session_id: Option<String>,
    reservation_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct CreateReservationRequest {
    request_id: String,
    session_id: Option<String>,
    gate_id: String,
    starts_at: String,
    ends_at: String,
}

#[derive(Deserialize, Serialize)]
struct UpdateReservationRequest {
    request_id: String,
    session_id: Option<String>,
    starts_at: String,
    ends_at: String,
}

#[derive(Deserialize, Serialize)]
struct MutationRequest {
    request_id: String,
    session_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct CreateRegistrationRequest {
    request_id: String,
    session_id: Option<String>,
    credential_id: String,
    reservation_id: String,
    gate_id: String,
    simulate_failure: Option<bool>,
}

#[derive(Deserialize, Serialize)]
struct RetryRegistrationRequest {
    request_id: String,
    session_id: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct AuthorizationRequest {
    session_id: String,
    registration_id: String,
    evaluated_at: Option<String>,
}

#[derive(Deserialize)]
struct EventQuery {
    session_id: String,
}

#[derive(Deserialize)]
struct IdempotencyRow {
    request_hash: String,
    response_status: u16,
    response_json: String,
}

#[derive(Deserialize)]
struct CredentialRow {
    status: String,
    body_json: String,
}

#[derive(Deserialize)]
struct ReservationRow {
    gate_id: String,
    starts_at: String,
    ends_at: String,
    status: String,
    session_id: String,
    body_json: String,
}

#[derive(Deserialize)]
struct RegistrationRow {
    status: String,
    gate_applied: i64,
    updated_at: String,
    session_id: String,
    body_json: String,
}

trait CredentialProvider {
    fn issue(&self, reservation_id: Option<String>) -> Credential;
}

struct MockCredentialProvider;

impl CredentialProvider for MockCredentialProvider {
    fn issue(&self, reservation_id: Option<String>) -> Credential {
        Credential {
            id: new_id(),
            reservation_id,
            status: "issued".to_owned(),
            provider: "mock".to_owned(),
            mode: "mock".to_owned(),
            presentment_supported: false,
            issued_at: now_iso(),
        }
    }
}

struct AdapterOutcome {
    status: &'static str,
    gate_applied: bool,
}

trait LockAdapter {
    fn register(&self, simulate_failure: bool) -> AdapterOutcome;
    fn revoke(&self) -> AdapterOutcome;
}

struct MockLockAdapter;

impl LockAdapter for MockLockAdapter {
    fn register(&self, simulate_failure: bool) -> AdapterOutcome {
        AdapterOutcome {
            status: if simulate_failure {
                "failed"
            } else {
                "registered"
            },
            gate_applied: false,
        }
    }

    fn revoke(&self) -> AdapterOutcome {
        AdapterOutcome {
            status: "revoked",
            gate_applied: false,
        }
    }
}

#[derive(Clone)]
enum SqlValue {
    Text(String),
    Integer(i64),
}

fn text(value: impl Into<String>) -> SqlValue {
    SqlValue::Text(value.into())
}

fn integer(value: i64) -> SqlValue {
    SqlValue::Integer(value)
}

fn statement(
    db: &D1Database,
    sql: &str,
    values: Vec<SqlValue>,
) -> WorkerResult<worker::D1PreparedStatement> {
    let bindings = values
        .into_iter()
        .map(|value| match value {
            SqlValue::Text(value) => JsValue::from_str(&value),
            SqlValue::Integer(value) => JsValue::from_f64(value as f64),
        })
        .collect::<Vec<_>>();
    db.prepare(sql).bind(&bindings)
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_request_id(value: &str) -> ApiResult<()> {
    if valid_identifier(value) {
        Ok(())
    } else {
        Err(ApiError::BadRequest("request_idの形式が正しくありません。"))
    }
}

fn session_id(request_id: &str, value: Option<&str>) -> ApiResult<String> {
    let value = value.unwrap_or(request_id);
    if valid_identifier(value) {
        Ok(value.to_owned())
    } else {
        Err(ApiError::BadRequest("session_idの形式が正しくありません。"))
    }
}

fn request_session_id(headers: &http::HeaderMap, fallback: &str) -> ApiResult<String> {
    let value = headers
        .get("x-session-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or(fallback);
    if valid_identifier(value) {
        Ok(value.to_owned())
    } else {
        Err(ApiError::BadRequest("session_idの形式が正しくありません。"))
    }
}

fn normalize_time(value: &str) -> ApiResult<String> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| ApiError::BadRequest("日時はRFC 3339形式で指定してください。"))?;
    Ok(parsed
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn now_iso() -> String {
    js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_owned())
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn sha256_hex(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

fn fingerprint<T: Serialize>(value: &T) -> ApiResult<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| ApiError::Internal)?;
    Ok(sha256_hex(&bytes))
}

fn json<T: Serialize>(value: &T) -> ApiResult<String> {
    serde_json::to_string(value).map_err(|_| ApiError::Internal)
}

async fn find_replay(
    db: &D1Database,
    owner_id: &str,
    route: &str,
    request_id: &str,
    request_hash: &str,
) -> ApiResult<Option<Response>> {
    let query = statement(
        db,
        "SELECT request_hash, response_status, response_json FROM idempotency_keys \
         WHERE owner_id = ?1 AND route = ?2 AND request_id = ?3",
        vec![text(owner_id), text(route), text(request_id)],
    )
    .map_err(|_| ApiError::Internal)?;
    let Some(row) = query
        .first::<IdempotencyRow>(None)
        .await
        .map_err(|_| ApiError::Internal)?
    else {
        return Ok(None);
    };

    if row.request_hash != request_hash {
        return Err(ApiError::Conflict(
            "同じrequest_idが異なる内容です。新しいrequest_idを使用してください。",
        ));
    }

    let payload =
        serde_json::from_str::<Value>(&row.response_json).map_err(|_| ApiError::Internal)?;
    let status = StatusCode::from_u16(row.response_status).unwrap_or(StatusCode::OK);
    let mut response = (status, Json(payload)).into_response();
    response.headers_mut().insert(
        HeaderName::from_static("idempotency-replayed"),
        HeaderValue::from_static("true"),
    );
    Ok(Some(response))
}

fn event_statement(
    db: &D1Database,
    owner_id: &str,
    event: &AuditEvent,
) -> ApiResult<worker::D1PreparedStatement> {
    statement(
        db,
        "INSERT INTO audit_events \
         (id, owner_id, session_id, action, result, detail, mode, occurred_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        vec![
            text(&event.id),
            text(owner_id),
            text(&event.session_id),
            text(&event.action),
            text(&event.result),
            text(&event.detail),
            text(&event.mode),
            text(&event.created_at),
        ],
    )
    .map_err(|_| ApiError::Internal)
}

fn idempotency_statement(
    db: &D1Database,
    owner_id: &str,
    route: &str,
    request_id: &str,
    request_hash: &str,
    status: StatusCode,
    response_json: &str,
) -> ApiResult<worker::D1PreparedStatement> {
    statement(
        db,
        "INSERT INTO idempotency_keys \
         (owner_id, route, request_id, request_hash, response_status, response_json, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        vec![
            text(owner_id),
            text(route),
            text(request_id),
            text(request_hash),
            integer(i64::from(status.as_u16())),
            text(response_json),
            text(now_iso()),
        ],
    )
    .map_err(|_| ApiError::Internal)
}

async fn commit_mutation<T: Serialize>(
    db: &D1Database,
    owner_id: &str,
    route: &str,
    request_id: &str,
    request_hash: &str,
    status: StatusCode,
    response: &T,
    mut statements: Vec<worker::D1PreparedStatement>,
) -> ApiResult<Option<Response>> {
    let response_json = json(response)?;
    statements.push(idempotency_statement(
        db,
        owner_id,
        route,
        request_id,
        request_hash,
        status,
        &response_json,
    )?);

    let failure = match db.batch(statements).await {
        Ok(results) => results
            .iter()
            .filter(|result| !result.success())
            .filter_map(|result| result.error())
            .collect::<Vec<_>>()
            .join(" "),
        Err(error) => error.to_string(),
    };

    if failure.is_empty() {
        return Ok(None);
    }

    if let Some(replay) = find_replay(db, owner_id, route, request_id, request_hash).await? {
        return Ok(Some(replay));
    }
    if failure.contains("reservation_overlap") {
        return Err(ApiError::Conflict("同じゲートに重複する予約があります。"));
    }
    if failure.contains("registration_requires_active_reservation") {
        return Err(ApiError::Conflict("予約が取消済みのため登録を作成できません。"));
    }
    if failure.contains("registration_retry_requires_failed") {
        return Err(ApiError::Conflict("失敗状態の登録のみ再試行できます。"));
    }
    if failure.contains("credential_requires_active_reservation") {
        return Err(ApiError::Conflict("予約が取消済みのため資格情報を発行できません。"));
    }
    if failure.contains("reservation_update_requires_active") {
        return Err(ApiError::Conflict("取消済み予約は変更できません。"));
    }
    if failure.contains("registration_already_exists") {
        return Err(ApiError::Conflict(
            "同じ資格情報と予約の登録がすでに存在します。状態取得または失敗時の再試行を行ってください。",
        ));
    }
    Err(ApiError::Internal)
}

fn audit_event(session_id: &str, action: &str, result: &str, detail: &str) -> AuditEvent {
    AuditEvent {
        id: new_id(),
        session_id: session_id.to_owned(),
        action: action.to_owned(),
        result: result.to_owned(),
        detail: detail.to_owned(),
        mode: "mock".to_owned(),
        created_at: now_iso(),
    }
}

fn authorization_decision(
    registration_status: &str,
    credential_status: &str,
    reservation_status: &str,
    starts_at: &str,
    ends_at: &str,
    evaluated_at: &str,
) -> (bool, &'static str) {
    if registration_status != "registered" {
        (false, "registration_inactive")
    } else if credential_status != "issued" {
        (false, "credential_inactive")
    } else if reservation_status != "active" {
        (false, "reservation_cancelled")
    } else if evaluated_at < starts_at {
        (false, "before_start")
    } else if evaluated_at >= ends_at {
        (false, "at_or_after_end")
    } else {
        (true, "within_window")
    }
}

fn credential_from_row(row: CredentialRow) -> ApiResult<Credential> {
    let mut credential: Credential =
        serde_json::from_str(&row.body_json).map_err(|_| ApiError::Internal)?;
    credential.status = row.status;
    Ok(credential)
}

fn reservation_from_row(row: ReservationRow) -> ApiResult<Reservation> {
    let mut reservation: Reservation =
        serde_json::from_str(&row.body_json).map_err(|_| ApiError::Internal)?;
    reservation.gate_id = row.gate_id;
    reservation.starts_at = row.starts_at;
    reservation.ends_at = row.ends_at;
    reservation.status = row.status;
    reservation.session_id = row.session_id;
    Ok(reservation)
}

fn registration_from_row(row: RegistrationRow) -> ApiResult<Registration> {
    let mut registration: Registration =
        serde_json::from_str(&row.body_json).map_err(|_| ApiError::Internal)?;
    registration.status = row.status;
    registration.gate_applied = row.gate_applied != 0;
    registration.updated_at = row.updated_at;
    registration.session_id = row.session_id;
    Ok(registration)
}

async fn credential_by_id(
    db: &D1Database,
    owner_id: &str,
    id: &str,
) -> ApiResult<Option<Credential>> {
    let query = statement(
        db,
        "SELECT status, body_json FROM credentials WHERE owner_id = ?1 AND id = ?2",
        vec![text(owner_id), text(id)],
    )
    .map_err(|_| ApiError::Internal)?;
    query
        .first::<CredentialRow>(None)
        .await
        .map_err(|_| ApiError::Internal)?
        .map(credential_from_row)
        .transpose()
}

async fn reservation_by_id(
    db: &D1Database,
    owner_id: &str,
    id: &str,
) -> ApiResult<Option<Reservation>> {
    let query = statement(
        db,
        "SELECT gate_id, starts_at, ends_at, status, session_id, body_json \
         FROM reservations WHERE owner_id = ?1 AND id = ?2",
        vec![text(owner_id), text(id)],
    )
    .map_err(|_| ApiError::Internal)?;
    query
        .first::<ReservationRow>(None)
        .await
        .map_err(|_| ApiError::Internal)?
        .map(reservation_from_row)
        .transpose()
}

async fn registration_by_id(
    db: &D1Database,
    owner_id: &str,
    id: &str,
) -> ApiResult<Option<Registration>> {
    let query = statement(
        db,
        "SELECT status, gate_applied, updated_at, session_id, body_json \
         FROM registrations WHERE owner_id = ?1 AND id = ?2",
        vec![text(owner_id), text(id)],
    )
    .map_err(|_| ApiError::Internal)?;
    query
        .first::<RegistrationRow>(None)
        .await
        .map_err(|_| ApiError::Internal)?
        .map(registration_from_row)
        .transpose()
}

fn credential_insert(
    db: &D1Database,
    owner_id: &str,
    credential: &Credential,
) -> ApiResult<worker::D1PreparedStatement> {
    statement(
        db,
        "INSERT INTO credentials (id, owner_id, reservation_id, status, provider, issued_at, body_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        vec![
            text(&credential.id),
            text(owner_id),
            text(credential.reservation_id.clone().unwrap_or_default()),
            text(&credential.status),
            text(&credential.provider),
            text(&credential.issued_at),
            text(json(credential)?),
        ],
    )
    .map_err(|_| ApiError::Internal)
}

fn reservation_insert(
    db: &D1Database,
    owner_id: &str,
    reservation: &Reservation,
) -> ApiResult<worker::D1PreparedStatement> {
    statement(
        db,
        "INSERT INTO reservations \
         (id, owner_id, gate_id, starts_at, ends_at, status, created_at, session_id, body_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        vec![
            text(&reservation.id),
            text(owner_id),
            text(&reservation.gate_id),
            text(&reservation.starts_at),
            text(&reservation.ends_at),
            text(&reservation.status),
            text(&reservation.created_at),
            text(&reservation.session_id),
            text(json(reservation)?),
        ],
    )
    .map_err(|_| ApiError::Internal)
}

fn registration_insert(
    db: &D1Database,
    owner_id: &str,
    registration: &Registration,
) -> ApiResult<worker::D1PreparedStatement> {
    statement(
        db,
        "INSERT INTO registrations \
         (id, owner_id, credential_id, reservation_id, gate_id, status, adapter_mode, gate_applied, \
          created_at, updated_at, session_id, body_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        vec![
            text(&registration.id),
            text(owner_id),
            text(&registration.credential_id),
            text(&registration.reservation_id),
            text(&registration.gate_id),
            text(&registration.status),
            text(&registration.adapter_mode),
            integer(i64::from(registration.gate_applied)),
            text(&registration.created_at),
            text(&registration.updated_at),
            text(&registration.session_id),
            text(json(registration)?),
        ],
    )
    .map_err(|_| ApiError::Internal)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        mode: "mock",
        persistence: "d1",
        auth_configured: !state.token_owners.is_empty(),
    })
}

#[worker::send]
async fn authenticate(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    if request.uri().path() == "/healthz" {
        return next.run(request).await;
    }

    let token = request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(token) = token else {
        return ApiError::Unauthorized.into_response();
    };
    let token_hash = sha256_hex(token.as_bytes());
    let Some(owner_id) = state.token_owners.get(&token_hash) else {
        return ApiError::Unauthorized.into_response();
    };
    request.extensions_mut().insert(Principal {
        owner_id: owner_id.clone(),
    });
    next.run(request).await
}

#[derive(Serialize)]
struct IdentityResponse {
    identity: String,
}

#[worker::send]
async fn get_identity(
    axum::Extension(principal): axum::Extension<Principal>,
) -> Json<IdentityResponse> {
    Json(IdentityResponse {
        identity: sha256_hex(principal.owner_id.as_bytes()),
    })
}

#[worker::send]
async fn issue_credential(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Json(request): Json<IssueCredentialRequest>,
) -> ApiResult<Response> {
    validate_request_id(&request.request_id)?;
    let session_id = session_id(&request.request_id, request.session_id.as_deref())?;
    let request_hash = fingerprint(&request)?;
    let db = state.database()?;
    let route = "POST:/v1/credentials/issue";
    if let Some(replay) = find_replay(
        &db,
        &principal.owner_id,
        route,
        &request.request_id,
        &request_hash,
    )
    .await?
    {
        return Ok(replay);
    }

    if let Some(reservation_id) = request.reservation_id.as_deref() {
        let reservation = reservation_by_id(&db, &principal.owner_id, reservation_id)
            .await?
            .ok_or(ApiError::NotFound)?;
        if reservation.status != "active" {
            return Err(ApiError::Conflict(
                "有効な予約に対してのみ資格情報を発行できます。",
            ));
        }
    }

    let credential = MockCredentialProvider.issue(request.reservation_id.clone());
    let event = audit_event(
        &session_id,
        "credential_issue",
        "issued",
        "モック資格情報参照を作成しました。NFC情報は発行していません。",
    );
    let mut statements = vec![
        credential_insert(&db, &principal.owner_id, &credential)?,
        event_statement(&db, &principal.owner_id, &event)?,
    ];
    if let Some(replay) = commit_mutation(
        &db,
        &principal.owner_id,
        route,
        &request.request_id,
        &request_hash,
        StatusCode::CREATED,
        &credential,
        std::mem::take(&mut statements),
    )
    .await?
    {
        return Ok(replay);
    }
    Ok((StatusCode::CREATED, Json(credential)).into_response())
}

#[worker::send]
async fn get_credential(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult<Json<Credential>> {
    let db = state.database()?;
    credential_by_id(&db, &principal.owner_id, &id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[worker::send]
async fn create_reservation(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Json(request): Json<CreateReservationRequest>,
) -> ApiResult<Response> {
    validate_request_id(&request.request_id)?;
    if !valid_identifier(&request.gate_id) {
        return Err(ApiError::BadRequest("gate_idの形式が正しくありません。"));
    }
    let session_id = session_id(&request.request_id, request.session_id.as_deref())?;
    let starts_at = normalize_time(&request.starts_at)?;
    let ends_at = normalize_time(&request.ends_at)?;
    if starts_at >= ends_at {
        return Err(ApiError::BadRequest(
            "終了日時は開始日時より後にしてください。",
        ));
    }
    let request_hash = fingerprint(&request)?;
    let db = state.database()?;
    let route = "POST:/v1/reservations";
    if let Some(replay) = find_replay(
        &db,
        &principal.owner_id,
        route,
        &request.request_id,
        &request_hash,
    )
    .await?
    {
        return Ok(replay);
    }

    let reservation = Reservation {
        id: new_id(),
        gate_id: request.gate_id,
        starts_at,
        ends_at,
        status: "active".to_owned(),
        mode: "mock".to_owned(),
        created_at: now_iso(),
        session_id: session_id.clone(),
    };
    let event = audit_event(
        &session_id,
        "reservation_create",
        "active",
        "モックテスト予約を作成しました。",
    );
    let statements = vec![
        reservation_insert(&db, &principal.owner_id, &reservation)?,
        event_statement(&db, &principal.owner_id, &event)?,
    ];
    if let Some(replay) = commit_mutation(
        &db,
        &principal.owner_id,
        route,
        &request.request_id,
        &request_hash,
        StatusCode::CREATED,
        &reservation,
        statements,
    )
    .await?
    {
        return Ok(replay);
    }
    Ok((StatusCode::CREATED, Json(reservation)).into_response())
}

#[worker::send]
async fn get_reservation(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult<Json<Reservation>> {
    let db = state.database()?;
    reservation_by_id(&db, &principal.owner_id, &id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[worker::send]
async fn update_reservation(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
    Json(request): Json<UpdateReservationRequest>,
) -> ApiResult<Response> {
    validate_request_id(&request.request_id)?;
    let session_id = session_id(&request.request_id, request.session_id.as_deref())?;
    let starts_at = normalize_time(&request.starts_at)?;
    let ends_at = normalize_time(&request.ends_at)?;
    if starts_at >= ends_at {
        return Err(ApiError::BadRequest(
            "終了日時は開始日時より後にしてください。",
        ));
    }
    let request_hash = fingerprint(&request)?;
    let db = state.database()?;
    let route = format!("PATCH:/v1/reservations/{id}");
    if let Some(replay) = find_replay(
        &db,
        &principal.owner_id,
        &route,
        &request.request_id,
        &request_hash,
    )
    .await?
    {
        return Ok(replay);
    }
    let mut reservation = reservation_by_id(&db, &principal.owner_id, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if reservation.status != "active" {
        return Err(ApiError::Conflict("取消済み予約は変更できません。"));
    }
    reservation.starts_at = starts_at.clone();
    reservation.ends_at = ends_at.clone();
    reservation.session_id = session_id.clone();
    let update = statement(
        &db,
        "UPDATE reservations SET starts_at = ?1, ends_at = ?2, session_id = ?3, body_json = ?4 \
         WHERE owner_id = ?5 AND id = ?6",
        vec![
            text(starts_at),
            text(ends_at),
            text(&session_id),
            text(json(&reservation)?),
            text(&principal.owner_id),
            text(&id),
        ],
    )
    .map_err(|_| ApiError::Internal)?;
    let event = audit_event(
        &session_id,
        "reservation_update",
        "active",
        "モックテスト予約の有効時間を変更しました。",
    );
    let statements = vec![update, event_statement(&db, &principal.owner_id, &event)?];
    if let Some(replay) = commit_mutation(
        &db,
        &principal.owner_id,
        &route,
        &request.request_id,
        &request_hash,
        StatusCode::OK,
        &reservation,
        statements,
    )
    .await?
    {
        return Ok(replay);
    }
    Ok(Json(reservation).into_response())
}

#[worker::send]
async fn cancel_reservation(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
    headers: http::HeaderMap,
) -> ApiResult<Response> {
    let request_id = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::BadRequest("Idempotency-Key headerが必要です。"))?;
    validate_request_id(request_id)?;
    let session_id = request_session_id(&headers, request_id)?;
    let request = (id.as_str(), request_id);
    let request_hash = fingerprint(&request)?;
    let db = state.database()?;
    let route = format!("DELETE:/v1/reservations/{id}");
    if let Some(replay) =
        find_replay(&db, &principal.owner_id, &route, request_id, &request_hash).await?
    {
        return Ok(replay);
    }
    let mut reservation = reservation_by_id(&db, &principal.owner_id, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if reservation.status == "cancelled" {
        return Err(ApiError::Conflict("予約はすでに取消済みです。"));
    }
    reservation.status = "cancelled".to_owned();
    reservation.session_id = session_id.clone();
    let cancel = statement(
        &db,
        "UPDATE reservations SET status = 'cancelled', session_id = ?1 \
         WHERE owner_id = ?2 AND id = ?3 AND status = 'active'",
        vec![text(&session_id), text(&principal.owner_id), text(&id)],
    )
    .map_err(|_| ApiError::Internal)?;
    let revoke_registrations = statement(
        &db,
        "UPDATE registrations SET status = 'revoked', gate_applied = 0, updated_at = ?1, session_id = ?2 \
         WHERE owner_id = ?3 AND reservation_id = ?4 \
           AND status IN ('registration_pending', 'registered', 'revocation_pending', 'failed')",
        vec![text(now_iso()), text(&session_id), text(&principal.owner_id), text(&id)],
    )
    .map_err(|_| ApiError::Internal)?;
    let revoke_credentials = statement(
        &db,
        "UPDATE credentials SET status = 'revoked' \
         WHERE owner_id = ?1 AND reservation_id = ?2 AND status = 'issued'",
        vec![text(&principal.owner_id), text(&id)],
    )
    .map_err(|_| ApiError::Internal)?;
    let event = audit_event(
        &session_id,
        "reservation_cancel",
        "cancelled",
        "予約を取消し、関連するモック権限を失効しました。",
    );
    let statements = vec![
        cancel,
        revoke_registrations,
        revoke_credentials,
        event_statement(&db, &principal.owner_id, &event)?,
    ];
    if let Some(replay) = commit_mutation(
        &db,
        &principal.owner_id,
        &route,
        request_id,
        &request_hash,
        StatusCode::OK,
        &reservation,
        statements,
    )
    .await?
    {
        return Ok(replay);
    }
    Ok(Json(reservation).into_response())
}

#[worker::send]
async fn create_registration(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Json(request): Json<CreateRegistrationRequest>,
) -> ApiResult<Response> {
    validate_request_id(&request.request_id)?;
    if !valid_identifier(&request.gate_id) {
        return Err(ApiError::BadRequest("gate_idの形式が正しくありません。"));
    }
    let session_id = session_id(&request.request_id, request.session_id.as_deref())?;
    let simulate_failure = request.simulate_failure.unwrap_or(false);
    if simulate_failure && !state.allows_time_simulation() {
        return Err(ApiError::BadRequest(
            "simulate_failureはローカルモック検証でのみ指定できます。",
        ));
    }
    let request_hash = fingerprint(&request)?;
    let db = state.database()?;
    let route = "POST:/v1/registrations";
    if let Some(replay) = find_replay(
        &db,
        &principal.owner_id,
        route,
        &request.request_id,
        &request_hash,
    )
    .await?
    {
        return Ok(replay);
    }
    let credential = credential_by_id(&db, &principal.owner_id, &request.credential_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if credential.status != "issued"
        || credential
            .reservation_id
            .as_ref()
            .is_some_and(|reservation_id| reservation_id != &request.reservation_id)
    {
        return Err(ApiError::Conflict(
            "資格情報が有効でないか、予約と一致しません。",
        ));
    }
    let reservation = reservation_by_id(&db, &principal.owner_id, &request.reservation_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if reservation.status != "active" || reservation.gate_id != request.gate_id {
        return Err(ApiError::Conflict("有効な予約とゲートが一致しません。"));
    }

    let duplicate = statement(
        &db,
        "SELECT status, gate_applied, updated_at, session_id, body_json FROM registrations \
         WHERE owner_id = ?1 AND credential_id = ?2 AND reservation_id = ?3 AND gate_id = ?4 \
           AND status IN ('registration_pending', 'registered', 'failed') LIMIT 1",
        vec![
            text(&principal.owner_id),
            text(&request.credential_id),
            text(&request.reservation_id),
            text(&request.gate_id),
        ],
    )
    .map_err(|_| ApiError::Internal)?;
    if duplicate
        .first::<RegistrationRow>(None)
        .await
        .map_err(|_| ApiError::Internal)?
        .is_some()
    {
        return Err(ApiError::Conflict(
            "同じ資格情報と予約の登録がすでに存在します。状態取得または失敗時の再試行を行ってください。",
        ));
    }

    let adapter = MockLockAdapter.register(simulate_failure);
    let now = now_iso();
    let registration = Registration {
        id: new_id(),
        credential_id: request.credential_id,
        reservation_id: request.reservation_id,
        gate_id: request.gate_id,
        status: adapter.status.to_owned(),
        adapter_mode: "mock".to_owned(),
        gate_applied: adapter.gate_applied,
        physical_unlock_confirmed: false,
        created_at: now.clone(),
        updated_at: now,
        session_id: session_id.clone(),
    };
    let event = audit_event(
        &session_id,
        "registration_create",
        adapter.status,
        if simulate_failure {
            "ローカル検証でモック失敗を再現しました。ゲートには接続していません。"
        } else {
            "モックアダプター上で登録状態を作成しました。ゲートへの反映はありません。"
        },
    );
    let statements = vec![
        registration_insert(&db, &principal.owner_id, &registration)?,
        event_statement(&db, &principal.owner_id, &event)?,
    ];
    if let Some(replay) = commit_mutation(
        &db,
        &principal.owner_id,
        route,
        &request.request_id,
        &request_hash,
        StatusCode::CREATED,
        &registration,
        statements,
    )
    .await?
    {
        return Ok(replay);
    }
    Ok((StatusCode::CREATED, Json(registration)).into_response())
}

#[worker::send]
async fn retry_registration(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
    Json(request): Json<RetryRegistrationRequest>,
) -> ApiResult<Response> {
    validate_request_id(&request.request_id)?;
    let session_id = session_id(&request.request_id, request.session_id.as_deref())?;
    let request_hash = fingerprint(&(id.as_str(), &request))?;
    let db = state.database()?;
    let route = format!("POST:/v1/registrations/{id}/retry");
    if let Some(replay) = find_replay(
        &db,
        &principal.owner_id,
        &route,
        &request.request_id,
        &request_hash,
    )
    .await?
    {
        return Ok(replay);
    }
    let mut registration = registration_by_id(&db, &principal.owner_id, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if registration.status != "failed" {
        return Err(ApiError::Conflict("失敗状態の登録のみ再試行できます。"));
    }
    let credential = credential_by_id(&db, &principal.owner_id, &registration.credential_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let reservation = reservation_by_id(&db, &principal.owner_id, &registration.reservation_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if credential.status != "issued" || reservation.status != "active" {
        return Err(ApiError::Conflict(
            "資格情報または予約が有効ではありません。",
        ));
    }

    let adapter = MockLockAdapter.register(false);
    registration.status = adapter.status.to_owned();
    registration.gate_applied = adapter.gate_applied;
    registration.physical_unlock_confirmed = false;
    registration.updated_at = now_iso();
    registration.session_id = session_id.clone();
    let update = statement(
        &db,
        "UPDATE registrations SET status = ?1, gate_applied = ?2, updated_at = ?3, session_id = ?4 \
         WHERE owner_id = ?5 AND id = ?6",
        vec![
            text(&registration.status),
            integer(i64::from(registration.gate_applied)),
            text(&registration.updated_at),
            text(&session_id),
            text(&principal.owner_id),
            text(&id),
        ],
    )
    .map_err(|_| ApiError::Internal)?;
    let event = audit_event(
        &session_id,
        "registration_retry",
        "registered",
        "失敗したモック登録を再試行しました。ゲートには接続していません。",
    );
    let statements = vec![update, event_statement(&db, &principal.owner_id, &event)?];
    if let Some(replay) = commit_mutation(
        &db,
        &principal.owner_id,
        &route,
        &request.request_id,
        &request_hash,
        StatusCode::OK,
        &registration,
        statements,
    )
    .await?
    {
        return Ok(replay);
    }
    Ok(Json(registration).into_response())
}

#[worker::send]
async fn get_registration(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult<Json<Registration>> {
    let db = state.database()?;
    registration_by_id(&db, &principal.owner_id, &id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[worker::send]
async fn cancel_registration(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
    headers: http::HeaderMap,
) -> ApiResult<Response> {
    let request_id = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::BadRequest("Idempotency-Key headerが必要です。"))?;
    validate_request_id(request_id)?;
    let session_id = request_session_id(&headers, request_id)?;
    let request_hash = fingerprint(&(id.as_str(), request_id))?;
    let db = state.database()?;
    let route = format!("DELETE:/v1/registrations/{id}");
    if let Some(replay) =
        find_replay(&db, &principal.owner_id, &route, request_id, &request_hash).await?
    {
        return Ok(replay);
    }
    let mut registration = registration_by_id(&db, &principal.owner_id, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if registration.status == "revoked" {
        return Err(ApiError::Conflict("登録はすでに取消済みです。"));
    }
    let adapter = MockLockAdapter.revoke();
    registration.status = adapter.status.to_owned();
    registration.gate_applied = adapter.gate_applied;
    registration.updated_at = now_iso();
    registration.session_id = session_id.clone();
    let update = statement(
        &db,
        "UPDATE registrations SET status = ?1, gate_applied = ?2, updated_at = ?3, session_id = ?4 \
         WHERE owner_id = ?5 AND id = ?6 AND status <> 'revoked'",
        vec![
            text(&registration.status),
            integer(i64::from(registration.gate_applied)),
            text(&registration.updated_at),
            text(&registration.session_id),
            text(&principal.owner_id),
            text(&id),
        ],
    )
    .map_err(|_| ApiError::Internal)?;
    let event = audit_event(
        &session_id,
        "registration_revoke",
        "revoked",
        "モック登録を失効しました。ゲートには接続していません。",
    );
    let statements = vec![update, event_statement(&db, &principal.owner_id, &event)?];
    if let Some(replay) = commit_mutation(
        &db,
        &principal.owner_id,
        &route,
        request_id,
        &request_hash,
        StatusCode::OK,
        &registration,
        statements,
    )
    .await?
    {
        return Ok(replay);
    }
    Ok(Json(registration).into_response())
}

#[worker::send]
async fn check_authorization(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Json(request): Json<AuthorizationRequest>,
) -> ApiResult<Json<AuthorizationDecision>> {
    if !valid_identifier(&request.session_id) {
        return Err(ApiError::BadRequest("session_idの形式が正しくありません。"));
    }
    let evaluated_at = match request.evaluated_at.as_deref() {
        Some(value) if state.allows_time_simulation() => normalize_time(value)?,
        Some(_) => {
            return Err(ApiError::BadRequest(
                "evaluated_atはローカル時間シミュレーションでのみ指定できます。",
            ))
        }
        None => normalize_time(&now_iso())?,
    };
    let db = state.database()?;
    let registration = registration_by_id(&db, &principal.owner_id, &request.registration_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let reservation = reservation_by_id(&db, &principal.owner_id, &registration.reservation_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let credential = credential_by_id(&db, &principal.owner_id, &registration.credential_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    let (allowed, reason) = authorization_decision(
        &registration.status,
        &credential.status,
        &reservation.status,
        &reservation.starts_at,
        &reservation.ends_at,
        &evaluated_at,
    );
    let decision = AuthorizationDecision {
        allowed,
        reason,
        mode: "mock",
        registration_status: registration.status,
        reservation_status: reservation.status,
        evaluated_at,
        gate_applied: false,
        physical_unlock_confirmed: false,
    };
    let event = audit_event(
        &request.session_id,
        "authorization_check",
        if allowed { "allowed" } else { "denied" },
        "モック時間・登録状態の判定です。物理ゲートには照会していません。",
    );
    let insert = event_statement(&db, &principal.owner_id, &event)?;
    db.batch(vec![insert])
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(Json(decision))
}

#[worker::send]
async fn list_events(
    State(state): State<AppState>,
    axum::Extension(principal): axum::Extension<Principal>,
    Query(query): Query<EventQuery>,
) -> ApiResult<Json<EventEnvelope>> {
    if !valid_identifier(&query.session_id) {
        return Err(ApiError::BadRequest("session_idの形式が正しくありません。"));
    }
    let db = state.database()?;
    let statement = statement(
        &db,
        "SELECT id, session_id, action, result, detail, mode, occurred_at AS created_at \
         FROM audit_events WHERE owner_id = ?1 AND session_id = ?2 \
         ORDER BY occurred_at DESC, id DESC LIMIT 500",
        vec![text(&principal.owner_id), text(&query.session_id)],
    )
    .map_err(|_| ApiError::Internal)?;
    let results = statement.all().await.map_err(|_| ApiError::Internal)?;
    let mut events = results
        .results::<AuditEvent>()
        .map_err(|_| ApiError::Internal)?;
    events.reverse();
    Ok(Json(EventEnvelope { events }))
}

fn app_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/identity", get(get_identity))
        .route(
            "/v1/credentials/issue",
            axum::routing::post(issue_credential),
        )
        .route("/v1/credentials/{id}", get(get_credential))
        .route("/v1/reservations", axum::routing::post(create_reservation))
        .route(
            "/v1/reservations/{id}",
            get(get_reservation)
                .patch(update_reservation)
                .delete(cancel_reservation),
        )
        .route(
            "/v1/registrations",
            axum::routing::post(create_registration),
        )
        .route(
            "/v1/registrations/{id}",
            get(get_registration).delete(cancel_registration),
        )
        .route(
            "/v1/registrations/{id}/retry",
            axum::routing::post(retry_registration),
        )
        .route(
            "/v1/authorizations/check",
            axum::routing::post(check_authorization),
        )
        .route("/v1/events", get(list_events))
        .layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .with_state(state)
}

#[event(fetch)]
async fn fetch(
    request: worker::HttpRequest,
    env: Env,
    _context: Context,
) -> WorkerResult<http::Response<axum::body::Body>> {
    let mut router = app_router(AppState::new(env));
    Ok(router.call(request).await?)
}

#[cfg(test)]
mod tests {
    use super::{authorization_decision, normalize_time, sha256_hex};

    #[test]
    fn normalizes_offsets_before_comparison() {
        assert_eq!(
            normalize_time("2026-09-28T12:00:00+09:00").unwrap(),
            "2026-09-28T03:00:00.000Z"
        );
    }

    #[test]
    fn hashes_tokens_without_retaining_them_as_map_keys() {
        assert_eq!(
            sha256_hex(b"local-only-example-token"),
            "f7d43ad7511a9a13339cb8f81738cd10878a22eb94c0694e0f20e5a147696deb"
        );
    }

    #[test]
    fn reservation_window_includes_start_and_excludes_end() {
        let start = "2026-09-28T03:00:00.000Z";
        let end = "2026-09-28T04:00:00.000Z";
        let active = ("registered", "issued", "active");

        assert_eq!(
            authorization_decision(
                active.0,
                active.1,
                active.2,
                start,
                end,
                "2026-09-28T02:59:59.999Z"
            ),
            (false, "before_start")
        );
        assert_eq!(
            authorization_decision(active.0, active.1, active.2, start, end, start),
            (true, "within_window")
        );
        assert_eq!(
            authorization_decision(
                active.0,
                active.1,
                active.2,
                start,
                end,
                "2026-09-28T03:59:59.999Z"
            ),
            (true, "within_window")
        );
        assert_eq!(
            authorization_decision(active.0, active.1, active.2, start, end, end),
            (false, "at_or_after_end")
        );
    }

    #[test]
    fn inactive_registration_credential_or_reservation_denies_entry() {
        let start = "2026-09-28T03:00:00.000Z";
        let end = "2026-09-28T04:00:00.000Z";
        let at = "2026-09-28T03:30:00.000Z";

        assert_eq!(
            authorization_decision("revoked", "issued", "active", start, end, at),
            (false, "registration_inactive")
        );
        assert_eq!(
            authorization_decision("registered", "revoked", "active", start, end, at),
            (false, "credential_inactive")
        );
        assert_eq!(
            authorization_decision("registered", "issued", "cancelled", start, end, at),
            (false, "reservation_cancelled")
        );
    }
}
