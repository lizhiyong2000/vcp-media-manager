use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaServerInstance {
    pub id: String,
    pub region_id: String,
    pub name: String,
    pub api_url: String,
    pub public_host: String,
    pub rtmp_port: u16,
    pub rtsp_port: u16,
    #[serde(default = "default_webrtc_port")]
    pub webrtc_port: u16,
}

fn default_webrtc_port() -> u16 {
    9080
}

impl MediaServerInstance {
    /// HTTP / HLS / FLV / WebRTC 测试页与 api_url 同端口。
    pub fn http_port(&self) -> u16 {
        parse_port_from_url(&self.api_url, 8081)
    }
}

fn parse_port_from_url(raw: &str, default: u16) -> u16 {
    let after_scheme = raw.split("//").nth(1).unwrap_or(raw);
    let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
    if let Some((_host, port)) = host_port.rsplit_once(':') {
        if let Ok(p) = port.parse::<u16>() {
            return p;
        }
    }
    default
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub region_id: String,
    pub server_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServersConfig {
    pub regions: Vec<Region>,
    pub servers: Vec<MediaServerInstance>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayUrls {
    pub rtmp: String,
    pub rtsp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_flv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hls: Option<String>,
    pub webrtc_test_page: String,
    pub webrtc_signaling_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfo {
    pub status: Option<String>,
    pub status_description: Option<String>,
    pub playback_status: Option<String>,
    pub playback_description: Option<String>,
    pub protocol: Option<String>,
    pub tracks: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceWithStream {
    #[serde(flatten)]
    pub device: Device,
    pub stream_online: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<StreamInfo>,
}
