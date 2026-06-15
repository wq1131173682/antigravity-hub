use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use super::platform_manager;

const KEY_MODEL_MAP_FILE: &str = "key_model_map.json";

/// Association between API keys and models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyModelMap {
    /// List of (key_id, model_id) pairs
    #[serde(default)]
    pub mappings: Vec<KeyModelEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyModelEntry {
    pub key_id: String,
    pub model_id: String,
}

impl KeyModelMap {
    pub fn new() -> Self {
        Self { mappings: Vec::new() }
    }
}

fn get_map_file_path() -> Result<PathBuf, String> {
    let data_dir = platform_manager::get_data_dir()?;
    Ok(data_dir.join(KEY_MODEL_MAP_FILE))
}

pub fn load_map() -> Result<KeyModelMap, String> {
    let path = get_map_file_path()?;
    if !path.exists() {
        return Ok(KeyModelMap::new());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("read key_model_map failed: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("parse key_model_map failed: {}", e))
}

fn save_map(map: &KeyModelMap) -> Result<(), String> {
    let path = get_map_file_path()?;
    let content = serde_json::to_string_pretty(map)
        .map_err(|e| format!("serialize key_model_map failed: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("write key_model_map failed: {}", e))
}

/// Get all key IDs associated with a model
pub fn get_keys_for_model(model_id: &str) -> Result<Vec<String>, String> {
    let map = load_map()?;
    Ok(map.mappings.iter()
        .filter(|e| e.model_id == model_id)
        .map(|e| e.key_id.clone())
        .collect())
}

/// Get all model IDs associated with a key
pub fn get_models_for_key(key_id: &str) -> Result<Vec<String>, String> {
    let map = load_map()?;
    Ok(map.mappings.iter()
        .filter(|e| e.key_id == key_id)
        .map(|e| e.model_id.clone())
        .collect())
}

/// Associate a key with a model
pub fn associate_key_with_model(key_id: &str, model_id: &str) -> Result<(), String> {
    let mut map = load_map()?;
    // Avoid duplicates
    if !map.mappings.iter().any(|e| e.key_id == key_id && e.model_id == model_id) {
        map.mappings.push(KeyModelEntry {
            key_id: key_id.to_string(),
            model_id: model_id.to_string(),
        });
    }
    save_map(&map)
}

/// Disassociate a key from a model
pub fn disassociate_key_from_model(key_id: &str, model_id: &str) -> Result<(), String> {
    let mut map = load_map()?;
    map.mappings.retain(|e| !(e.key_id == key_id && e.model_id == model_id));
    save_map(&map)
}

/// Remove all associations for a key
pub fn remove_key_associations(key_id: &str) -> Result<(), String> {
    let mut map = load_map()?;
    map.mappings.retain(|e| e.key_id != key_id);
    save_map(&map)
}

/// Remove all associations for a model
pub fn remove_model_associations(model_id: &str) -> Result<(), String> {
    let mut map = load_map()?;
    map.mappings.retain(|e| e.model_id != model_id);
    save_map(&map)
}

/// Remove all associations for a platform
pub fn remove_platform_associations(platform_id: &str) -> Result<(), String> {
    // We need model IDs for the platform to clean up
    let models = super::model_manager::list_models(platform_id)?;
    let mut map = load_map()?;
    let model_ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
    map.mappings.retain(|e| !model_ids.contains(&e.model_id));
    save_map(&map)
}
