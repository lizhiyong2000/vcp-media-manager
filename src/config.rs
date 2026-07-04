use std::path::Path;

use anyhow::{Context, Result};

use crate::model::{MediaServerInstance, Region, ServersConfig};

#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    pub devices_file: String,
    pub servers_config: ServersConfig,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let port = read_u16("PORT", 8090);
        let devices_file = std::env::var("DEVICES_FILE")
            .unwrap_or_else(|_| "data/devices.json".to_string());
        let servers_config = load_servers_config()?;
        Ok(Self {
            port,
            devices_file,
            servers_config,
        })
    }
}

fn load_servers_config() -> Result<ServersConfig> {
    let config_path = std::env::var("SERVERS_CONFIG").unwrap_or_else(|_| "servers.json".to_string());
    if Path::new(&config_path).exists() {
        let text = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read servers config: {config_path}"))?;
        serde_json::from_str(&text)
            .with_context(|| format!("failed to parse servers config: {config_path}"))
    } else {
        Ok(default_servers_from_env())
    }
}

fn default_servers_from_env() -> ServersConfig {
    ServersConfig {
        regions: vec![Region {
            id: "default".to_string(),
            name: "默认区域".to_string(),
        }],
        servers: vec![MediaServerInstance {
            id: "default-01".to_string(),
            region_id: "default".to_string(),
            name: "本地节点".to_string(),
            api_url: std::env::var("MEDIA_SERVER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8081".to_string()),
            public_host: std::env::var("MEDIA_PUBLIC_HOST")
                .unwrap_or_else(|_| "127.0.0.1".to_string()),
            rtmp_port: 1935,
            rtsp_port: 554,
        }],
    }
}

fn read_u16(name: &str, default: u16) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
