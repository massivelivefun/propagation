use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use std::collections::HashMap;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ConnectionConfig {
    pub input: String,
    pub output: String,
    pub muted: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum EndpointType {
    Playback,  // Output to the system (users hear this)
    Recording, // Input from the system (users speak into this)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VirtualEndpointConfig {
    pub name: String,
    pub channels: u16,
    pub kind: EndpointType,
}

#[derive(Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub connections: Vec<ConnectionConfig>,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
    #[serde(default)]
    pub latency_ms: Option<f32>,
    // Host ID serialization is tricky as it's an enum or opaque. 
    // We'll store it as a String and try to parse it.
    #[serde(default)]
    pub host_name: Option<String>,
    #[serde(default)]
    pub virtual_endpoints: Vec<VirtualEndpointConfig>,
}

impl AppConfig {
    pub fn load() -> Self {
        if Path::new("propagation_config.json").exists() {
            if let Ok(file) = File::open("propagation_config.json") {
                let reader = BufReader::new(file);
                if let Ok(config) = serde_json::from_reader(reader) {
                    return config;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Ok(file) = File::create("propagation_config.json") {
            let _ = serde_json::to_writer_pretty(file, self);
        }
    }
}
