use anyhow::{Result, anyhow};
use regex::Regex;
use reqwest::header::{LOCATION, USER_AGENT};
use reqwest::redirect::Policy;
use serde_json::Value;
// use std::sync::Arc;
use tracing::debug;
use url::Url;

pub struct TikTokService {
    client: reqwest::Client,
    user_agent: String,
    post_id_regex: Regex,
}

#[derive(Debug)]
pub struct TikTokResult {
    pub video_url: Option<String>,
    pub video_filename: Option<String>,
    pub audio_url: Option<String>,
    pub audio_filename: Option<String>,
    pub images: Vec<String>,
    pub cookies: Option<String>,
}

impl TikTokService {
    pub fn new(user_agent: String) -> Self {
        let client = reqwest::Client::builder().cookie_store(true).build().unwrap();

        Self { client, user_agent, post_id_regex: Regex::new(r"/(?:video|photo)/(\d+)").unwrap() }
    }

    pub async fn extract(
        &self,
        url: &Url,
        audio_only: bool,
        allow_h265: bool,
        full_audio: bool,
    ) -> Result<TikTokResult> {
        let post_id = self.resolve_post_id(url).await?;
        debug!(post_id = %post_id, "resolved post ID");

        let (data, cookies) = self.fetch_video_data(&post_id).await?;
        self.parse_result(data, &post_id, cookies, audio_only, allow_h265, full_audio)
    }

    async fn resolve_post_id(&self, url: &Url) -> Result<String> {
        if let Some(caps) = self.post_id_regex.captures(url.path()) {
            return Ok(caps[1].to_string());
        }

        let host = url.host_str().unwrap_or("");
        if host.contains("vt.tiktok") || host.contains("vm.tiktok") {
            return self.resolve_short_link(url.as_str()).await;
        }

        let path = url.path().trim_matches('/');
        if let Some(short_code) = path.split('/').last() {
            if !short_code.is_empty() {
                let short_url = format!("https://vt.tiktok.com/{}", short_code);
                return self.resolve_short_link(&short_url).await;
            }
        }

        Err(anyhow!("error.api.link.unsupported"))
    }

    async fn resolve_short_link(&self, url: &str) -> Result<String> {
        let ua = self.user_agent.split(" Chrome/1").next().unwrap_or(&self.user_agent);

        let client = reqwest::Client::builder().redirect(Policy::none()).build()?;

        let resp = client.get(url).header(USER_AGENT, ua).send().await?;

        if let Some(loc) = resp.headers().get(LOCATION) {
            let loc_str = loc.to_str()?;
            if let Some(caps) = self.post_id_regex.captures(loc_str) {
                return Ok(caps[1].to_string());
            }
        }

        let body = resp.text().await?;
        if body.starts_with("<a href=\"https://") {
            if let Some(start) = body.find("href=\"") {
                let start = start + 6;
                if let Some(end) = body[start..].find('"') {
                    let mut extracted_url = &body[start..start + end];
                    if let Some(idx) = extracted_url.find('?') {
                        extracted_url = &extracted_url[..idx];
                    }
                    if let Some(caps) = self.post_id_regex.captures(extracted_url) {
                        return Ok(caps[1].to_string());
                    }
                }
            }
        }

        Err(anyhow!("error.api.fetch.short_link"))
    }

    async fn fetch_video_data(&self, post_id: &str) -> Result<(Value, Option<String>)> {
        let video_url = format!("https://www.tiktok.com/@i/video/{}", post_id);

        let resp = self.client.get(&video_url).header(USER_AGENT, &self.user_agent).send().await?;

        let mut cookies = Vec::new();
        for cookie in resp.cookies() {
            cookies.push(format!("{}={}", cookie.name(), cookie.value()));
        }
        let cookie_str = if cookies.is_empty() { None } else { Some(cookies.join("; ")) };

        let body = resp.text().await?;
        let marker = "<script id=\"__UNIVERSAL_DATA_FOR_REHYDRATION__\" type=\"application/json\">";
        let start =
            body.find(marker).ok_or_else(|| anyhow!("error.api.fetch.fail"))? + marker.len();
        let end = body[start..].find("</script>").ok_or_else(|| anyhow!("error.api.fetch.fail"))?;

        let json_str = &body[start..start + end];
        let data: Value = serde_json::from_str(json_str)?;

        Ok((data, cookie_str))
    }

    fn parse_result(
        &self,
        data: Value,
        post_id: &str,
        cookies: Option<String>,
        audio_only: bool,
        allow_h265: bool,
        full_audio: bool,
    ) -> Result<TikTokResult> {
        let default_scope =
            data.get("__DEFAULT_SCOPE__").ok_or_else(|| anyhow!("error.api.fetch.fail"))?;
        let video_detail = default_scope
            .get("webapp.video-detail")
            .ok_or_else(|| anyhow!("error.api.fetch.fail"))?;

        if let Some(status_msg) = video_detail.get("statusMsg").and_then(|v| v.as_str()) {
            if !status_msg.is_empty() {
                return Err(anyhow!("error.api.content.post.unavailable"));
            }
        }

        let item_struct = video_detail
            .get("itemInfo")
            .and_then(|v| v.get("itemStruct"))
            .ok_or_else(|| anyhow!("error.api.fetch.fail"))?;

        if item_struct.get("isContentClassified").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Err(anyhow!("error.api.content.post.age"));
        }

        let author = item_struct.get("author").ok_or_else(|| anyhow!("error.api.fetch.empty"))?;
        let unique_id = author.get("uniqueId").and_then(|v| v.as_str()).unwrap_or("user");
        let filename_base = format!("tiktok_{}_{}", unique_id, post_id);

        let mut result = TikTokResult {
            video_url: None,
            video_filename: None,
            audio_url: None,
            audio_filename: None,
            images: Vec::new(),
            cookies,
        };

        // check for images
        if let Some(image_post) = item_struct.get("imagePost") {
            if let Some(images) = image_post.get("images").and_then(|v| v.as_array()) {
                for img in images {
                    if let Some(url_list) = img
                        .get("imageURL")
                        .and_then(|v| v.get("urlList"))
                        .and_then(|v| v.as_array())
                    {
                        for u in url_list {
                            if let Some(url_str) = u.as_str() {
                                if url_str.contains(".jpeg?") {
                                    result.images.push(url_str.to_string());
                                    break;
                                }
                            }
                        }
                    }
                }

                if let Some(music) = item_struct.get("music") {
                    if let Some(play_url) = music.get("playUrl").and_then(|v| v.as_str()) {
                        result.audio_url = Some(play_url.to_string());
                        result.audio_filename = Some(format!("{}_audio", filename_base));
                    }
                }

                return Ok(result);
            }
        }

        // video
        let video = item_struct.get("video").ok_or_else(|| anyhow!("error.api.fetch.empty"))?;
        let mut play_addr =
            video.get("playAddr").and_then(|v| v.as_str()).unwrap_or("").to_string();

        if allow_h265 {
            if let Some(bitrate_info) = video.get("bitrateInfo").and_then(|v| v.as_array()) {
                for b in bitrate_info {
                    let codec_type = b.get("CodecType").and_then(|v| v.as_str()).unwrap_or("");
                    if codec_type.contains("h265") {
                        if let Some(h265_url) = b
                            .get("PlayAddr")
                            .and_then(|v| v.get("UrlList"))
                            .and_then(|v| v.as_array())
                            .and_then(|v| v.first())
                            .and_then(|v| v.as_str())
                        {
                            play_addr = h265_url.to_string();
                            break;
                        }
                    }
                }
            }
        }

        if !audio_only {
            result.video_url = Some(play_addr);
            result.video_filename = Some(format!("{}.mp4", filename_base));
        } else {
            let mut audio_url = play_addr;
            let mut audio_filename = format!("{}_audio", filename_base);

            if full_audio {
                if let Some(music) = item_struct.get("music") {
                    if let Some(music_play_url) = music.get("playUrl").and_then(|v| v.as_str()) {
                        if !music_play_url.is_empty() {
                            audio_url = music_play_url.to_string();
                            audio_filename = format!("{}_original", audio_filename);
                        }
                    }
                }
            }

            result.audio_url = Some(audio_url);
            result.audio_filename = Some(audio_filename);
        }

        Ok(result)
    }
}
