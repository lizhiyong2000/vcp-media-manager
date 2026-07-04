use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;

use crate::client::MediaServerClient;
use crate::model::{MediaServerInstance, Region, StreamInfo};

pub struct MediaServerRegistry {
    regions: Vec<Region>,
    servers: HashMap<String, MediaServerInstance>,
    clients: HashMap<String, MediaServerClient>,
}

impl MediaServerRegistry {
    pub fn from_config(regions: Vec<Region>, servers: Vec<MediaServerInstance>) -> Result<Self> {
        let mut server_map = HashMap::new();
        let mut clients = HashMap::new();

        for server in servers {
            let client = MediaServerClient::new(&server.api_url)
                .with_context(|| format!("failed to create client for server {}", server.id))?;
            clients.insert(server.id.clone(), client);
            server_map.insert(server.id.clone(), server);
        }

        Ok(Self {
            regions,
            servers: server_map,
            clients,
        })
    }

    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    pub fn list_servers(&self) -> Vec<&MediaServerInstance> {
        self.servers.values().collect()
    }

    pub fn servers_in_region(&self, region_id: &str) -> Vec<&MediaServerInstance> {
        self.servers
            .values()
            .filter(|s| s.region_id == region_id)
            .collect()
    }

    pub fn get_server(&self, server_id: &str) -> Option<&MediaServerInstance> {
        self.servers.get(server_id)
    }

    pub fn get_client(&self, server_id: &str) -> Option<&MediaServerClient> {
        self.clients.get(server_id)
    }

    pub fn default_server_for_region(&self, region_id: &str) -> Result<&MediaServerInstance> {
        self.servers_in_region(region_id)
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no media server configured for region {region_id}"))
    }

    pub async fn streams_on_server(&self, server_id: &str) -> Result<Vec<Value>> {
        let client = self
            .get_client(server_id)
            .ok_or_else(|| anyhow!("unknown server: {server_id}"))?;
        let (_, value) = client
            .request_json(axum::http::Method::GET, "/api/streams", None)
            .await
            .map_err(|e| anyhow!(e.to_string()))?;

        Ok(value
            .get("streams")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    }

    pub async fn metrics_on_server(&self, server_id: &str) -> Result<Value> {
        let client = self
            .get_client(server_id)
            .ok_or_else(|| anyhow!("unknown server: {server_id}"))?;
        let (_, value) = client
            .request_json(axum::http::Method::GET, "/api/metrics", None)
            .await
            .map_err(|e| anyhow!(e.to_string()))?;
        Ok(value)
    }

    pub async fn health_for_server(&self, server_id: &str) -> Value {
        let Some(server) = self.get_server(server_id) else {
            return serde_json::json!({ "status": "unknown" });
        };
        match self.get_client(server_id) {
            Some(client) => match client.health().await {
                Ok(health) => serde_json::json!({
                    "status": "healthy",
                    "detail": health,
                }),
                Err(err) => serde_json::json!({
                    "status": "unhealthy",
                    "error": err.to_string(),
                }),
            },
            None => serde_json::json!({
                "status": "misconfigured",
                "apiUrl": server.api_url,
            }),
        }
    }

    pub async fn stream_ids_on_server(&self, server_id: &str) -> Result<std::collections::HashSet<String>> {
        let client = self
            .get_client(server_id)
            .ok_or_else(|| anyhow!("unknown server: {server_id}"))?;
        let (_, value) = client
            .request_json(axum::http::Method::GET, "/api/streams", None)
            .await
            .map_err(|e| anyhow!(e.to_string()))?;

        let mut ids = std::collections::HashSet::new();
        if let Some(streams) = value.get("streams").and_then(|v| v.as_array()) {
            for item in streams {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    ids.insert(id.to_string());
                }
            }
        }
        Ok(ids)
    }

    pub async fn fetch_stream_info(
        &self,
        server_id: &str,
        stream_id: &str,
    ) -> Result<Option<StreamInfo>> {
        let client = self
            .get_client(server_id)
            .ok_or_else(|| anyhow!("unknown server: {server_id}"))?;
        let path = format!("/api/stream/{stream_id}");
        match client
            .request_json(axum::http::Method::GET, &path, None)
            .await
        {
            Ok((status, value)) if status.is_success() => Ok(Some(parse_stream_info(&value))),
            Ok((status, _)) if status == axum::http::StatusCode::NOT_FOUND => Ok(None),
            Ok((_, _)) => Ok(None),
            Err(_) => Ok(None),
        }
    }

    pub async fn health_all(&self) -> Vec<Value> {
        let mut results = Vec::new();
        for server in self.list_servers() {
            let entry = match self.get_client(&server.id) {
                Some(client) => match client.health().await {
                    Ok(health) => serde_json::json!({
                        "serverId": server.id,
                        "regionId": server.region_id,
                        "name": server.name,
                        "apiUrl": server.api_url,
                        "status": "healthy",
                        "detail": health,
                    }),
                    Err(err) => serde_json::json!({
                        "serverId": server.id,
                        "regionId": server.region_id,
                        "name": server.name,
                        "apiUrl": server.api_url,
                        "status": "unhealthy",
                        "error": err.to_string(),
                    }),
                },
                None => serde_json::json!({
                    "serverId": server.id,
                    "status": "misconfigured",
                }),
            };
            results.push(entry);
        }
        results
    }
}

fn parse_stream_info(value: &Value) -> StreamInfo {
    StreamInfo {
        status: value.get("status").and_then(|v| v.as_str()).map(str::to_string),
        status_description: value
            .get("status_description")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        playback_status: value
            .get("playback_status")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        playback_description: value
            .get("playback_description")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        protocol: value.get("protocol").and_then(|v| v.as_str()).map(str::to_string),
        tracks: value.get("tracks").and_then(|v| v.as_array()).map(|a| a.len() as u64),
    }
}
