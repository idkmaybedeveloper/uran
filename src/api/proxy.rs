// Copyright 2026 NotVkontakte LLC (aka Lain)
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use axum::{
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use dashmap::DashMap;
use hmac::{Hmac, Mac, KeyInit};
use serde::Deserialize;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time;
use tracing::{debug, error};

type HmacSha256 = Hmac<Sha256>;

pub const STREAM_LIFESPAN: i64 = 30 * 60; // seconds
const SECRET_KEY: &[u8] = b"uran-stream-secret"; // FIXME: move to config???

use crate::api::handlers::AppState;

#[derive(Clone)]
pub struct StreamData {
    pub url: String,
    pub filename: String,
    pub service: String,
    pub headers: Option<HashMap<String, String>>,
    pub expires_at: i64,
}

pub struct StreamCache {
    pub cache: DashMap<String, StreamData>,
}

impl StreamCache {
    pub fn new() -> Self {
        let cache = DashMap::new();
        Self { cache }
    }

    pub async fn cleanup_task(self: Arc<Self>) {
        let mut interval = time::interval(time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            let now = Utc::now().timestamp();
            self.cache.retain(|_, data| data.expires_at > now);
        }
    }
}

#[derive(Deserialize)]
pub struct TunnelParams {
    pub id: String,
    pub exp: i64,
    pub sig: String,
}

pub fn sign_stream(id: &str, exp: i64) -> String {
    let mut mac = HmacSha256::new_from_slice(SECRET_KEY).unwrap();
    mac.update(format!("{}:{}", id, exp).as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

pub fn verify_signature(id: &str, exp: i64, sig: &str) -> bool {
    let expected = sign_stream(id, exp);
    expected == sig
}

pub fn generate_stream_id() -> String {
    let id = uuid::Uuid::new_v4();
    URL_SAFE_NO_PAD.encode(id.as_bytes())
}

pub async fn handle_tunnel(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TunnelParams>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    if !verify_signature(&params.id, params.exp, &params.sig) {
        debug!(id = %params.id, "invalid signature");
        return Err(StatusCode::UNAUTHORIZED);
    }

    if Utc::now().timestamp() > params.exp {
        debug!(id = %params.id, "stream expired");
        return Err(StatusCode::GONE);
    }

    let data = state.cache.cache.get(&params.id).map(|v| v.clone()).ok_or(StatusCode::NOT_FOUND)?;

    let client = reqwest::Client::new();
    let mut req = client.get(&data.url)
        .header(header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");

    // Service specific headers
    match data.service.as_str() {
        "tiktok" => {
            req = req.header(header::REFERER, "https://www.tiktok.com/");
        }
        "twitter" => {
            req = req.header(header::REFERER, "https://twitter.com/");
        }
        _ => {}
    }

    if let Some(h) = data.headers {
        for (k, v) in h {
            req = req.header(k, v);
        }
    }

    if let Some(range) = headers.get(header::RANGE) {
        req = req.header(header::RANGE, range);
    }

    let resp = req.send().await.map_err(|e| {
        error!(error = ?e, "proxy request failed");
        StatusCode::BAD_GATEWAY
    })?;

    let mut response_builder = Response::builder().status(resp.status());

    if let Some(ct) = resp.headers().get(header::CONTENT_TYPE) {
        response_builder = response_builder.header(header::CONTENT_TYPE, ct);
    }
    if let Some(cl) = resp.headers().get(header::CONTENT_LENGTH) {
        response_builder = response_builder.header(header::CONTENT_LENGTH, cl);
    }
    if let Some(ar) = resp.headers().get(header::ACCEPT_RANGES) {
        response_builder = response_builder.header(header::ACCEPT_RANGES, ar);
    }
    if let Some(cr) = resp.headers().get(header::CONTENT_RANGE) {
        response_builder = response_builder.header(header::CONTENT_RANGE, cr);
    }

    let escaped_filename = data.filename.replace('"', "\\\"");
    response_builder = response_builder.header(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", escaped_filename),
    );

    let body = Body::from_stream(resp.bytes_stream());
    Ok(response_builder.body(body).unwrap())
}

pub fn get_base_url(headers: &HeaderMap, proto: &str, host: &str) -> String {
    let proto = headers.get("X-Forwarded-Proto").and_then(|v| v.to_str().ok()).unwrap_or(proto);

    let host = headers.get("X-Forwarded-Host").and_then(|v| v.to_str().ok()).unwrap_or(host);

    format!("{}://{}", proto, host)
}
