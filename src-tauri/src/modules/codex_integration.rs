use std::path::{Path, PathBuf};
use tracing::{info, warn};

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
    let mut path = config_path();
    let ext = path.extension().map(|e| e.to_string_lossy().to_string());
    match ext {
        Some(e) => {
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            path.set_file_name(format!("{}.{}{}", stem, BACKUP_SUFFIX.trim_start_matches('.'), e));
            path
        }
        None => {
            // No extension: just append the backup suffix
            let mut p = config_path();
            let new_name = format!("{}{}", p.file_name().unwrap().to_string_lossy(), BACKUP_SUFFIX);
            p.set_file_name(new_name);
            p
        }
    }
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
                let is_ag = content.contains("Managed by Antigravity Hub");
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
            Some(cfg_path.to_string_lossy().to_string()) // show the expected path even if not exists
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
/// This writes a minimal config that points Codex CLI to the local Antigravity Hub proxy.
/// Any existing configuration is backed up automatically.
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

    // Parse existing config if present to preserve other settings
    let existing_content = if cfg_path.exists() {
        std::fs::read_to_string(&cfg_path).ok()
    } else {
        None
    };

    // Build the TOML config for Antigravity Hub
    // Codex CLI ConfigToml uses openai_base_url (top-level) to override the
    // built-in OpenAI provider's base URL. There is NO [api] section.
    // The built-in default is "https://api.openai.com/v1", so we keep /v1.
    // The path_prefix is the platform's routing path (e.g., "sensenova").
    // See: https://github.com/openai/codex/blob/main/codex-rs/config/src/config_toml.rs
    // If proxy_host is "0.0.0.0" (bind-all), use 127.0.0.1 as the connect address.
    let connect_host = if proxy_host == "0.0.0.0" { "127.0.0.1" } else { proxy_host };
    let openai_base_url = format!("http://{}:{}/{}/v1", connect_host, proxy_port, path_prefix);

    // We'll construct the new config. Try to preserve existing top-level keys
    // that aren't related to api/model configuration.
    let mut config = toml::Table::new();

    // Parse existing config to preserve user settings
    if let Some(ref content) = existing_content {
        if let Ok(existing_table) = content.parse::<toml::Table>() {
            // Preserve non-conflicting top-level keys and sections
            for (key, value) in &existing_table {
                match key.as_str() {
                    "model" | "openai_base_url" => {
                        // These will be overwritten
                    }
                    _ => {
                        config.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }

    // Set the default model
    config.insert("model".to_string(), toml::Value::String(model_name.to_string()));

    // Set openai_base_url to point to the Antigravity Hub proxy
    // Codex CLI appends /v1/responses internally — no suffix needed here.
    config.insert(
        "openai_base_url".to_string(),
        toml::Value::String(openai_base_url.clone()),
    );

    // Serialize and write
    let output = toml::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize Codex config: {}", e))?;

    // Add a header comment
    let final_content = format!(
        "# Codex CLI Configuration\n\
         # Managed by Antigravity Hub v5.0.0\n\
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
        "Codex config applied: model={}, openai_base_url={}, path={:?}",
        model_name, openai_base_url, cfg_path
    );

    Ok(ApplyResult {
        success: true,
        message: format!(
            "Configuration applied successfully!\n\
             Model: {}\n\
             openai_base_url: {}\n\
             Config: {}",
            model_name, openai_base_url, cfg_path.display()
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
