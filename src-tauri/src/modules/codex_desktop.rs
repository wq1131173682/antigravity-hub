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
                // Look for key = value or "key" = "value" patterns
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

/// Generate a model catalog file for Codex Desktop for a specific platform.
///
/// The model catalog tells Codex Desktop which models are available.
/// It is written to `~/.codex/model-catalogs/{path_prefix}-models.json`.
pub fn generate_model_catalog(platform_id: &str) -> Result<String, String> {
    use crate::modules::config;
    use crate::modules::model_manager;

    let app_config = config::load_app_config()?;

    let platform = app_config.platforms.iter()
        .find(|p| p.id == platform_id)
        .ok_or_else(|| format!("Platform not found: {}", platform_id))?;

    let platform_name = &platform.name;
    let path_prefix = &platform.path_prefix;

    // List all models for this platform
    let models = model_manager::list_models(platform_id)
        .map_err(|e| format!("Failed to list models: {}", e))?;

    // Build catalog entries in Codex Desktop format
    let catalog_models: Vec<serde_json::Value> = models.iter().map(|m| {
        let context_window = m.max_input_tokens.unwrap_or(DEFAULT_CONTEXT_WINDOW);
        // Auto-compact at half the context window, capped at 196608
        let auto_compact = std::cmp::min(context_window / 2, 196608_u64);

        serde_json::json!({
            "model": m.model_name,
            "slug": m.model_name,
            "display_name": format!("{} / {}", platform_name, m.display_name),
            "description": m.model_name,
            "visibility": "list",
            "supported_in_api": true,
            "context_window": context_window,
            "max_context_window": context_window,
            "effective_context_window_percent": 95,
            "auto_compact_token_limit": auto_compact,
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
            "priority": 10
        })
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
/// Generates a model catalog and writes `config.toml` pointing to our proxy.
/// The config.toml uses the Desktop-compatible format with `model_catalog_json`.
pub fn apply_codex_config(
    platform_id: String,
    model_name: String,
) -> Result<CodexApplyResult, String> {
    use crate::modules::config;

    let app_config = config::load_app_config()?;

    let platform = app_config.platforms.iter()
        .find(|p| p.id == platform_id)
        .ok_or_else(|| format!("Platform not found: {}", platform_id))?;

    let path_prefix = &platform.path_prefix;
    let proxy_port = app_config.proxy_port;
    let proxy_host = app_config.proxy_host.as_str();

    // Determine the base URL for Codex Desktop to call
    let client_host = if proxy_host == "0.0.0.0" { "127.0.0.1" } else { proxy_host };
    let proxy_base_url = format!("http://{}:{}/{}/v1", client_host, proxy_port, path_prefix);

    // ── Generate model catalog ──
    let catalog_path = generate_model_catalog(&platform_id)?;

    // ── Build config.toml content ──
    let mut config_toml = toml::Table::new();

    // Top-level keys (Desktop-compatible)
    config_toml.insert(
        "model_provider".to_string(),
        toml::Value::String("custom".to_string()),
    );
    config_toml.insert(
        "model".to_string(),
        toml::Value::String(model_name.clone()),
    );
    config_toml.insert(
        "model_catalog_json".to_string(),
        toml::Value::String(catalog_path.clone()),
    );

    // [model_providers.custom] section
    let mut provider_table = toml::Table::new();
    provider_table.insert(
        "name".to_string(),
        toml::Value::String("custom".to_string()),
    );
    provider_table.insert(
        "base_url".to_string(),
        toml::Value::String(proxy_base_url.clone()),
    );
    provider_table.insert(
        "wire_api".to_string(),
        toml::Value::String("responses".to_string()),
    );
    // Desktop reads API keys from env vars, not config
    provider_table.insert(
        "env_key".to_string(),
        toml::Value::String("OPENAI_API_KEY".to_string()),
    );

    // Add model list so Desktop UI dropdown shows available models
    if let Ok(models) = crate::modules::model_manager::list_models(&platform_id) {
        let models_array: Vec<toml::Value> = models.iter().map(|m| {
            let mut entry = toml::Table::new();
            entry.insert(
                "model".to_string(),
                toml::Value::String(m.model_name.clone()),
            );
            entry.insert(
                "display_name".to_string(),
                toml::Value::String(m.display_name.clone()),
            );
            toml::Value::Table(entry)
        }).collect();
        if !models_array.is_empty() {
            provider_table.insert(
                "models".to_string(),
                toml::Value::Array(models_array),
            );
        }
    }

    config_toml.insert(
        "model_providers".to_string(),
        toml::Value::Table({
            let mut providers = toml::Table::new();
            providers.insert("custom".to_string(), toml::Value::Table(provider_table));
            providers
        }),
    );

    let output = toml::to_string_pretty(&config_toml)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    // Add header comment
    let final_content = format!(
        "# Codex Desktop Configuration\n\
         # Managed by Antigravity Hub\n\
         # Applied at: {}\n\n{}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        output
    );

    // Backup existing config
    let codex_dir = resolve_codex_home();
    let config_path = codex_dir.join(CONFIG_FILE);
    let backup_path = codex_dir.join(format!("{}{}", CONFIG_FILE, BACKUP_SUFFIX));

    if config_path.exists() {
        if let Err(e) = std::fs::copy(&config_path, &backup_path) {
            warn!("Failed to create backup: {}", e);
        } else {
            info!("Backup created: {:?}", backup_path);
        }
    }

    // Write the new config
    std::fs::write(&config_path, &final_content)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    info!(
        "Codex Desktop config applied: model={}, platform={}, base_url={}, path={:?}",
        model_name, platform.name, proxy_base_url, config_path
    );

    Ok(CodexApplyResult {
        success: true,
        catalog_path: Some(catalog_path.clone()),
        config_path: Some(config_path.to_string_lossy().to_string()),
        message: format!(
            "✅ Codex Desktop 配置已应用！\n\
             提供商: custom (通过 /{}/ 路由)\n\
             默认模型: {}\n\
             代理地址: {}\n\
             配置文件: {:?}\n\
             模型目录: {:?}",
            path_prefix, model_name, proxy_base_url, config_path, catalog_path
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

    // Remove backup after successful restore
    let _ = std::fs::remove_file(&backup_path);

    info!("Codex Desktop config restored from backup: {:?}", config_path);

    Ok(format!(
        "已从备份恢复 Codex Desktop 配置。配置文件: {:?}",
        config_path
    ))
}
