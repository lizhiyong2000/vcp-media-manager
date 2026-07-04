use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use tokio::sync::RwLock;

use crate::model::Device;

#[derive(Default)]
struct StoreData {
    devices: Vec<Device>,
}

pub struct DeviceStore {
    path: String,
    inner: RwLock<StoreData>,
}

impl DeviceStore {
    pub fn load(path: &str) -> Result<Self> {
        let data = if Path::new(path).exists() {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read devices file: {path}"))?;
            if text.trim().is_empty() {
                StoreData::default()
            } else {
                let devices: Vec<Device> = serde_json::from_str(&text)
                    .with_context(|| format!("failed to parse devices file: {path}"))?;
                StoreData { devices }
            }
        } else {
            if let Some(parent) = Path::new(path).parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory for {path}"))?;
            }
            StoreData::default()
        };

        Ok(Self {
            path: path.to_string(),
            inner: RwLock::new(data),
        })
    }

    pub async fn list(&self) -> Vec<Device> {
        self.inner.read().await.devices.clone()
    }

    pub async fn get(&self, id: &str) -> Option<Device> {
        self.inner
            .read()
            .await
            .devices
            .iter()
            .find(|d| d.id == id)
            .cloned()
    }

    pub async fn create(&self, device: Device) -> Result<Device> {
        let mut guard = self.inner.write().await;
        if guard.devices.iter().any(|d| d.id == device.id) {
            return Err(anyhow!("device already exists: {}", device.id));
        }
        guard.devices.push(device.clone());
        Self::persist(&self.path, &guard.devices)?;
        Ok(device)
    }

    pub async fn update(&self, id: &str, mut device: Device) -> Result<Device> {
        let mut guard = self.inner.write().await;
        let Some(index) = guard.devices.iter().position(|d| d.id == id) else {
            return Err(anyhow!("device not found: {id}"));
        };
        device.id = id.to_string();
        device.created_at = guard.devices[index].created_at.clone();
        device.updated_at = Utc::now().to_rfc3339();
        guard.devices[index] = device.clone();
        Self::persist(&self.path, &guard.devices)?;
        Ok(device)
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let mut guard = self.inner.write().await;
        let len_before = guard.devices.len();
        guard.devices.retain(|d| d.id != id);
        if guard.devices.len() == len_before {
            return Err(anyhow!("device not found: {id}"));
        }
        Self::persist(&self.path, &guard.devices)?;
        Ok(())
    }

    fn persist(path: &str, devices: &[Device]) -> Result<()> {
        let text = serde_json::to_string_pretty(devices)?;
        std::fs::write(path, text).with_context(|| format!("failed to write devices file: {path}"))
    }
}

pub type SharedDeviceStore = Arc<DeviceStore>;

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}
