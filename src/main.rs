mod client;
mod config;
mod device_store;
mod model;
mod play_urls;
mod registry;
mod routes;
mod state;

use std::sync::Arc;

use axum::{routing::get, Router};
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::config::Config;
use crate::device_store::DeviceStore;
use crate::registry::MediaServerRegistry;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vcp_media_manager=info,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env()?;
    let registry = Arc::new(MediaServerRegistry::from_config(
        config.servers_config.regions.clone(),
        config.servers_config.servers.clone(),
    )?);
    let devices = Arc::new(DeviceStore::load(&config.devices_file)?);

    let state = AppState {
        registry: registry.clone(),
        devices,
    };

    let app = Router::new()
        .route("/", get(routes::root))
        .nest("/api", routes::api_router())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = config.port;
    let addr = format!("0.0.0.0:{}", port);
    info!("vcp-media-manager listening on http://127.0.0.1:{}", port);
    info!(
        "Regions: {}, media servers: {}",
        registry.regions().len(),
        registry.list_servers().len()
    );
    for server in registry.list_servers() {
        info!("  [{}] {} -> {}", server.region_id, server.id, server.api_url);
    }

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
