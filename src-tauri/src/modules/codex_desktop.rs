use std::path::PathBuf;
use tracing::{info, warn};

/// Codex CLI/Desktop configuration directory
const CODEX_DIR: &str = ".codex";
const CONFIG_FILE: &str = "config.toml";
const BACKUP_SUFFIX: &str = ".antigravity.bak";
const CATALOG_FILE_TEMPLATE: &str = "{}-models.json";
const DEFAULT_CONTEXT_WINDOW: u64 = 128000;

/// Status of Codex CLI/Desktop installation
#[derive(Debug, serde::Serialize)]
pub struct CodexStatus {
    pub installed: bool,
    pub config_path: Option<String>,
    pub has_backup: bool,
    pub version: Option<String>,
    pub message: String,
}

/// Result of applying Codex Desktop config
#[derive(Debug, serde::Serialize)]
pub struct CodexApplyResult {
    pub success: bool,
    pub catalog_path: Option<String>,
    pub config_path: Option<String>,
    pub message: String,
}

/// Resolve the Codex home directory (~/.codex)
fn resolve_codex_home() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(CODEX_DIR)
}

/// Check if Codex CLI/Desktop is installed by looking for its config directory
pub fn check_codex_status() -> CodexStatus {
    let codex_dir = resolve_codex_home();

    if !codex_dir.exists() {
        return CodexStatus {
            installed: false,
            config_path: None,
            has_backup: false,
            version: None,
            message: format!("Codex directory not found at {:?}", codex_dir),
        };
    }

    let config_path = codex_dir.join(CONFIG_FILE);
    let backup_path = codex_dir.join(format!("{}{}", CONFIG_FILE, BACKUP_SUFFIX));
    let has_backup = backup_path.exists();

    // Try to detect version from config.toml
    let version = if config_path.exists() {
        std::fs::read_to_string(&config_path).ok()
            .and_then(|content| {
                for line in content.lines() {
                    if let Some(val) = line.strip_prefix("version = ") {
                        return Some(val.trim_matches('"').to_string());
                    }
                }
                None
            })
    } else {
        None
    };

    CodexStatus {
        installed: true,
        config_path: Some(config_path.to_string_lossy().to_string()),
        has_backup,
        version,
        message: "Codex CLI/Desktop detected.".to_string(),
    }
}

/// Read the current config.toml content and return it as a string
pub fn read_codex_config() -> Result<String, String> {
    let config_path = resolve_codex_home().join(CONFIG_FILE);
    std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read Codex config: {}", e))
}

// ── Template-based model catalog generation ──
//
// cc-switch approach: define a default template entry with ALL fields that
// Codex Desktop's deserialization expects. Clone this template for each
// model and override only the specific fields (slug, display_name, etc.).
// This ensures NO field is ever missing — matching Codex++ behavior.

/// Return a template model entry with ALL required fields.
/// This is cloned for each model, then fields are overridden.
/// Mirrors Codex++'s `first_bundled_template_entry()` fallback.
fn default_model_template() -> serde_json::Value {
    serde_json::json!({
        "model": "",
        "slug": "",
        "display_name": "",
        "description": "",
        "visibility": "list",
        "supported_in_api": true,
        "context_window": DEFAULT_CONTEXT_WINDOW,
        "max_context_window": DEFAULT_CONTEXT_WINDOW,
        "effective_context_window_percent": 100,
        "auto_compact_token_limit": null,
        "input_modalities": ["text", "image"],
        "supports_image_detail_original": true,
        "supports_parallel_tool_calls": true,
        "supports_search_tool": true,
        "web_search_tool_type": "text_and_image",
        "apply_patch_tool_type": "freeform",
        "shell_type": "shell_command",
        "supports_reasoning_summaries": true,
        "default_reasoning_summary": "auto",
        "default_reasoning_level": "medium",
        "support_verbosity": true,
        "default_verbosity": "low",
        "truncation_policy": {
            "mode": "tokens",
            "limit": 10000
        },
        "priority": 10,
        "supported_reasoning_levels": [
            {"reasoningEffort": "low", "description": "Low effort"},
            {"reasoningEffort": "medium", "description": "Medium effort"},
            {"reasoningEffort": "high", "description": "High effort"}
        ],
        "additional_speed_tiers": [],
        "service_tiers": [],
        "availability_nux": null,
        "upgrade": null
    })
}

/// Generate a model catalog file for Codex Desktop.
///
/// Uses template-cloning approach (like Codex++/cc-switch):
/// 1. Start with a default template entry that has ALL required fields
/// 2. For each model, clone the template and override specific fields
/// 3. This guarantees no field is ever missing
pub fn generate_model_catalog(platform_id: &str) -> Result<String, String> {
    use crate::modules::config;
    use crate::modules::model_manager;

    let app_config = config::load_app_config()?;

    let platform = app_config.platforms.iter()
        .find(|p| p.id == platform_id)
        .ok_or_else(|| format!("Platform not found: {}", platform_id))?;

    let platform_name = &platform.name;
    let path_prefix = &platform.path_prefix;

    let models = model_manager::list_models(platform_id)
        .map_err(|e| format!("Failed to list models: {}", e))?;

    // Build catalog: clone template → override per-model fields
    let template = default_model_template();
    let catalog_models: Vec<serde_json::Value> = models.iter().enumerate().map(|(idx, m)| {
        let context_window = m.max_input_tokens.unwrap_or(DEFAULT_CONTEXT_WINDOW);
        let mut entry = template.clone();

        // Override per-model fields (keep all template fields intact)
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("model".to_string(), serde_json::json!(m.model_name));
            obj.insert("slug".to_string(), serde_json::json!(m.model_name));
            obj.insert("display_name".to_string(), serde_json::json!(format!("{} / {}", platform_name, m.display_name)));
            obj.insert("description".to_string(), serde_json::json!(m.model_name));
            obj.insert("context_window".to_string(), serde_json::json!(context_window));
            obj.insert("max_context_window".to_string(), serde_json::json!(context_window));
            obj.insert("priority".to_string(), serde_json::json!(1000 + idx));
        }

        entry
    }).collect();

    let catalog = serde_json::json!({
        "models": catalog_models
    });

    // Ensure the model-catalogs directory exists
    let catalog_dir = resolve_codex_home().join("model-catalogs");
    std::fs::create_dir_all(&catalog_dir)
        .map_err(|e| format!("Failed to create model-catalogs directory: {}", e))?;

    // Write the catalog file atomically
    let catalog_filename = CATALOG_FILE_TEMPLATE.replace("{}", path_prefix);
    let catalog_path = catalog_dir.join(&catalog_filename);
    let catalog_json = serde_json::to_string_pretty(&catalog)
        .map_err(|e| format!("Failed to serialize model catalog: {}", e))?;

    let temp_path = catalog_dir.join(format!("{}.tmp", catalog_filename));
    std::fs::write(&temp_path, &catalog_json)
        .map_err(|e| format!("Failed to write model catalog: {}", e))?;
    std::fs::rename(&temp_path, &catalog_path)
        .map_err(|e| format!("Failed to finalize model catalog: {}", e))?;

    info!(
        "Model catalog generated: {:?} ({} models for platform '{}')",
        catalog_path,
        models.len(),
        platform_name
    );

    Ok(catalog_path.to_string_lossy().to_string())
}

/// Apply Codex Desktop configuration.
///
/// Uses toml_edit for in-place config.toml editing (cc-switch approach).
/// This preserves ALL existing content, comments, formatting, and sections
/// (other model_providers, MCP servers, skills, plugins, desktop, etc.).
pub fn apply_codex_config(
    platform_id: String,
    model_name: String,
    api_key: Option<String>,
) -> Result<CodexApplyResult, String> {
    use crate::modules::config;

    let app_config = config::load_app_config()?;

    let platform = app_config.platforms.iter()
        .find(|p| p.id == platform_id)
        .ok_or_else(|| format!("Platform not found: {}", platform_id))?;

    let path_prefix = &platform.path_prefix;
    let proxy_port = app_config.proxy_port;
    let proxy_host = app_config.proxy_host.as_str();

    let client_host = if proxy_host == "0.0.0.0" { "127.0.0.1" } else { proxy_host };
    let proxy_base_url = format!("http://{}:{}/{}/v1", client_host, proxy_port, path_prefix);

    // ── Resolve API key ──
    let bearer_token = api_key.unwrap_or_else(|| {
        format!("sk-antigravity-{}", uuid::Uuid::new_v4().to_string().replace('-', ""))
    });

    // ── Generate model catalog ──
    generate_model_catalog(&platform_id)?;

    let codex_dir = resolve_codex_home();
    std::fs::create_dir_all(&codex_dir)
        .map_err(|e| format!("Failed to create .codex directory: {}", e))?;

    // ── Write auth.json ──
    let auth_path = codex_dir.join("auth.json");
    let auth_backup_path = codex_dir.join(format!("auth.json{}", BACKUP_SUFFIX));
    if auth_path.exists() {
        if let Err(e) = std::fs::copy(&auth_path, &auth_backup_path) {
            warn!("Failed to backup auth.json: {}", e);
        } else {
            info!("auth.json backed up: {:?}", auth_backup_path);
        }
    }
    let auth_content = serde_json::json!({ "OPENAI_API_KEY": bearer_token });
    std::fs::write(&auth_path, serde_json::to_string_pretty(&auth_content)
        .map_err(|e| format!("Failed to serialize auth.json: {}", e))?)
        .map_err(|e| format!("Failed to write auth.json: {}", e))?;
    info!("auth.json written: {:?}", auth_path);

    // ── Backup existing config.toml ──
    let config_path = codex_dir.join(CONFIG_FILE);
    let backup_path = codex_dir.join(format!("{}{}", CONFIG_FILE, BACKUP_SUFFIX));
    if config_path.exists() {
        if let Err(e) = std::fs::copy(&config_path, &backup_path) {
            warn!("Failed to create backup: {}", e);
        } else {
            info!("Backup created: {:?}", backup_path);
        }
    }

    // ── Read existing config & edit in-place using toml_edit ──
    // This preserves ALL existing content, comments, formatting.
    // Only our specific keys/sections are inserted/updated.
    let catalog_relative = format!("model-catalogs/{}-models.json", path_prefix);
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut doc = if existing.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        existing.parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("Failed to parse config.toml: {}", e))?
    };

    // ── Set language from app config ──
    let codex_language = match app_config.language.as_str() {
        "zh" | "zh-CN" | "zh-cn" | "zh-Hans" => "zh-cn",
        "zh-TW" | "zh-tw" | "zh-Hant" => "zh-tw",
        "ja" | "ja-JP" => "ja",
        _ => "en",
    };
    doc["language"] = toml_edit::value(codex_language);

    // ── Set top-level keys ──
    doc["model_provider"] = toml_edit::value("custom");
    doc["model"] = toml_edit::value(&model_name);
    doc["model_catalog_json"] = toml_edit::value(&catalog_relative);

    // ── Set [model_providers.custom] section ──
    // Ensure model_providers table exists
    if !doc.contains_key("model_providers") {
        doc["model_providers"] = toml_edit::table();
    }
    // Ensure custom table exists inside model_providers
    let has_custom = doc["model_providers"]
        .as_table()
        .map_or(false, |t| t.contains_key("custom"));
    if !has_custom {
        doc["model_providers"]["custom"] = toml_edit::table();
    }

    doc["model_providers"]["custom"]["name"] = toml_edit::value("custom");
    doc["model_providers"]["custom"]["base_url"] = toml_edit::value(&proxy_base_url);
    doc["model_providers"]["custom"]["wire_api"] = toml_edit::value("responses");
    doc["model_providers"]["custom"]["requires_openai_auth"] = toml_edit::value(false);
    doc["model_providers"]["custom"]["env_key"] = toml_edit::value("OPENAI_API_KEY");

    // Add model list
    if let Ok(models) = crate::modules::model_manager::list_models(&platform_id) {
        if !models.is_empty() {
            let models_array: Vec<toml_edit::Value> = models.iter().map(|m| {
                let mut entry = toml_edit::InlineTable::new();
                entry.insert("model", toml_edit::value(m.model_name.clone()).into_value().unwrap());
                entry.insert("display_name", toml_edit::value(m.display_name.clone()).into_value().unwrap());
                toml_edit::Value::InlineTable(entry)
            }).collect();
            doc["model_providers"]["custom"]["models"] = toml_edit::value(toml_edit::Array::from_iter(models_array));
        }
    }

    // ── Serialize and write ──
    let final_content = format!(
        "# Codex Desktop Configuration\n\
         # Managed by Antigravity Hub\n\
         # Applied at: {}\n\
         # Restore with: restore_codex_config\n\n{}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        doc.to_string()
    );

    std::fs::write(&config_path, &final_content)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    info!(
        "Codex Desktop config applied: model={}, platform={}, base_url={}, path={:?}",
        model_name, platform.name, proxy_base_url, config_path
    );

    Ok(CodexApplyResult {
        success: true,
        catalog_path: Some(catalog_relative.clone()),
        config_path: Some(config_path.to_string_lossy().to_string()),
        message: format!(
            "✅ Codex Desktop 配置已应用！\n\
             提供商: custom (通过 /{}/ 路由)\n\
             默认模型: {}\n\
             代理地址: {}\n\
             配置文件: {:?}\n\
             模型目录: {}\n\
             API Key: {} (已写入 auth.json)",
            path_prefix, model_name, proxy_base_url, config_path, catalog_relative, bearer_token
        ),
    })
}

/// Restore Codex Desktop config from backup
pub fn restore_codex_config() -> Result<String, String> {
    let codex_dir = resolve_codex_home();
    let config_path = codex_dir.join(CONFIG_FILE);
    let backup_path = codex_dir.join(format!("{}{}", CONFIG_FILE, BACKUP_SUFFIX));

    if !backup_path.exists() {
        return Err("No backup found to restore.".to_string());
    }

    std::fs::copy(&backup_path, &config_path)
        .map_err(|e| format!("Failed to restore backup: {}", e))?;
    let _ = std::fs::remove_file(&backup_path);

    // Restore auth.json backup if exists
    let auth_path = codex_dir.join("auth.json");
    let auth_backup_path = codex_dir.join(format!("auth.json{}", BACKUP_SUFFIX));
    if auth_backup_path.exists() {
        std::fs::copy(&auth_backup_path, &auth_path)
            .map_err(|e| format!("Failed to restore auth.json backup: {}", e))?;
        let _ = std::fs::remove_file(&auth_backup_path);
        info!("auth.json restored from backup");
    } else if auth_path.exists() {
        let content = std::fs::read_to_string(&auth_path).unwrap_or_default();
        if content.contains("antigravity") || content.contains("sk-antigravity") {
            let _ = std::fs::remove_file(&auth_path);
            info!("Cleaned up auth.json (written by Antigravity Hub)");
        }
    }

    info!("Codex Desktop config restored from backup: {:?}", config_path);
    Ok(format!(
        "已从备份恢复 Codex Desktop 配置。配置文件: {:?}",
        config_path
    ))
}
