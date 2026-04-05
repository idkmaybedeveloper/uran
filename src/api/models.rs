// Copyright 2026 NotVkontakte LLC (aka Lain)
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub url: String,
    pub download_mode: Option<String>,
    pub _audio_format: Option<String>,
    pub _filename_style: Option<String>,
    pub _video_quality: Option<String>,
    #[serde(default)]
    pub allow_h265: bool,
    #[serde(default)]
    pub tiktok_full_audio: bool,
}

#[derive(Debug, Serialize)]
pub struct ErrorDetail {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub status: String,
    pub error: ErrorDetail,
}


#[derive(Debug, Serialize)]
pub struct TunnelResponse {
    pub status: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PickerItem {
    pub r#type: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickerResponse {
    pub status: String,
    pub picker: Vec<PickerItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_filename: Option<String>,
}

impl ErrorResponse {
    pub fn new(code: &str, context: Option<HashMap<String, serde_json::Value>>) -> Self {
        Self { status: "error".to_string(), error: ErrorDetail { code: code.to_string(), context } }
    }
}

impl TunnelResponse {
    pub fn new(url: String, filename: String) -> Self {
        Self { status: "tunnel".to_string(), url, filename: Some(filename) }
    }
}

impl PickerResponse {
    pub fn new(
        items: Vec<PickerItem>,
        audio: Option<String>,
        audio_filename: Option<String>,
    ) -> Self {
        Self { status: "picker".to_string(), picker: items, audio, audio_filename }
    }
}
