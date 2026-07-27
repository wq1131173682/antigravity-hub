use std::path::PathBuf;
use tracing::info;

/// Codex CLI configuration directory and file constants
const CODEX_DIR: &str = ".codex";
const CONFIG_FILE: &str = "config.toml";
const BACKUP_SUFFIX: &str = ".antigravity.bak";

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
                let is_ag = content.contains("antigravity") || content.contains("Antigravity")
                    || content.contains("model_providers");
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
            Some(cfg_path.to_string_lossy().to_string())
        },
        has_backup,
        current_config_preview,
        is_antigravity_configured,
    }
}

/// Read the current config.toml content as a string.
pub fn read_codex_config() -> Result<Option<String>, String> {
    let path = config_path();
    if !path.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| format!("Failed to read Codex config: {}", e))
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
/// Uses a custom provider entry in `[model_providers.<path_prefix>]` with
/// `base_url` pointing to the Antigravity Hub proxy. Sets
/// `preferred_auth_method = "apikey"` so Codex CLI uses API Key auth
/// (stored in auth.json) instead of ChatGPT login — this avoids the
/// "cannot open ChatGPT" issue when using a proxy endpoint.
///
/// # Arguments
/// * `proxy_host` - Proxy host (e.g., "127.0.0.1")
/// * `proxy_port` - Proxy port (e.g., 8045)
/// * `path_prefix` - Platform path prefix for routing (e.g., "sensenova", "openai")
/// * `model_name` - The model ID to set as default (e.g., "claude-sonnet-4-6-thinking")
pub fn apply_codex_config(
    proxy_host: &str,
    proxy_port: u16,
    path_prefix: &str,
    model_name: &str,
) -> Result<ApplyResult, String> {
    let codex_dir = resolve_codex_home();

    // Ensure ~/.codex/ directory exists
    if !codex_dir.exists() {
        std::fs::create_dir_all(&codex_dir)
            .map_err(|e| format!("Failed to create Codex directory {:?}: {}", codex_dir, e))?;
        info!("Created Codex directory: {:?}", codex_dir);
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

    // Parse existing config to preserve all settings
    let existing_content = if cfg_path.exists() {
        std::fs::read_to_string(&cfg_path).ok()
    } else {
        None
    };

    let mut config: toml::Table = existing_content
        .as_deref()
        .and_then(|c| c.parse::<toml::Table>().ok())
        .unwrap_or_default();

    // Build the proxy base URL
    // Codex CLI appends /v1/responses internally — the proxy handles the
    // Responses API ↔ Chat Completions translation transparently.
    let proxy_base_url = format!("http://{}:{}/{}/v1", proxy_host, proxy_port, path_prefix);

    // ── Set top-level keys ──
    // Use the path_prefix as the provider name (custom provider, not built-in
    // "openai"). This avoids the ChatGPT login flow — Codex CLI will use
    // API Key auth instead.
    config.insert(
        "model_provider".to_string(),
        toml::Value::String(path_prefix.to_string()),
    );
    config.insert(
        "model".to_string(),
        toml::Value::String(model_name.to_string()),
    );
    config.insert(
        "preferred_auth_method".to_string(),
        toml::Value::String("apikey".to_string()),
    );

    // ── Update [model_providers.<path_prefix>] section ──
    // Define the custom provider with base_url pointing to the proxy.
    // Codex CLI requires: name, base_url, wire_api = "responses".
    let model_providers = config
        .entry("model_providers".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));

    if let Some(providers_table) = model_providers.as_table_mut() {
        let provider = providers_table
            .entry(path_prefix.to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));

        if let Some(provider_table) = provider.as_table_mut() {
            provider_table.insert(
                "name".to_string(),
                toml::Value::String(path_prefix.to_string()),
            );
            provider_table.insert(
                "base_url".to_string(),
                toml::Value::String(proxy_base_url.clone()),
            );
            provider_table.insert(
                "wire_api".to_string(),
                toml::Value::String("responses".to_string()),
            );
        }
    }

    // Serialize and write
    let output = toml::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize Codex config: {}", e))?;

    // Add a header comment
    let final_content = format!(
        "# Codex CLI Configuration\n\
         # Managed by Antigravity Hub\n\
         # Applied at: {}\n\
         # To revert, delete this file or restore the backup.\n\n{}",
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
        "Codex config applied: model_provider={}, model={}, base_url={}, path={:?}",
        path_prefix, model_name, proxy_base_url, cfg_path
    );

    Ok(ApplyResult {
        success: true,
        message: format!(
            "Configuration applied successfully!\n\
             Provider: {}\n\
             Model: {}\n\
             Base URL: {}\n\
             Config: {}",
            path_prefix, model_name, proxy_base_url, cfg_path.display()
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