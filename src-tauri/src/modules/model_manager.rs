use std::fs;
use std::path::PathBuf;
use crate::models::Model;

use super::platform_manager;

const MODELS_FILE: &str = "models.json";

/// Simple storage for models
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelStore {
    pub models: Vec<Model>,
}

impl ModelStore {
    pub fn new() -> Self {
        Self { models: Vec::new() }
    }
}

fn get_models_file_path() -> Result<PathBuf, String> {
    let data_dir = platform_manager::get_data_dir()?;
    Ok(data_dir.join(MODELS_FILE))
}

fn load_store() -> Result<ModelStore, String> {
    let path = get_models_file_path()?;
    if !path.exists() {
        return Ok(ModelStore::new());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("read models failed: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("parse models failed: {}", e))
}

fn save_store(store: &ModelStore) -> Result<(), String> {
    let path = get_models_file_path()?;
    let content = serde_json::to_string_pretty(store)
        .map_err(|e| format!("serialize models failed: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("write models failed: {}", e))
}

/// List all models for a platform
pub fn list_models(platform_id: &str) -> Result<Vec<Model>, String> {
    let store = load_store()?;
    let mut models: Vec<Model> = store.models.into_iter()
        .filter(|m| m.platform_id == platform_id)
        .collect();
    models.sort_by_key(|m| m.sort_order);
    Ok(models)
}

/// Add a new model
pub fn add_model(
    platform_id: String,
    model_name: String,
    display_name: String,
    per_5hour: Option<u32>,
    per_day: Option<u32>,
    per_month: Option<u32>,
) -> Result<Model, String> {
    let mut store = load_store()?;
    let mut model = Model::new(platform_id, model_name, display_name);
    if let Some(v) = per_5hour { model.per_5hour = v; }
    if let Some(v) = per_day { model.per_day = v; }
    if let Some(v) = per_month { model.per_month = v; }
    model.sort_order = store.models.len() as i32;
    store.models.push(model.clone());
    save_store(&store)?;
    Ok(model)
}

/// Update a model
pub fn update_model(
    model_id: &str,
    model_name: Option<String>,
    display_name: Option<String>,
    per_5hour: Option<u32>,
    per_day: Option<u32>,
    per_month: Option<u32>,
) -> Result<Model, String> {
    let mut store = load_store()?;
    let model = store.models.iter_mut()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("Model not found: {}", model_id))?;

    if let Some(v) = model_name { model.model_name = v; }
    if let Some(v) = display_name { model.display_name = v; }
    if let Some(v) = per_5hour { model.per_5hour = v; }
    if let Some(v) = per_day { model.per_day = v; }
    if let Some(v) = per_month { model.per_month = v; }

    let result = model.clone();
    save_store(&store)?;
    Ok(result)
}

/// Delete a model
pub fn delete_model(model_id: &str) -> Result<(), String> {
    let mut store = load_store()?;
    let pos = store.models.iter().position(|m| m.id == model_id)
        .ok_or_else(|| format!("Model not found: {}", model_id))?;
    store.models.remove(pos);
    save_store(&store)?;

    // Also clean up key-model associations
    let _ = super::key_model_map::remove_model_associations(model_id);

    Ok(())
}

/// Delete all models for a platform
pub fn delete_platform_models(platform_id: &str) -> Result<(), String> {
    let mut store = load_store()?;
    store.models.retain(|m| m.platform_id != platform_id);
    save_store(&store)
}
