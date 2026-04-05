// Copyright 2026 NotVkontakte LLC (aka Lain)
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod api;
mod config;
mod services;

use axum::{
    Router,
    routing::{get, post},
};
use chrono::Utc;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::api::handlers::{AppState, handle_get, handle_post};
use crate::api::proxy::{StreamCache, handle_tunnel};
use crate::config::Config;
use crate::services::tiktok::TikTokService;
use crate::services::twitter::TwitterService;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "uran=debug,tower_http=debug,axum=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cfg = Config::load();
    let start_time = Utc::now().timestamp_millis().to_string();

    let stream_cache = Arc::new(StreamCache::new());

    // Spawn cleanup task
    let cache_clone = Arc::clone(&stream_cache);
    let cache_for_cleanup = Arc::clone(&stream_cache);
    tokio::spawn(async move {
        cache_for_cleanup.cleanup_task().await;
    });

    let state = Arc::new(AppState {
        tiktok: Arc::new(TikTokService::new(cfg.user_agent.clone())),
        twitter: Arc::new(TwitterService::new(cfg.user_agent)),
        cache: cache_clone,
        start_time,
    });

    let app = Router::new()
        .route("/", get(handle_get))
        .route("/", post(handle_post))
        .route("/tunnel", get(handle_tunnel))
        .with_state(state)
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", cfg.port)).await.unwrap();

    tracing::info!("starting uran server on port {}", cfg.port);
    axum::serve(listener, app).await.unwrap();
}
