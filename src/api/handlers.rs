// Copyright 2026 NotVkontakte LLC (aka Lain)
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use chrono::Utc;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::error;
use url::Url;

use crate::api::models::{ErrorResponse, PickerItem, PickerResponse, Request, TunnelResponse};
use crate::api::proxy::{StreamCache, StreamData, generate_stream_id, get_base_url, sign_stream};
// use crate::config::Config;
use crate::services::tiktok::TikTokService;
use crate::services::twitter::TwitterService;

#[derive(Clone)]
pub struct AppState {
    pub tiktok: Arc<TikTokService>,
    pub twitter: Arc<TwitterService>,
    pub cache: Arc<StreamCache>,
    pub start_time: String,
}

pub async fn handle_get(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let base_url = get_base_url(&headers, "http", "localhost:8080");
    let version = "11.5";
    let start_time = state.start_time.clone();

    Json(json!({
        "cobalt": {
            "version": version,
            "url": format!("{}/", base_url),
            "startTime": start_time,
            "services": ["tiktok", "twitter"]
        },
        "git": {
            "branch": env!("GIT_BRANCH"),
            "commit": env!("GIT_COMMIT"),
            "remote": env!("GIT_REMOTE")
        }
    }))
}

pub async fn handle_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<Request>,
) -> impl IntoResponse {
    if req.url.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(ErrorResponse::new("error.api.link.missing", None)))
            .into_response();
    }

    let parsed_url = match Url::parse(&req.url) {
        Ok(u) => u,
        Err(e) => {
            error!(error = ?e, "failed to parse URL");
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("error.api.link.invalid", None)),
            )
                .into_response();
        }
    };

    let host = parsed_url.host_str().unwrap_or("").to_lowercase();
    let base_url = get_base_url(&headers, "http", "localhost:8080");

    if host.contains("tiktok.com") || host.contains("tiktok") {
        match state
            .tiktok
            .extract(
                &parsed_url,
                req.download_mode.as_deref() == Some("audio"),
                req.allow_h265,
                req.tiktok_full_audio,
            )
            .await
        {
            Ok(result) => {
                let mut headers_map = HashMap::new();
                if let Some(cookies) = result.cookies {
                    headers_map.insert("Cookie".to_string(), cookies);
                }

                if !result.images.is_empty() {
                    let mut items = Vec::new();
                    for (i, img) in result.images.into_iter().enumerate() {
                        let filename = format!(
                            "{}_{}.jpg",
                            result.video_filename.as_deref().unwrap_or("image"),
                            i + 1
                        );
                        let tunnel_url = create_tunnel_url(
                            &state.cache,
                            &base_url,
                            "tiktok",
                            img,
                            filename,
                            Some(headers_map.clone()),
                        );
                        items.push(PickerItem {
                            r#type: "photo".to_string(),
                            url: tunnel_url,
                            thumb: None,
                        });
                    }

                    let audio_tunnel = result.audio_url.map(|url| {
                        create_tunnel_url(
                            &state.cache,
                            &base_url,
                            "tiktok",
                            url,
                            format!("{}.mp3", result.audio_filename.as_deref().unwrap_or("audio")),
                            Some(headers_map),
                        )
                    });

                    return Json(PickerResponse::new(items, audio_tunnel, result.audio_filename))
                        .into_response();
                }

                if let Some(video_url) = result.video_url {
                    let tunnel_url = create_tunnel_url(
                        &state.cache,
                        &base_url,
                        "tiktok",
                        video_url,
                        result.video_filename.unwrap_or_else(|| "video.mp4".to_string()),
                        Some(headers_map),
                    );
                    return Json(TunnelResponse::new(tunnel_url, "video.mp4".to_string()))
                        .into_response();
                }

                if let Some(audio_url) = result.audio_url {
                    let tunnel_url = create_tunnel_url(
                        &state.cache,
                        &base_url,
                        "tiktok",
                        audio_url,
                        format!("{}.mp3", result.audio_filename.as_deref().unwrap_or("audio")),
                        Some(headers_map),
                    );
                    return Json(TunnelResponse::new(tunnel_url, "audio.mp3".to_string()))
                        .into_response();
                }

                (StatusCode::BAD_REQUEST, Json(ErrorResponse::new("error.api.fetch.empty", None)))
                    .into_response()
            }
            Err(e) => {
                error!(error = ?e, "tiktok extraction failed");
                let mut context = HashMap::new();
                context.insert("service".to_string(), json!("tiktok"));
                (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(&e.to_string(), Some(context))))
                    .into_response()
            }
        }
    } else if host.contains("twitter.com") || host.contains("x.com") {
        // Simple index extraction from URL path if present
        let mut index = -1;
        let re = regex::Regex::new(r"/video/(\d+)").unwrap();
        if let Some(caps) = re.captures(parsed_url.path()) {
            if let Ok(i) = caps[1].parse::<i32>() {
                index = i - 1;
            }
        }

        match state.twitter.extract(&parsed_url, index).await {
            Ok(result) => {
                if result.media.len() == 1 {
                    let item = &result.media[0];
                    let mut filename = result.filename.clone();
                    if item.r#type == "photo" {
                        filename += ".jpg";
                    } else {
                        filename += ".mp4";
                    }
                    let tunnel_url = create_tunnel_url(
                        &state.cache,
                        &base_url,
                        "twitter",
                        item.url.clone(),
                        filename.clone(),
                        None,
                    );
                    return Json(TunnelResponse::new(tunnel_url, filename)).into_response();
                }

                let mut items = Vec::new();
                for (i, media) in result.media.into_iter().enumerate() {
                    let mut filename = result.filename.clone();
                    if media.r#type == "photo" {
                        filename = format!("{}_{}.jpg", filename, i + 1);
                    } else {
                        filename = format!("{}_{}.mp4", filename, i + 1);
                    }
                    let tunnel_url = create_tunnel_url(
                        &state.cache,
                        &base_url,
                        "twitter",
                        media.url,
                        filename,
                        None,
                    );
                    items.push(PickerItem { r#type: media.r#type, url: tunnel_url, thumb: None });
                }

                Json(PickerResponse::new(items, None, None)).into_response()
            }
            Err(e) => {
                error!(error = ?e, "twitter extraction failed");
                let mut context = HashMap::new();
                context.insert("service".to_string(), json!("twitter"));
                (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(&e.to_string(), Some(context))))
                    .into_response()
            }
        }
    } else {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse::new("error.api.service.unsupported", None)))
            .into_response()
    }
}

fn create_tunnel_url(
    cache: &StreamCache,
    base_url: &str,
    service: &str,
    url: String,
    filename: String,
    headers: Option<HashMap<String, String>>,
) -> String {
    let id = generate_stream_id();
    let exp = Utc::now().timestamp() + crate::api::proxy::STREAM_LIFESPAN;

    cache.cache.insert(
        id.clone(),
        StreamData { url, filename, service: service.to_string(), headers, expires_at: exp },
    );

    let sig = sign_stream(&id, exp);
    format!("{}/tunnel?id={}&exp={}&sig={}", base_url, id, exp, sig)
}
