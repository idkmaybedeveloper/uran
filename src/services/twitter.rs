// Copyright 2026 NotVkontakte LLC (aka Lain)
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{Result, anyhow};
use regex::Regex;
use reqwest::header::{AUTHORIZATION, COOKIE, USER_AGENT};
use serde_json::Value;
use std::f64;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;
use url::Url;

const GRAPHQL_URL: &str = "https://api.x.com/graphql/4Siu98E55GquhG52zHdY5w/TweetDetail";
const TOKEN_URL: &str = "https://api.x.com/1.1/guest/activate.json";
const BEARER_TOKEN: &str = "Bearer AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA"; // idk stolen from cobalt

const TWEET_FEATURES: &str = r#"{"rweb_video_screen_enabled":false,"payments_enabled":false,"rweb_xchat_enabled":false,"profile_label_improvements_pcf_label_in_post_enabled":true,"rweb_tipjar_consumption_enabled":true,"verified_phone_label_enabled":false,"creator_subscriptions_tweet_preview_api_enabled":true,"responsive_web_graphql_timeline_navigation_enabled":true,"responsive_web_graphql_skip_user_profile_image_extensions_enabled":false,"premium_content_api_read_enabled":false,"communities_web_enable_tweet_community_results_fetch":true,"c9s_tweet_anatomy_moderator_badge_enabled":true,"responsive_web_grok_analyze_button_fetch_trends_enabled":false,"responsive_web_grok_analyze_post_followups_enabled":true,"responsive_web_jetfuel_frame":true,"responsive_web_grok_share_attachment_enabled":true,"articles_preview_enabled":true,"responsive_web_edit_tweet_api_enabled":true,"graphql_is_translatable_rweb_tweet_is_translatable_enabled":true,"view_counts_everywhere_api_enabled":true,"longform_notetweets_consumption_enabled":true,"responsive_web_twitter_article_tweet_consumption_enabled":true,"tweet_awards_web_tipping_enabled":false,"responsive_web_grok_show_grok_translated_post":false,"responsive_web_grok_analysis_button_from_backend":true,"creator_subscriptions_quote_tweet_preview_enabled":false,"freedom_of_speech_not_reach_fetch_enabled":true,"standardized_nudges_misinfo":true,"tweet_with_visibility_results_prefer_gql_limited_actions_policy_enabled":true,"longform_notetweets_rich_text_read_enabled":true,"longform_notetweets_inline_media_enabled":true,"responsive_web_grok_image_annotation_enabled":true,"responsive_web_grok_imagine_annotation_enabled":true,"responsive_web_grok_community_note_auto_translation_is_enabled":false,"responsive_web_enhance_cards_enabled":false}"#;
const TWEET_FIELD_TOGGLES: &str = r#"{"withArticleRichContentState":true,"withArticlePlainText":false,"withGrokAnalyze":false,"withDisallowedReplyControls":false}"#;

pub struct TwitterService {
    client: reqwest::Client,
    user_agent: String,
    cached_token: Arc<RwLock<Option<String>>>,
    tweet_id_regex: Regex,
}

#[derive(Debug)]
pub struct MediaItem {
    pub r#type: String,
    pub url: String,
    pub _thumbnail_url: String,
    pub _is_gif: bool,
}

#[derive(Debug)]
pub struct TwitterResult {
    pub media: Vec<MediaItem>,
    pub filename: String,
}

impl TwitterService {
    pub fn new(user_agent: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            user_agent,
            cached_token: Arc::new(RwLock::new(None)),
            tweet_id_regex: Regex::new(r"/status/(\d+)").unwrap(),
        }
    }

    pub async fn extract(&self, url: &Url, index: i32) -> Result<TwitterResult> {
        let tweet_id = self.extract_tweet_id(url)?;
        debug!(tweet_id = %tweet_id, "extracted tweet ID");

        let guest_token = self.get_guest_token(false).await?;

        let mut media = match self.fetch_from_graphql(&tweet_id, &guest_token).await {
            Ok(m) => m,
            Err(e) => {
                debug!(error = ?e, "graphql failed, trying syndication");
                self.fetch_from_syndication(&tweet_id).await?
            }
        };

        if media.is_empty() {
            return Err(anyhow!("error.api.fetch.empty"));
        }

        if index >= 0 && (index as usize) < media.len() {
            media = vec![media.remove(index as usize)];
        }

        Ok(TwitterResult { media, filename: format!("twitter_{}", tweet_id) })
    }

    fn extract_tweet_id(&self, url: &Url) -> Result<String> {
        self.tweet_id_regex
            .captures(url.path())
            .map(|caps| caps[1].to_string())
            .ok_or_else(|| anyhow!("error.api.link.unsupported"))
    }

    async fn get_guest_token(&self, force_refresh: bool) -> Result<String> {
        if !force_refresh {
            if let Some(token) = &*self.cached_token.read().await {
                return Ok(token.clone());
            }
        }

        let mut lock = self.cached_token.write().await;
        if !force_refresh {
            if let Some(token) = &*lock {
                return Ok(token.clone());
            }
        }

        let resp = self
            .client
            .post(TOKEN_URL)
            .header(USER_AGENT, &self.user_agent)
            .header(AUTHORIZATION, BEARER_TOKEN)
            .header("x-twitter-client-language", "en")
            .header("x-twitter-active-user", "yes")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("error.api.fetch.fail"));
        }

        let data: Value = resp.json().await?;
        let token = data
            .get("guest_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("error.api.fetch.fail"))?
            .to_string();

        *lock = Some(token.clone());
        debug!(token = %token, "got guest token");
        Ok(token)
    }

    async fn fetch_from_graphql(
        &self,
        tweet_id: &str,
        guest_token: &str,
    ) -> Result<Vec<MediaItem>> {
        let variables = serde_json::json!({
            "focalTweetId": tweet_id,
            "with_rux_injections": false,
            "rankingMode": "Relevance",
            "includePromotedContent": true,
            "withCommunity": true,
            "withQuickPromoteEligibilityTweetFields": true,
            "withBirdwatchNotes": true,
            "withVoice": true,
        });

        let url = Url::parse_with_params(
            GRAPHQL_URL,
            &[
                ("variables", variables.to_string()),
                ("features", TWEET_FEATURES.to_string()),
                ("fieldToggles", TWEET_FIELD_TOGGLES.to_string()),
            ],
        )?;

        let resp = self
            .client
            .get(url)
            .header(USER_AGENT, &self.user_agent)
            .header(AUTHORIZATION, BEARER_TOKEN)
            .header("x-guest-token", guest_token)
            .header("x-twitter-client-language", "en")
            .header("x-twitter-active-user", "yes")
            .header(COOKIE, format!("guest_id=v1%3A{}", guest_token))
            .send()
            .await?;

        if resp.status().as_u16() == 403 || resp.status().as_u16() == 429 {
            let new_token = self.get_guest_token(true).await?;
            return Box::pin(self.fetch_from_graphql(tweet_id, &new_token)).await;
        }

        if !resp.status().is_success() {
            return Err(anyhow!("error.api.fetch.fail: status {}", resp.status()));
        }

        let data: Value = resp.json().await?;
        self.parse_graphql_response(data, tweet_id)
    }

    fn parse_graphql_response(&self, data: Value, tweet_id: &str) -> Result<Vec<MediaItem>> {
        let instructions = data
            .pointer("/data/threaded_conversation_with_injections_v2/instructions")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("error.api.fetch.fail"))?;

        let mut tweet_result = None;
        for insn in instructions {
            if insn.get("type").and_then(|v| v.as_str()) == Some("TimelineAddEntries") {
                if let Some(entries) = insn.get("entries").and_then(|v| v.as_array()) {
                    for entry in entries {
                        if entry.get("entryId").and_then(|v| v.as_str())
                            == Some(&format!("tweet-{}", tweet_id))
                        {
                            tweet_result = entry
                                .pointer("/content/itemContent/tweet_results/result")
                                .and_then(|v| v.as_object())
                                .cloned();
                            break;
                        }
                    }
                }
            }
        }

        let tweet_result = tweet_result.ok_or_else(|| anyhow!("error.api.fetch.empty"))?;
        let typename = tweet_result.get("__typename").and_then(|v| v.as_str()).unwrap_or("");

        if typename == "TweetUnavailable" || typename == "TweetTombstone" {
            return Err(anyhow!("error.api.content.post.unavailable"));
        }

        let legacy = if typename == "TweetWithVisibilityResults" {
            tweet_result.get("tweet").and_then(|v| v.get("legacy"))
        } else {
            tweet_result.get("legacy")
        }
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("error.api.fetch.empty"))?;

        let mut extended_entities =
            legacy.get("extended_entities").and_then(|v| v.get("media")).and_then(|v| v.as_array());
        if extended_entities.is_none() {
            extended_entities = data.pointer("/data/threaded_conversation_with_injections_v2/instructions")
                .and_then(|v| v.as_array())
                .and_then(|instructions| {
                    for insn in instructions {
                        if insn.get("type").and_then(|v| v.as_str()) == Some("TimelineAddEntries") {
                            if let Some(entries) = insn.get("entries").and_then(|v| v.as_array()) {
                                for entry in entries {
                                    if entry.get("entryId").and_then(|v| v.as_str()) == Some(&format!("tweet-{}", tweet_id)) {
                                        return entry.pointer("/content/itemContent/tweet_results/result/legacy/retweeted_status_result/result/legacy/extended_entities/media")
                                            .or_else(|| entry.pointer("/content/itemContent/tweet_results/result/tweet/legacy/retweeted_status_result/result/legacy/extended_entities/media"))
                                            .and_then(|v| v.as_array());
                                    }
                                }
                            }
                        }
                    }
                    None
                });
        }

        let extended_entities =
            extended_entities.ok_or_else(|| anyhow!("error.api.fetch.empty"))?;
        self.parse_media_entities(extended_entities)
    }

    async fn fetch_from_syndication(&self, tweet_id: &str) -> Result<Vec<MediaItem>> {
        let id_num: f64 = tweet_id.parse()?;
        let mut token = format!("{:.}", (id_num / 1e15) * f64::consts::PI);
        token = token.replace(['0', '.'], "");

        let url = format!(
            "https://cdn.syndication.twimg.com/tweet-result?id={}&token={}",
            tweet_id, token
        );

        let resp = self.client.get(url).header(USER_AGENT, &self.user_agent).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("error.api.fetch.fail"));
        }

        let data: Value = resp.json().await?;
        let media_details = data
            .get("mediaDetails")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("error.api.fetch.empty"))?;

        self.parse_media_entities(media_details)
    }

    fn parse_media_entities(&self, media: &[Value]) -> Result<Vec<MediaItem>> {
        let mut items = Vec::new();

        for m in media {
            let media_type = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match media_type {
                "photo" => {
                    if let Some(media_url) = m.get("media_url_https").and_then(|v| v.as_str()) {
                        items.push(MediaItem {
                            r#type: "photo".to_string(),
                            url: format!("{}?name=4096x4096", media_url),
                            _thumbnail_url: media_url.to_string(),
                            _is_gif: false,
                        });
                    }
                }
                "video" | "animated_gif" => {
                    if let Some(variants) =
                        m.pointer("/video_info/variants").and_then(|v| v.as_array())
                    {
                        let mut best_url = None;
                        let mut best_bitrate = -1.0;

                        for v in variants {
                            if v.get("content_type").and_then(|v| v.as_str()) == Some("video/mp4") {
                                let bitrate =
                                    v.get("bitrate").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                let url = v.get("url").and_then(|v| v.as_str());
                                if let Some(u) = url {
                                    if bitrate >= best_bitrate {
                                        best_bitrate = bitrate;
                                        best_url = Some(u.to_string());
                                    }
                                }
                            }
                        }

                        if let Some(url) = best_url {
                            let thumbnail = m
                                .get("media_url_https")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            items.push(MediaItem {
                                r#type: "video".to_string(),
                                url,
                                _thumbnail_url: thumbnail,
                                _is_gif: media_type == "animated_gif",
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(items)
    }
}
