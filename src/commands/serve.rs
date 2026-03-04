use crate::cli::ServeArgs;
use crate::config::Config;
use crate::store::{open_store, SharedStore};
use crate::types::AddressBook;
use anyhow::{Context, Result};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use axum::http::HeaderValue;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

// ── shared state ────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    store: SharedStore,
    api_key: String,
}

// ── authentication ───────────────────────────────────────────────────────────

fn check_api_key(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == expected)
        .unwrap_or(false)
}

macro_rules! require_auth {
    ($headers:expr, $state:expr) => {
        if !check_api_key(&$headers, &$state.api_key) {
            return (StatusCode::UNAUTHORIZED, Json(ApiError::unauthorized())).into_response();
        }
    };
}

// ── error type ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ApiError {
    error: String,
}

impl ApiError {
    fn unauthorized() -> Self {
        Self { error: "missing or invalid X-Api-Key header".to_string() }
    }
    fn not_found(msg: &str) -> Self {
        Self { error: msg.to_string() }
    }
    fn internal(msg: &str) -> Self {
        Self { error: msg.to_string() }
    }
}

// ── response types ───────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListResponse<T> {
    total: usize,
    items: Vec<T>,
}

// ── query params ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListQuery {
    /// Max records to return (default 50, max 200).
    limit: Option<usize>,
    /// Skip N records (for pagination).
    offset: Option<usize>,
    /// Filter by source chain ID.
    src_chain: Option<String>,
    /// Filter by destination chain ID.
    dst_chain: Option<String>,
}

// ── handlers ─────────────────────────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn list_relays(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    require_auth!(headers, state);

    let limit = query.limit.unwrap_or(50).min(200);
    let offset = query.offset.unwrap_or(0);

    let store = state.store.read().await;
    let all: Vec<_> = match (&query.src_chain, &query.dst_chain) {
        (Some(src), Some(dst)) => store.by_chain_pair(src, dst),
        _ => store.all(),
    };

    let total = all.len();
    let items: Vec<_> = all.into_iter().skip(offset).take(limit).cloned().collect();

    Json(ListResponse { total, items }).into_response()
}

async fn get_relay_by_bundle_hash(
    State(state): State<AppState>,
    Path(bundle_hash): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    require_auth!(headers, state);

    let store = state.store.read().await;
    match store.by_bundle_hash(&bundle_hash) {
        Some(record) => Json(record.clone()).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError::not_found(&format!("bundle {} not found", bundle_hash))),
        )
            .into_response(),
    }
}

async fn get_relay_by_tx_hash(
    State(state): State<AppState>,
    Path(tx_hash): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    require_auth!(headers, state);

    let store = state.store.read().await;
    match store.by_tx_hash(&tx_hash) {
        Some(record) => Json(record.clone()).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError::not_found(&format!("tx {} not found", tx_hash))),
        )
            .into_response(),
    }
}

async fn clear_relays(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    require_auth!(headers, state);

    let mut store = state.store.write().await;
    match store.clear() {
        Ok(()) => Json(serde_json::json!({ "cleared": true })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal(&e.to_string())),
        )
            .into_response(),
    }
}

// ── router ───────────────────────────────────────────────────────────────────

fn build_router(state: AppState, cors_origin: Option<&str>) -> Router {
    let cors = if cors_origin.map(|o| o == "*").unwrap_or(true) {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_headers(Any)
            .allow_methods(Any)
    } else {
        let origin: HeaderValue = cors_origin.unwrap().parse().expect("invalid CORS origin");
        CorsLayer::new()
            .allow_origin(origin)
            .allow_headers(Any)
            .allow_methods(Any)
    };

    Router::new()
        .route("/health", get(health))
        .route("/api/relays", get(list_relays).delete(clear_relays))
        .route("/api/relays/by-bundle/:bundle_hash", get(get_relay_by_bundle_hash))
        .route("/api/relays/by-tx/:tx_hash", get(get_relay_by_tx_hash))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// ── entry point ───────────────────────────────────────────────────────────────

pub async fn run(args: ServeArgs, _config: Config, _addresses: AddressBook) -> Result<()> {
    // Validate CORS origin early so we get a clean error, not a panic inside build_router.
    if let Some(origin) = &args.cors_origin {
        if origin != "*" {
            origin
                .parse::<HeaderValue>()
                .with_context(|| format!("invalid CORS origin: {origin}"))?;
        }
    }

    let store_path = args.store_path.unwrap_or_else(crate::store::default_store_path);

    if args.clear {
        if store_path.exists() {
            std::fs::remove_file(&store_path)
                .with_context(|| format!("failed to clear store at {}", store_path.display()))?;
            println!("store cleared: {}", store_path.display());
        }
    }

    let store = open_store(store_path.clone())
        .with_context(|| format!("failed to open relay store at {}", store_path.display()))?;

    // Resolve API key: clap already reads CAST_INTEROP_API_KEY via #[arg(env)],
    // so args.api_key is Some if the flag or env var was provided; otherwise auto-generate.
    let api_key = args.api_key.unwrap_or_else(|| {
        let key = uuid::Uuid::new_v4().to_string().replace('-', "");
        println!("generated API key: {key}");
        key
    });

    let state = AppState { store, api_key: api_key.clone() };
    let router = build_router(state, args.cors_origin.as_deref());

    let addr: std::net::SocketAddr = args
        .bind
        .parse()
        .with_context(|| format!("invalid bind address: {}", args.bind))?;

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind to {addr}"))?;

    info!("relay API server listening on http://{addr}");
    println!("relay API server listening on http://{addr}");
    println!("store: {}", store_path.display());
    println!("set X-Api-Key: {api_key}");

    axum::serve(listener, router)
        .await
        .context("server error")?;

    Ok(())
}
