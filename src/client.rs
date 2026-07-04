use anyhow::{Context, Result};
use axum::http::{Method, StatusCode};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone)]
pub struct MediaServerClient {
    base_url: String,
    http: Client,
}

#[derive(Debug)]
pub struct UpstreamError {
    pub status: StatusCode,
    pub body: String,
}

impl std::fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "upstream HTTP {}: {}", self.status, self.body)
    }
}

impl std::error::Error for UpstreamError {}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayUrls {
    pub rtmp: String,
    pub rtsp: String,
    pub http_flv: Option<String>,
    pub hls: Option<String>,
    pub webrtc_test_page: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayUrlsResponse {
    pub stream_id: String,
    pub play_urls: PlayUrls,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranscodeProfile {
    pub video_codec: Option<String>,
    pub video_bitrate: Option<String>,
    pub resolution: Option<String>,
    pub fps: Option<u32>,
    pub audio_codec: Option<String>,
    pub audio_bitrate: Option<String>,
    pub audio_sample_rate: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct StartTranscodeRequest {
    pub source_stream_id: String,
    pub target_stream_id: String,
    pub profile: Option<TranscodeProfile>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct StopTranscodeRequest {
    pub source_stream_id: String,
    pub target_stream_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct TranscodeSessionInfo {
    pub source_stream_id: String,
    pub target_stream_id: String,
    pub profile: TranscodeProfile,
    pub status: String,
    pub started_at: u64,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct TranscodeResponse {
    pub transcode: TranscodeSessionInfo,
    pub message: String,
}

impl MediaServerClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { base_url, http })
    }

    pub async fn request_json(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<(StatusCode, Value), UpstreamError> {
        let url = format!("{}{}", self.base_url, normalize_path(path));
        let mut req = self.http.request(method, &url).header("Accept", "application/json");
        if let Some(body) = body {
            req = req.json(&body);
        }

        let response = req.send().await.map_err(|err| UpstreamError {
            status: StatusCode::BAD_GATEWAY,
            body: format!("cannot reach media server at {}: {err}", self.base_url),
        })?;

        let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(UpstreamError { status, body: text });
        }

        let value = if text.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        Ok((status, value))
    }

    pub async fn health(&self) -> Result<Value, UpstreamError> {
        self.request_json(Method::GET, "/health", None)
            .await
            .map(|(_, v)| v)
    }

    pub async fn play_urls(&self, stream_id: &str) -> Result<PlayUrlsResponse, UpstreamError> {
        let path = format!("/api/play-urls/{}", stream_id);
        let (_, value) = self.request_json(Method::GET, &path, None).await?;
        serde_json::from_value(value)
            .map_err(|e| UpstreamError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: format!("failed to parse play urls response: {e}"),
            })
    }

    pub async fn start_transcode(&self, req: StartTranscodeRequest) -> Result<TranscodeResponse, UpstreamError> {
        let body = serde_json::to_value(req)
            .map_err(|e| UpstreamError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: format!("failed to serialize request: {e}"),
            })?;
        let (_, value) = self.request_json(Method::POST, "/api/transcode/start", Some(body)).await?;
        serde_json::from_value(value)
            .map_err(|e| UpstreamError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: format!("failed to parse transcode response: {e}"),
            })
    }

    pub async fn stop_transcode(&self, req: StopTranscodeRequest) -> Result<TranscodeResponse, UpstreamError> {
        let body = serde_json::to_value(req)
            .map_err(|e| UpstreamError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: format!("failed to serialize request: {e}"),
            })?;
        let (_, value) = self.request_json(Method::POST, "/api/transcode/stop", Some(body)).await?;
        serde_json::from_value(value)
            .map_err(|e| UpstreamError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: format!("failed to parse transcode response: {e}"),
            })
    }

    pub async fn list_transcode_sessions(&self) -> Result<Vec<TranscodeSessionInfo>, UpstreamError> {
        let (_, value) = self.request_json(Method::GET, "/api/transcode", None).await?;
        let sessions = value.get("sessions")
            .ok_or_else(|| UpstreamError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: "missing sessions field in response".to_string(),
            })?;
        serde_json::from_value(sessions.clone())
            .map_err(|e| UpstreamError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: format!("failed to parse sessions: {e}"),
            })
    }
}

fn normalize_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}
