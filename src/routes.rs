use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;

use crate::{
    device_store::now_rfc3339,
    model::{Device, DeviceWithStream},
    play_urls,
    state::AppState,
};

pub async fn root() -> &'static str {
    "vcp-media-manager"
}

pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/regions", get(list_regions))
        .route("/servers", get(list_servers))
        .route("/servers/:id", get(get_server))
        .route("/devices", get(list_devices).post(create_device))
        .route(
            "/devices/:id",
            get(get_device)
                .put(update_device)
                .delete(delete_device),
        )
        .route("/devices/:id/play-urls", get(get_device_play_urls))
        .route("/streams", get(list_streams).post(create_stream))
        .route("/streams/:id", get(get_stream).delete(delete_stream))
        .route("/metrics", get(get_metrics))
        .route("/pull/rtmp", axum::routing::post(pull_rtmp))
        .route("/pull/rtsp", axum::routing::post(pull_rtsp))
        .route("/play-urls/:id", get(get_play_urls_legacy))
        .route("/transcode/start", axum::routing::post(start_transcode))
        .route("/transcode/stop", axum::routing::post(stop_transcode))
        .route("/transcode", get(list_transcode_sessions))
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let servers = state.registry.health_all().await;
    let all_healthy = servers.iter().all(|s| s.get("status") == Some(&json!("healthy")));
    Json(json!({
        "status": if all_healthy { "healthy" } else { "degraded" },
        "adminServer": true,
        "regionCount": state.registry.regions().len(),
        "serverCount": state.registry.list_servers().len(),
        "servers": servers,
    }))
}

async fn list_regions(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "regions": state.registry.regions(),
        "count": state.registry.regions().len(),
    }))
}

async fn list_servers(State(state): State<AppState>) -> Json<serde_json::Value> {
    let devices = state.devices.list().await;
    let health_entries = state.registry.health_all().await;
    let health_by_id: std::collections::HashMap<String, &serde_json::Value> = health_entries
        .iter()
        .filter_map(|h| {
            h.get("serverId")
                .and_then(|v| v.as_str())
                .map(|id| (id.to_string(), h))
        })
        .collect();

    let mut items = Vec::new();
    for server in state.registry.list_servers() {
        let stream_count = state
            .registry
            .stream_ids_on_server(&server.id)
            .await
            .map(|ids| ids.len())
            .unwrap_or(0);
        let device_count = devices.iter().filter(|d| d.server_id == server.id).count();
        let health = health_by_id.get(&server.id);
        let status = health
            .and_then(|h| h.get("status"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        items.push(json!({
            "id": server.id,
            "regionId": server.region_id,
            "name": server.name,
            "apiUrl": server.api_url,
            "publicHost": server.public_host,
            "rtmpPort": server.rtmp_port,
            "rtspPort": server.rtsp_port,
            "webrtcPort": server.webrtc_port,
            "httpPort": server.http_port(),
            "status": status,
            "streamCount": stream_count,
            "deviceCount": device_count,
        }));
    }
    items.sort_by(|a, b| {
        a.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("id").and_then(|v| v.as_str()).unwrap_or(""))
    });
    Json(json!({
        "servers": items,
        "count": items.len(),
    }))
}

async fn get_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(server) = state.registry.get_server(&id) else {
        return not_found(format!("media server not found: {id}"));
    };

    let health = state.registry.health_for_server(&id).await;
    let status = health
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let streams = state
        .registry
        .streams_on_server(&id)
        .await
        .unwrap_or_default();
    let metrics = state
        .registry
        .metrics_on_server(&id)
        .await
        .unwrap_or_else(|e| json!({ "error": e.to_string() }));
    let devices: Vec<_> = state
        .devices
        .list()
        .await
        .into_iter()
        .filter(|d| d.server_id == id)
        .collect();
    let region_name = state
        .registry
        .regions()
        .iter()
        .find(|r| r.id == server.region_id)
        .map(|r| r.name.as_str())
        .unwrap_or(&server.region_id);

    (
        StatusCode::OK,
        Json(json!({
            "id": server.id,
            "regionId": server.region_id,
            "regionName": region_name,
            "name": server.name,
            "apiUrl": server.api_url,
            "publicHost": server.public_host,
            "rtmpPort": server.rtmp_port,
            "rtspPort": server.rtsp_port,
            "webrtcPort": server.webrtc_port,
            "httpPort": server.http_port(),
            "status": status,
            "health": health,
            "streamCount": streams.len(),
            "deviceCount": devices.len(),
            "streams": streams,
            "devices": devices,
            "metrics": metrics,
        })),
    )
}

async fn list_devices(State(state): State<AppState>) -> Json<serde_json::Value> {
    let devices = state.devices.list().await;
    let enriched = enrich_devices(&state, devices).await;
    Json(json!({
        "devices": enriched,
        "count": enriched.len(),
    }))
}

async fn get_device(State(state): State<AppState>, Path(id): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    let Some(device) = state.devices.get(&id).await else {
        return not_found(format!("device not found: {id}"));
    };
    let enriched = enrich_one(&state, device).await;
    (StatusCode::OK, Json(serde_json::to_value(enriched).unwrap_or_default()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDeviceRequest {
    id: String,
    name: String,
    region_id: String,
    server_id: Option<String>,
    description: Option<String>,
}

async fn create_device(
    State(state): State<AppState>,
    Json(body): Json<CreateDeviceRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let id = body.id.trim().to_string();
    if id.is_empty() {
        return bad_request("device id is required");
    }
    if state.registry.regions().iter().all(|r| r.id != body.region_id) {
        return bad_request(format!("unknown region: {}", body.region_id));
    }

    let server = match &body.server_id {
        Some(server_id) => {
            let Some(server) = state.registry.get_server(server_id) else {
                return bad_request(format!("unknown server: {server_id}"));
            };
            server
        }
        None => {
            let Ok(server) = state.registry.default_server_for_region(&body.region_id) else {
                return bad_request(format!("no media server configured for region {}", body.region_id));
            };
            server
        }
    };
    if server.region_id != body.region_id {
        return bad_request(format!(
            "server {} does not belong to region {}",
            server.id, body.region_id
        ));
    }

    let now = now_rfc3339();
    let device = Device {
        id: id.clone(),
        name: body.name.trim().to_string(),
        region_id: body.region_id,
        server_id: server.id.clone(),
        description: body
            .description
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty()),
        created_at: now.clone(),
        updated_at: now,
    };

    if let Err(e) = state.devices.create(device.clone()).await {
        return bad_request(e.to_string());
    }

    let enriched = enrich_one(&state, device).await;
    (
        StatusCode::CREATED,
        Json(serde_json::to_value(enriched).unwrap_or_default()),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateDeviceRequest {
    name: String,
    region_id: String,
    server_id: Option<String>,
    description: Option<String>,
}

async fn update_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateDeviceRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(existing) = state.devices.get(&id).await else {
        return not_found(format!("device not found: {id}"));
    };

    if state.registry.regions().iter().all(|r| r.id != body.region_id) {
        return bad_request(format!("unknown region: {}", body.region_id));
    }

    let server = match &body.server_id {
        Some(server_id) => {
            let Some(server) = state.registry.get_server(server_id) else {
                return bad_request(format!("unknown server: {server_id}"));
            };
            server
        }
        None => {
            let Ok(server) = state.registry.default_server_for_region(&body.region_id) else {
                return bad_request(format!("no media server configured for region {}", body.region_id));
            };
            server
        }
    };
    if server.region_id != body.region_id {
        return bad_request(format!(
            "server {} does not belong to region {}",
            server.id, body.region_id
        ));
    }

    let device = Device {
        id: id.clone(),
        name: body.name.trim().to_string(),
        region_id: body.region_id,
        server_id: server.id.clone(),
        description: body
            .description
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty()),
        created_at: existing.created_at,
        updated_at: now_rfc3339(),
    };

    let Ok(updated) = state.devices.update(&id, device).await else {
        return not_found(format!("device not found: {id}"));
    };
    let enriched = enrich_one(&state, updated).await;
    (StatusCode::OK, Json(serde_json::to_value(enriched).unwrap_or_default()))
}

async fn delete_device(State(state): State<AppState>, Path(id): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = state.devices.delete(&id).await {
        return not_found(e.to_string());
    }
    (StatusCode::OK, Json(json!({ "deleted": id })))
}

async fn get_device_play_urls(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(device) = state.devices.get(&id).await else {
        return not_found(format!("device not found: {id}"));
    };
    let Some(server) = state.registry.get_server(&device.server_id) else {
        return bad_request(format!("unknown server: {}", device.server_id));
    };
    let play_urls = play_urls::build_play_urls(server, &device.id);
    let stream = state
        .registry
        .fetch_stream_info(&device.server_id, &device.id)
        .await
        .unwrap_or(None);
    (
        StatusCode::OK,
        Json(json!({
            "deviceId": device.id,
            "streamOnline": stream.is_some(),
            "playUrls": play_urls,
        })),
    )
}

async fn enrich_devices(state: &AppState, devices: Vec<Device>) -> Vec<DeviceWithStream> {
    let mut by_server: std::collections::HashMap<String, Vec<Device>> =
        std::collections::HashMap::new();
    for device in devices {
        by_server
            .entry(device.server_id.clone())
            .or_default()
            .push(device);
    }

    let mut online_ids: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for server_id in by_server.keys() {
        if let Ok(ids) = state.registry.stream_ids_on_server(server_id).await {
            online_ids.insert(server_id.clone(), ids);
        }
    }

    let mut result = Vec::new();
    for device in by_server.into_values().flatten() {
        let stream_online = online_ids
            .get(&device.server_id)
            .map(|ids| ids.contains(&device.id))
            .unwrap_or(false);
        let stream = if stream_online {
            state
                .registry
                .fetch_stream_info(&device.server_id, &device.id)
                .await
                .ok()
                .flatten()
        } else {
            None
        };
        result.push(DeviceWithStream {
            device,
            stream_online,
            stream,
        });
    }
    result.sort_by(|a, b| a.device.id.cmp(&b.device.id));
    result
}

async fn enrich_one(state: &AppState, device: Device) -> DeviceWithStream {
    let stream = state
        .registry
        .fetch_stream_info(&device.server_id, &device.id)
        .await
        .ok()
        .flatten();
    DeviceWithStream {
        stream_online: stream.is_some(),
        stream,
        device,
    }
}

// --- legacy stream proxy (single-server style, routes to device's server when possible) ---

async fn list_streams(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let mut all_streams = Vec::new();
    for server in state.registry.list_servers() {
        let Some(client) = state.registry.get_client(&server.id) else {
            continue;
        };
        match client
            .request_json(axum::http::Method::GET, "/api/streams", None)
            .await
        {
            Ok((status, mut value)) if status.is_success() => {
                if let Some(streams) = value.get_mut("streams").and_then(|v| v.as_array_mut()) {
                    for item in streams.iter_mut() {
                        if let Some(obj) = item.as_object_mut() {
                            obj.insert("serverId".to_string(), json!(server.id));
                            obj.insert("regionId".to_string(), json!(server.region_id));
                        }
                    }
                    all_streams.extend(streams.clone());
                }
            }
            Err(e) => {
                return (e.status, Json(json!({ "error": e.body })));
            }
            _ => {}
        }
    }
    (StatusCode::OK, Json(json!({ "streams": all_streams, "count": all_streams.len() })))
}

async fn get_stream(State(state): State<AppState>, Path(id): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(device) = state.devices.get(&id).await {
        let path = format!("/api/stream/{}", device.id);
        if let Some(client) = state.registry.get_client(&device.server_id) {
            match client.request_json(axum::http::Method::GET, &path, None).await {
                Ok((status, mut value)) if status.is_success() => {
                    if let Some(server) = state.registry.get_server(&device.server_id) {
                        value["playUrls"] =
                            serde_json::to_value(play_urls::build_play_urls(server, &device.id))
                                .unwrap_or_default();
                        value["deviceId"] = json!(device.id);
                        value["serverId"] = json!(device.server_id);
                        value["regionId"] = json!(device.region_id);
                    }
                    return (status, Json(value));
                }
                Ok((status, value)) => return (status, Json(value)),
                Err(e) => return (e.status, Json(json!({ "error": e.body }))),
            }
        }
    }

    for server in state.registry.list_servers() {
        let Some(client) = state.registry.get_client(&server.id) else {
            continue;
        };
        let path = format!("/api/stream/{id}");
        if let Ok((status, mut value)) = client.request_json(axum::http::Method::GET, &path, None).await {
            if status.is_success() {
                value["playUrls"] =
                    serde_json::to_value(play_urls::build_play_urls(server, &id)).unwrap_or_default();
                value["serverId"] = json!(server.id);
                value["regionId"] = json!(server.region_id);
                return (status, Json(value));
            }
        }
    }
    (StatusCode::NOT_FOUND, Json(json!({ "error": "stream not found" })))
}

async fn create_stream(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let stream_id = body
        .get("stream_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if let Some(device) = state.devices.get(&stream_id).await {
        if let Some(client) = state.registry.get_client(&device.server_id) {
            return match client
                .request_json(axum::http::Method::POST, "/api/streams", Some(body))
                .await
            {
                Ok((status, value)) => (status, Json(value)),
                Err(e) => (e.status, Json(json!({ "error": e.body }))),
            };
        }
    }
    proxy_first_server(&state, axum::http::Method::POST, "/api/streams", Some(body)).await
}

async fn delete_stream(State(state): State<AppState>, Path(id): Path<String>) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(device) = state.devices.get(&id).await {
        if let Some(client) = state.registry.get_client(&device.server_id) {
            let path = format!("/api/stream/{id}");
            return match client
                .request_json(axum::http::Method::DELETE, &path, None)
                .await
            {
                Ok((status, value)) => (status, Json(value)),
                Err(e) => (e.status, Json(json!({ "error": e.body }))),
            };
        }
    }
    for server in state.registry.list_servers() {
        let Some(client) = state.registry.get_client(&server.id) else {
            continue;
        };
        let path = format!("/api/stream/{id}");
        if let Ok((status, value)) = client.request_json(axum::http::Method::DELETE, &path, None).await {
            if status.is_success() {
                return (status, Json(value));
            }
        }
    }
    (StatusCode::NOT_FOUND, Json(json!({ "error": "stream not found" })))
}

async fn get_metrics(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let mut merged = json!({ "servers": {} });
    for server in state.registry.list_servers() {
        let Some(client) = state.registry.get_client(&server.id) else {
            continue;
        };
        match client
            .request_json(axum::http::Method::GET, "/api/metrics", None)
            .await
        {
            Ok((status, value)) if status.is_success() => {
                merged["servers"][&server.id] = value;
            }
            Err(e) => {
                return (e.status, Json(json!({ "error": e.body })));
            }
            _ => {}
        }
    }
    (StatusCode::OK, Json(merged))
}

async fn pull_rtmp(State(state): State<AppState>, Json(body): Json<serde_json::Value>) -> (StatusCode, Json<serde_json::Value>) {
    route_pull(&state, body, "/api/rtmp/pull").await
}

async fn pull_rtsp(State(state): State<AppState>, Json(body): Json<serde_json::Value>) -> (StatusCode, Json<serde_json::Value>) {
    route_pull(&state, body, "/api/rtsp/pull").await
}

async fn route_pull(
    state: &AppState,
    body: serde_json::Value,
    path: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    let stream_id = body
        .get("stream_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if let Some(device) = state.devices.get(&stream_id).await {
        if let Some(client) = state.registry.get_client(&device.server_id) {
            return match client
                .request_json(axum::http::Method::POST, path, Some(body))
                .await
            {
                Ok((status, value)) => (status, Json(value)),
                Err(e) => (e.status, Json(json!({ "error": e.body }))),
            };
        }
    }
    proxy_first_server(state, axum::http::Method::POST, path, Some(body)).await
}

async fn get_play_urls_legacy(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(device) = state.devices.get(&id).await {
        if let Some(server) = state.registry.get_server(&device.server_id) {
            return (
                StatusCode::OK,
                Json(json!({
                    "streamId": id,
                    "deviceId": device.id,
                    "playUrls": play_urls::build_play_urls(server, &device.id),
                })),
            );
        }
    }
    for server in state.registry.list_servers() {
        return (
            StatusCode::OK,
            Json(json!({
                "streamId": id,
                "playUrls": play_urls::build_play_urls(server, &id),
            })),
        );
    }
    (StatusCode::NOT_FOUND, Json(json!({ "error": "no media server configured" })))
}

async fn proxy_first_server(
    state: &AppState,
    method: axum::http::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(server) = state.registry.list_servers().first().copied() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "no media server configured" })),
        );
    };
    let Some(client) = state.registry.get_client(&server.id) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "media server client unavailable" })),
        );
    };
    match client.request_json(method, path, body).await {
        Ok((status, value)) => (status, Json(value)),
        Err(e) => (e.status, Json(json!({ "error": e.body }))),
    }
}

fn bad_request(message: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message.to_string() })),
    )
}

fn not_found(message: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": message.to_string() })),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartTranscodeRequest {
    source_stream_id: String,
    target_stream_id: String,
    profile: Option<serde_json::Value>,
}

async fn start_transcode(
    State(state): State<AppState>,
    Json(body): Json<StartTranscodeRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let stream_id = body.source_stream_id.clone();
    if let Some(device) = state.devices.get(&stream_id).await {
        if let Some(client) = state.registry.get_client(&device.server_id) {
            let profile = body.profile
                .map(|p| serde_json::from_value(p).unwrap_or_default())
                .unwrap_or_default();
            let req = crate::client::StartTranscodeRequest {
                source_stream_id: body.source_stream_id,
                target_stream_id: body.target_stream_id,
                profile: Some(profile),
            };
            return match client.start_transcode(req).await {
                Ok(res) => (StatusCode::OK, Json(serde_json::to_value(res).unwrap_or_default())),
                Err(e) => (e.status, Json(json!({ "error": e.body }))),
            };
        }
    }
    proxy_first_server_transcode_start(&state, body).await
}

async fn proxy_first_server_transcode_start(
    state: &AppState,
    body: StartTranscodeRequest,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(server) = state.registry.list_servers().first().copied() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "no media server configured" })),
        );
    };
    let Some(client) = state.registry.get_client(&server.id) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "media server client unavailable" })),
        );
    };
    let profile = body.profile
        .map(|p| serde_json::from_value(p).unwrap_or_default())
        .unwrap_or_default();
    let req = crate::client::StartTranscodeRequest {
        source_stream_id: body.source_stream_id,
        target_stream_id: body.target_stream_id,
        profile: Some(profile),
    };
    match client.start_transcode(req).await {
        Ok(res) => (StatusCode::OK, Json(serde_json::to_value(res).unwrap_or_default())),
        Err(e) => (e.status, Json(json!({ "error": e.body }))),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StopTranscodeRequest {
    source_stream_id: String,
    target_stream_id: Option<String>,
}

async fn stop_transcode(
    State(state): State<AppState>,
    Json(body): Json<StopTranscodeRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let stream_id = body.source_stream_id.clone();
    if let Some(device) = state.devices.get(&stream_id).await {
        if let Some(client) = state.registry.get_client(&device.server_id) {
            let req = crate::client::StopTranscodeRequest {
                source_stream_id: body.source_stream_id,
                target_stream_id: body.target_stream_id,
            };
            return match client.stop_transcode(req).await {
                Ok(res) => (StatusCode::OK, Json(serde_json::to_value(res).unwrap_or_default())),
                Err(e) => (e.status, Json(json!({ "error": e.body }))),
            };
        }
    }
    proxy_first_server_transcode_stop(&state, body).await
}

async fn proxy_first_server_transcode_stop(
    state: &AppState,
    body: StopTranscodeRequest,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(server) = state.registry.list_servers().first().copied() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "no media server configured" })),
        );
    };
    let Some(client) = state.registry.get_client(&server.id) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "media server client unavailable" })),
        );
    };
    let req = crate::client::StopTranscodeRequest {
        source_stream_id: body.source_stream_id,
        target_stream_id: body.target_stream_id,
    };
    match client.stop_transcode(req).await {
        Ok(res) => (StatusCode::OK, Json(serde_json::to_value(res).unwrap_or_default())),
        Err(e) => (e.status, Json(json!({ "error": e.body }))),
    }
}

async fn list_transcode_sessions(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let mut all_sessions = Vec::new();
    for server in state.registry.list_servers() {
        let Some(client) = state.registry.get_client(&server.id) else {
            continue;
        };
        match client.list_transcode_sessions().await {
            Ok(sessions) => {
                for mut session in sessions {
                    let mut obj = serde_json::to_value(session).unwrap_or_default();
                    if let Some(o) = obj.as_object_mut() {
                        o.insert("serverId".to_string(), json!(server.id));
                        o.insert("regionId".to_string(), json!(server.region_id));
                    }
                    all_sessions.push(obj);
                }
            }
            Err(e) => {
                return (e.status, Json(json!({ "error": e.body })));
            }
        }
    }
    (StatusCode::OK, Json(json!({ "sessions": all_sessions, "count": all_sessions.len() })))
}
