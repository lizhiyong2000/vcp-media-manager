use std::sync::Arc;

use crate::device_store::DeviceStore;
use crate::registry::MediaServerRegistry;

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<MediaServerRegistry>,
    pub devices: Arc<DeviceStore>,
}
