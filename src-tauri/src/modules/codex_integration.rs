use std::path::PathBuf;
use tracing::{info, warn};
use uuid::Uuid;

/// Codex CLI configuration directory and file constants
const CODEX_DIR: &str = ".codex";
const CONFIG_FILE: &str = "config.toml";
const BACKUP_SUFFIX: &str = ".antigravity.bak";

/// Model catalog file name template
const CATALOG_FILE: &str = "{}-models.json";
/// Default context window for models that don't specify one
const DEFAULT_CONTEXT_WINDOW: u64 = 128000;

/// Status of Codex CLI installation and configuration
#[derive(Debug, serde::Serialize)]
pub struct CodexStatus {
    /// Whether the ~/.codex/ directory exists
    pub installed: bool,
    /// Path to the config.toml (if exists)
    pub config_path: Option<String>,
    /// Whether a backup from a previous apply exists
    pub has_backup: bool,
    /// Current config content preview (first 500 chars)
    pub current_config_preview: Option<String>,
    /// Whether the config currently points to Antigravity Hub
    pub is_antigravity_configured: bool,
}

/// Result of applying configuration to Codex CLI
#[derive(Debug, serde::Serialize)]
pub struct ApplyResult {
    pub success: bool,
    pub message: String,
    pub config_path: String,
    pub backup_path: Option<String>,
}

/// Result of checking for environment variable conflicts
#[derive(Debug, serde::Serialize)]
pub struct EnvConflictResult {
    pub has_openai_api_key: bool,
    pub has_openai_base_url: bool,
    pub has_openai_org_id: bool,
    pub has_codex_home: bool,
    pub messages: Vec<String>,
}

// ── Helpers ──

/// Resolve the Codex CLI home directory.
/// Checks `$CODEX_HOME` env var first, then falls back to `~/.codex`.
fn resolve_codex_home() -> PathBuf {
    if let Ok(home) = std::env::var("CODEX_HOME") {
        return PathBuf::from(home);
    }
    let home_dir = dirs::home_dir().expect("Cannot determine home directory");
    home_dir.join(CODEX_DIR)
}

/// Path to config.toml
fn config_path() -> PathBuf {
    resolve_codex_home().join(CONFIG_FILE)
}

/// Path to backup file
fn backup_path() -> PathBuf {
    let cfg = config_path();
    let file_name = cfg.file_name().unwrap_or_default().to_string_lossy();
    let bak_name = format!("{}{}", file_name, BACKUP_SUFFIX);
    let mut path = resolve_codex_home();
    path.push(&bak_name);
    path
}

/// Check whether the parsed TOML config points to Antigravity Hub.
/// Looks for a `model_providers.custom.base_url` that contains
/// "127.0.0.1" or "localhost" with the proxy port.
fn is_antigravity_config(config: &toml::Table) -> bool {
    if let Some(providers) = config.get("model_providers").and_then(|v| v.as_table()) {
        if let Some(provider_entry) = providers.get("custom").and_then(|v| v.as_table()) {
            if let Some(base_url) = provider_entry.get("base_url").and_then(|v| v.as_str()) {
                if base_url.contains("127.0.0.1") || base_url.contains("localhost") {
                    return true;
                }
            }
        }
    }
    false
}

/// Validate proxy configuration parameters.
fn validate_params(proxy_host: &str, proxy_port: u16, path_prefix: &str, model_name: &str) -> Result<(), String> {
    if proxy_host.is_empty() {
        return Err("Proxy host cannot be empty".to_string());
    }
    if proxy_port == 0 {
        return Err("Proxy port must be between 1 and 65535".to_string());
    }
    if path_prefix.is_empty() {
        return Err("Path prefix cannot be empty".to_string());
    }
    if path_prefix.contains('/') || path_prefix.contains('\\') || path_prefix.contains(' ') {
        return Err("Path prefix must not contain slashes, backslashes, or spaces".to_string());
    }
    if model_name.is_empty() {
        return Err("Model name cannot be empty".to_string());
    }
    Ok(())
}

/// Validate optional model_reasoning_effort value.
fn validate_reasoning_effort(effort: Option<&str>) -> Result<(), String> {
    if let Some(e) = effort {
        if e != "low" && e != "medium" && e != "high" {
            return Err(format!(
                "Invalid model_reasoning_effort '{}'. Must be one of: low, medium, high",
                e
            ));
        }
    }
    Ok(())
}

/// Calculate a smart auto_compact_token_limit based on the context window size.
///
/// Strategy:
/// - ≤ 128K (standard): use half, capped at 64K (balanced compression)
/// - ≤ 200K: use 100K (generous for mid-range)
/// - ≤ 1M (1,048,576): use 200K (allows large context but compresses early)
/// - > 1M: use window / 4, capped at 500K (very large windows get aggressive compression)
fn calc_auto_compact(context_window: u64) -> u64 {
    if context_window <= 128_000 {
        std::cmp::min(context_window / 2, 64_000)
    } else if context_window <= 200_000 {
        100_000
    } else if context_window <= 1_048_576 {
        200_000
    } else {
        std::cmp::min(context_window / 4, 500_000)
    }
}

/// Calculate the effective context window percent based on window size.
/// Very large windows get a slightly lower effective percent to account
/// for overhead from tool definitions, system prompts, etc.
fn calc_effective_pct(context_window: u64) -> u64 {
    if context_window >= 1_000_000 {
        90
    } else if context_window >= 200_000 {
        92
    } else {
        95
    }
}

/// Calculate truncation policy limit based on context window.
/// Larger windows can tolerate more truncation tokens.
fn calc_truncation_limit(context_window: u64) -> u64 {
    if context_window >= 1_000_000 {
        50_000
    } else if context_window >= 200_000 {
        20_000
    } else {
        10_000
    }
}

/// Generate a model catalog file for Codex Desktop.
///
/// Codex Desktop uses a `model_catalog_json` file to know which models are
/// available for selection in the UI. Without it, Desktop falls back to a
/// built-in catalog that may contain models not configured in Antigravity Hub,
/// causing 404 errors when the proxy routes them to the wrong upstream.
///
/// The catalog file is written to `~/.codex/model-catalogs/{path_prefix}-models.json`
/// and contains all models for the platform identified by `path_prefix`.
///
/// Enhanced features:
/// - Per-model context window sizes (128K, 200K, 1M+)
/// - Smart auto_compact_token_limit based on window size
/// - Proper max_context_window and effective_context_window_percent
/// - Scaled truncation policies
fn generate_model_catalog(path_prefix: &str) -> Result<String, String> {
    use crate::modules::config;
    use crate::modules::model_manager;

    // Look up the platform by path_prefix
    let app_config = config::load_app_config()
        .map_err(|e| format!("Failed to load app config: {}", e))?;

    let platform = app_config.platforms.iter()
        .find(|p| p.path_prefix == path_prefix)
        .ok_or_else(|| format!("Platform with path_prefix '{}' not found in config", path_prefix))?;

    let platform_id = &platform.id;
    let platform_name = &platform.name;

    // List all models for this platform
    let models = model_manager::list_models(platform_id)
        .map_err(|e| format!("Failed to list models for platform '{}': {}", platform_id, e))?;

    // Build the catalog entries with per-model context windows
    let catalog_models: Vec<serde_json::Value> = models.iter().map(|m| {
        let context_window = m.max_input_tokens.unwrap_or(DEFAULT_CONTEXT_WINDOW);
        let max_window = context_window;
        let auto_compact = calc_auto_compact(context_window);
        let effective_pct = calc_effective_pct(context_window);
        let truncation_limit = calc_truncation_limit(context_window);

        // Determine input modalities based on window size
        // Very large windows (1M+) support audio in addition to text+image
        let input_modalities = if context_window >= 1_000_000 {
            vec!["text", "image", "audio"]
        } else {
            vec!["text", "image"]
        };

        serde_json::json!({
            "model": m.model_name,
            "slug": m.model_name,
            "display_name": format!("{} / {}", platform_name, m.display_name),
            "description": m.model_name,
            "visibility": "list",
            "supported_in_api": true,
            "context_window": context_window,
            "max_context_window": max_window,
            "effective_context_window_percent": effective_pct,
            "auto_compact_token_limit": auto_compact,
            "input_modalities": input_modalities,
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
                "limit": truncation_limit
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

    // Write the catalog file
    let catalog_filename = CATALOG_FILE.replace("{}", path_prefix);
    let catalog_path = catalog_dir.join(&catalog_filename);
    let catalog_json = serde_json::to_string_pretty(&catalog)
        .map_err(|e| format!("Failed to serialize model catalog: {}", e))?;

    // Write atomically using temp file
    let temp_path = catalog_dir.join(format!("{}.tmp", catalog_filename));
    std::fs::write(&temp_path, &catalog_json)
        .map_err(|e| format!("Failed to write model catalog: {}", e))?;
    std::fs::rename(&temp_path, &catalog_path)
        .map_err(|e| format!("Failed to finalize model catalog: {}", e))?;

    info!(
        "Model catalog generated: {:?} ({} models, platform='{}')",
        catalog_path,
        models.len(),
        platform_name
    );

    Ok(catalog_path.to_string_lossy().to_string())
}

// ── Public API ──

/// Check Codex CLI installation status and current configuration.
pub fn check_codex_status() -> CodexStatus {
    let codex_dir = resolve_codex_home();
    let cfg_path = config_path();
    let bak_path = backup_path();

    let installed = codex_dir.exists();
    let config_exists = cfg_path.exists();
    let has_backup = bak_path.exists();

    let (current_config_preview, is_antigravity_configured) = if config_exists {
        match std::fs::read_to_string(&cfg_path) {
            Ok(content) => {
                let preview = Some(content.chars().take(500).collect::<String>());
                let is_ag = content.parse::<toml::Table>()
                    .ok()
                    .map(|table| is_antigravity_config(&table))
                    .unwrap_or(false);
                (preview, is_ag)
            }
            Err(_) => (None, false),
        }
    } else {
        (None, false)
    };

    CodexStatus {
        installed,
        config_path: if config_exists {
            Some(cfg_path.to_string_lossy().to_string())
        } else {
            None
        },
        has_backup,
        current_config_preview,
        is_antigravity_configured,
    }
}

/// Backup existing config.toml to {config}.antigravity.bak
pub fn backup_codex_config() -> Result<Option<String>, String> {
    let src = config_path();
    if !src.exists() {
        return Ok(None);
    }
    let dst = backup_path();
    std::fs::copy(&src, &dst)
        .map_err(|e| format!("Failed to backup Codex config: {}", e))?;
    info!("Codex config backed up: {} → {}", src.display(), dst.display());
    Ok(Some(dst.to_string_lossy().to_string()))
}

/// Write Antigravity Hub proxy configuration to Codex CLI's config.toml.
///
/// Uses a standard `[model_providers.custom]` entry with `base_url` pointing
/// to the Antigravity Hub proxy. Merges with existing config — only updates
/// the keys that Antigravity Hub manages (model, model_provider, etc.) and
/// preserves all other Codex CLI settings.
///
/// Sets `preferred_auth_method = "apikey"` so Codex CLI uses API Key auth
/// instead of ChatGPT login — this avoids the "cannot open ChatGPT" issue
/// when using a proxy endpoint.
///
/// # Arguments
/// * `proxy_host` - Proxy host (e.g., "127.0.0.1")
/// * `proxy_port` - Proxy port (e.g., 8045)
/// * `path_prefix` - Platform path prefix for routing (e.g., "sensenova", "openai")
/// * `model_name` - The model ID to set as default (e.g., "gpt-4o")
pub fn apply_codex_config(
    proxy_host: &str,
    proxy_port: u16,
    path_prefix: &str,
    model_name: &str,
    reasoning_effort: Option<&str>,
    disable_response_storage: Option<bool>,
    api_key: Option<&str>,
) -> Result<ApplyResult, String> {
    // ── Input validation ──
    validate_params(proxy_host, proxy_port, path_prefix, model_name)?;
    validate_reasoning_effort(reasoning_effort)?;

    let codex_dir = resolve_codex_home();

    // Ensure ~/.codex/ directory exists
    if !codex_dir.exists() {
        std::fs::create_dir_all(&codex_dir)
            .map_err(|e| format!("Failed to create Codex directory: {}", e))?;
    }

    let cfg_path = config_path();
    let bak_path = backup_path();

    // Backup existing config
    let backup_path_str = if cfg_path.exists() {
        std::fs::copy(&cfg_path, &bak_path)
            .map_err(|e| format!("Failed to backup existing config: {}", e))?;
        info!("Backed up existing config to {:?}", bak_path);
        Some(bak_path.to_string_lossy().to_string())
    } else {
        None
    };

    // ── Load existing config or start fresh ──
    // Load the existing config.toml so we can update only the keys we manage,
    // preserving all other Codex CLI settings (other providers, experimental
    // flags, etc.). If the file doesn't exist yet, start with an empty table.
    let mut config: toml::Table = if cfg_path.exists() {
        let content = std::fs::read_to_string(&cfg_path)
            .map_err(|e| format!("Failed to read existing config: {}", e))?;
        content.parse::<toml::Table>()
            .map_err(|e| format!("Failed to parse existing config: {}", e))?
    } else {
        toml::Table::new()
    };

    // Build the proxy base URL
    // Codex CLI appends /v1/responses internally — the proxy handles the
    // Responses API ↔ Chat Completions translation transparently.
    // Use 127.0.0.1 instead of the binding address (0.0.0.0) so Codex CLI
    // can actually reach the proxy.
    let client_host = if proxy_host == "0.0.0.0" { "127.0.0.1" } else { proxy_host };
    let proxy_base_url = format!("http://{}:{}/{}/v1", client_host, proxy_port, path_prefix);

    // ── Try generating model catalog for Codex Desktop ──
    // If successful, we produce a Desktop-compatible config (minimal fields).
    // If it fails, we fall back to CLI mode with all standard fields.
    let is_desktop = match generate_model_catalog(path_prefix) {
        Ok(catalog_path) => {
            info!("model_catalog_json set to: {}", catalog_path);
            config.insert(
                "model_catalog_json".to_string(),
                toml::Value::String(catalog_path),
            );
            true
        }
        Err(e) => {
            warn!("Failed to generate model catalog (non-fatal): {}", e);
            false
        }
    };

    // ── Set top-level keys ──
    config.insert(
        "model_provider".to_string(),
        toml::Value::String("custom".to_string()),
    );
    config.insert(
        "model".to_string(),
        toml::Value::String(model_name.to_string()),
    );

    // CLI-specific fields — skip for Desktop mode to avoid compatibility issues
    // Desktop's TOML parser doesn't recognize these fields, which can cause
    // "Windows installation not complete" errors during initialization.
    if !is_desktop {
        config.insert(
            "preferred_auth_method".to_string(),
            toml::Value::String("apikey".to_string()),
        );
        if let Some(val) = disable_response_storage {
            config.insert(
                "disable_response_storage".to_string(),
                toml::Value::Boolean(val),
            );
        }
        if let Some(effort) = reasoning_effort {
            config.insert(
                "model_reasoning_effort".to_string(),
                toml::Value::String(effort.to_string()),
            );
        }
    }

    // ── [model_providers.custom] section ──
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

    if is_desktop {
        // Desktop reads API keys from environment variables, not from config.
        // `env_key` tells Desktop which env var to use for the custom provider.
        provider_table.insert(
            "env_key".to_string(),
            toml::Value::String("OPENAI_API_KEY".to_string()),
        );

        // Add models array so Codex Desktop's UI dropdown shows available models.
        // Without this, the model selector in Desktop's settings page is empty.
        if let Some(platform) = crate::modules::config::load_app_config()
            .ok()
            .and_then(|cfg| cfg.platforms.into_iter().find(|p| p.path_prefix == path_prefix))
        {
            if let Ok(models) = crate::modules::model_manager::list_models(&platform.id) {
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
        }
    }

    if !is_desktop {
        // CLI-only: prevents OpenAI OAuth, injects API key
        provider_table.insert(
            "requires_openai_auth".to_string(),
            toml::Value::Boolean(false),
        );
        if let Some(key) = api_key {
            if !key.is_empty() {
                provider_table.insert(
                    "api_key".to_string(),
                    toml::Value::String(key.to_string()),
                );
            }
        }
    }

    let mut model_providers = config
        .get("model_providers")
        .and_then(|v| v.as_table())
        .map(|t| t.clone())
        .unwrap_or_else(toml::Table::new);
    model_providers.insert(
        "custom".to_string(),
        toml::Value::Table(provider_table),
    );
    config.insert(
        "model_providers".to_string(),
        toml::Value::Table(model_providers),
    );

    // Serialize and write
    let output = toml::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize Codex config: {}", e))?;

    // Add a header comment
    let config_type = if is_desktop { "Desktop" } else { "CLI" };
    let final_content = format!(
        "# Codex {} Configuration\n\
         # Managed by Antigravity Hub\n\
         # Applied at: {}\n\
         # To revert, delete this file or restore the backup.\n\n{}",
        config_type,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        output
    );

    // Write atomically using temp file + rename
    let temp_path = codex_dir.join("config.toml.tmp");
    std::fs::write(&temp_path, &final_content)
        .map_err(|e| format!("Failed to write Codex config: {}", e))?;
    std::fs::rename(&temp_path, &cfg_path)
        .map_err(|e| format!("Failed to finalize Codex config: {}", e))?;

    info!(
        "Codex config applied: type={}, model={}, path_prefix={}, base_url={}, path={:?}",
        config_type, model_name, path_prefix, proxy_base_url, cfg_path
    );

    Ok(ApplyResult {
        success: true,
        message: format!(
            "Codex {} configuration applied!\n\
             Provider: custom (routed via /{}/)\n\
             Model: {}\n\
             Base URL: {}\n\
             File: {}",
            config_type,
            path_prefix,
            model_name,
            proxy_base_url,
            cfg_path.display()
        ),
        config_path: cfg_path.to_string_lossy().to_string(),
        backup_path: backup_path_str,
    })
}

/// Restore the backup config.toml (from `.antigravity.bak`)
pub fn restore_codex_config() -> Result<ApplyResult, String> {
    let cfg_path = config_path();
    let bak_path = backup_path();

    if !bak_path.exists() {
        return Err("No backup found to restore.".to_string());
    }

    // Copy backup back to config
    std::fs::copy(&bak_path, &cfg_path)
        .map_err(|e| format!("Failed to restore Codex config: {}", e))?;

    // Remove backup file
    let _ = std::fs::remove_file(&bak_path);

    info!("Codex config restored from backup: {:?}", bak_path);

    Ok(ApplyResult {
        success: true,
        message: "Configuration restored from backup.".to_string(),
        config_path: cfg_path.to_string_lossy().to_string(),
        backup_path: None,
    })
}

/// Clear Codex CLI authentication data (auth.json and sqlite/ directory).
/// This resolves OAuth conflicts that occur when Codex CLI has previously
/// logged in with an OpenAI account — residual OAuth tokens can interfere
/// with custom provider authentication.
///
/// After this, Codex CLI will use the API key from config.toml for auth.
pub fn clear_codex_auth() -> Result<ApplyResult, String> {
    let codex_home = resolve_codex_home();

    if !codex_home.exists() {
        return Err("Codex CLI directory not found. Nothing to clear.".to_string());
    }

    let mut cleared_items = Vec::new();

    // Delete auth.json
    let auth_path = codex_home.join("auth.json");
    if auth_path.exists() {
        std::fs::remove_file(&auth_path)
            .map_err(|e| format!("Failed to delete auth.json: {}", e))?;
        cleared_items.push(format!("Deleted {}", auth_path.display()));
        info!("Codex auth file deleted: {:?}", auth_path);
    }

    // Delete sqlite/ directory (contains session data, OAuth tokens, etc.)
    let sqlite_path = codex_home.join("sqlite");
    if sqlite_path.exists() {
        std::fs::remove_dir_all(&sqlite_path)
            .map_err(|e| format!("Failed to delete sqlite directory: {}", e))?;
        cleared_items.push(format!("Deleted {}", sqlite_path.display()));
        info!("Codex sqlite directory deleted: {:?}", sqlite_path);
    }

    if cleared_items.is_empty() {
        return Ok(ApplyResult {
            success: true,
            message: "No OAuth data found to clear. Config is clean.".to_string(),
            config_path: config_path().to_string_lossy().to_string(),
            backup_path: None,
        });
    }

    let message = format!(
        "Codex OAuth data cleared successfully!\n\nCleared:\n{}",
        cleared_items.join("\n")
    );

    Ok(ApplyResult {
        success: true,
        message,
        config_path: config_path().to_string_lossy().to_string(),
        backup_path: None,
    })
}

// ── Codex Provider Profile Management ──

/// List all saved Codex provider profiles.
pub fn list_codex_profiles() -> Result<Vec<crate::models::CodexProfile>, String> {
    let app_config = crate::modules::config::load_app_config()?;
    Ok(app_config.codex_profiles)
}

/// Save a Codex provider profile.
/// If the profile has an existing `id`, it updates the existing one.
/// If `id` is empty or None, it creates a new profile with a generated UUID.
pub fn save_codex_profile(
    id: Option<String>,
    name: String,
    platform_id: String,
    model_name: String,
    proxy_host: String,
    proxy_port: u16,
    path_prefix: String,
    reasoning_effort: Option<String>,
    disable_response_storage: Option<bool>,
    api_key: Option<String>,
) -> Result<crate::models::CodexProfile, String> {
    let mut app_config = crate::modules::config::load_app_config()?;
    let now = chrono::Utc::now().timestamp();

    // Determine profile ID and whether this is an update
    let (profile_id, is_update) = match id.as_deref() {
        Some(existing) if !existing.is_empty() => {
            (existing.to_string(), true)
        }
        _ => (Uuid::new_v4().to_string(), false),
    };

    // Preserve original created_at on update
    let original_created_at = if is_update {
        app_config.codex_profiles.iter()
            .find(|p| p.id == profile_id)
            .map(|p| p.created_at)
    } else {
        None
    };

    let profile = crate::models::CodexProfile {
        id: profile_id.clone(),
        name,
        platform_id,
        model_name,
        proxy_host,
        proxy_port,
        path_prefix,
        reasoning_effort,
        disable_response_storage,
        api_key,
        created_at: original_created_at.unwrap_or(now),
        updated_at: now,
    };

    // Find and replace existing, or append
    if let Some(pos) = app_config.codex_profiles.iter().position(|p| p.id == profile_id) {
        app_config.codex_profiles[pos] = profile.clone();
    } else {
        app_config.codex_profiles.push(profile.clone());
    }

    crate::modules::config::save_app_config(&app_config)?;
    info!("Codex profile saved: {} ({})", profile.name, profile.id);
    Ok(profile)
}

/// Delete a Codex provider profile by ID.
pub fn delete_codex_profile(profile_id: String) -> Result<(), String> {
    let mut app_config = crate::modules::config::load_app_config()?;
    let len_before = app_config.codex_profiles.len();
    app_config.codex_profiles.retain(|p| p.id != profile_id);
    if app_config.codex_profiles.len() == len_before {
        return Err(format!("Codex profile '{}' not found", profile_id));
    }
    crate::modules::config::save_app_config(&app_config)?;
    info!("Codex profile deleted: {}", profile_id);
    Ok(())
}

/// Apply a saved Codex provider profile.
/// Loads the profile, resolves the platform's path_prefix if empty,
/// then calls `apply_codex_config` with the profile's settings.
pub fn apply_codex_profile(profile_id: String) -> Result<ApplyResult, String> {
    let app_config = crate::modules::config::load_app_config()?;

    let profile = app_config.codex_profiles.iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("Codex profile '{}' not found", profile_id))?;

    // Resolve path_prefix from platform if needed
    let path_prefix = if profile.path_prefix.is_empty() {
        app_config.platforms.iter()
            .find(|p| p.id == profile.platform_id)
            .map(|p| p.path_prefix.as_str())
            .unwrap_or("openai")
            .to_string()
    } else {
        profile.path_prefix.clone()
    };

    apply_codex_config(
        &profile.proxy_host,
        profile.proxy_port,
        &path_prefix,
        &profile.model_name,
        profile.reasoning_effort.as_deref(),
        profile.disable_response_storage,
        profile.api_key.as_deref(),
    )
}

/// Check for environment variable conflicts that could interfere with Codex CLI.
///
/// Common conflicts:
/// - `OPENAI_API_KEY` set → Codex CLI might use this instead of the configured key
/// - `OPENAI_BASE_URL` set → Could override the configured base_url
/// - `OPENAI_ORG_ID` set → May cause routing issues
/// - `CODEX_HOME` set → Changes where Codex looks for config
pub fn check_codex_env_conflicts() -> EnvConflictResult {
    let mut messages = Vec::new();

    let has_openai_api_key = if let Ok(val) = std::env::var("OPENAI_API_KEY") {
        if val.starts_with("sk-") || !val.is_empty() {
            messages.push(format!(
                "OPENAI_API_KEY is set (starts with '{}...'). This may override the configured API key.",
                &val[..val.len().min(8)]
            ));
            true
        } else {
            false
        }
    } else {
        false
    };

    let has_openai_base_url = if let Ok(val) = std::env::var("OPENAI_BASE_URL") {
        messages.push(format!(
            "OPENAI_BASE_URL is set to '{}'. This will override the proxy base URL.",
            val
        ));
        true
    } else {
        false
    };

    let has_openai_org_id = if std::env::var("OPENAI_ORG_ID").is_ok() {
        messages.push(
            "OPENAI_ORG_ID is set. This may cause routing issues with third-party providers."
                .to_string(),
        );
        true
    } else {
        false
    };

    let has_codex_home = if let Ok(val) = std::env::var("CODEX_HOME") {
        messages.push(format!(
            "CODEX_HOME is set to '{}'. Codex CLI will use this directory instead of ~/.codex.",
            val
        ));
        true
    } else {
        false
    };

    if messages.is_empty() {
        messages.push("No conflicting environment variables detected.".to_string());
    }

    EnvConflictResult {
        has_openai_api_key,
        has_openai_base_url,
        has_openai_org_id,
        has_codex_home,
        messages,
    }
}
