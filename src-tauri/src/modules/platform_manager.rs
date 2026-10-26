use std::fs;
use std::path::PathBuf;
use uuid::Uuid;
use crate::models::Platform;

// ── Path prefix rules ──

/// True for API version path segments: `v1`, `v2`, `v1beta`, ...
///
/// Such a segment must never be used as a platform `path_prefix`, because the
/// proxy treats `/<version>/...` as a UNIFIED request whose platform comes from
/// the body's `model` id. A platform named `v1` would make every unified request
/// ambiguous. Requires a digit straight after the `v`, so an ordinary prefix such
/// as `videos` remains usable.
pub fn is_version_segment(seg: &str) -> bool {
    let bytes = seg.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'v' || !bytes[1].is_ascii_digit() {
        return false;
    }
    bytes[1..].iter().all(|b| b.is_ascii_digit() || b.is_ascii_alphabetic())
}

/// Validate a platform `path_prefix`.
///
/// A prefix has to be usable as a URL path segment AND as the namespace in the
/// public model id `<prefix>:<model>`, so `/`, whitespace and `:` are rejected.
/// `except_platform_id` lets an update keep its own existing prefix.
pub fn validate_path_prefix(
    config: &crate::models::AppConfig,
    path_prefix: &str,
    except_platform_id: Option<&str>,
) -> Result<(), String> {
    let prefix = path_prefix.trim();
    if prefix.is_empty() {
        return Err("path_prefix must not be empty".to_string());
    }
    if prefix.contains('/') || prefix.contains('\\') {
        return Err(format!(
            "path_prefix '{}' must not contain a slash - it is a single URL path segment",
            prefix
        ));
    }
    if prefix.contains(':') {
        return Err(format!(
            "path_prefix '{}' must not contain ':' - that character separates the platform \
             namespace from the model name in public model ids",
            prefix
        ));
    }
    if prefix.chars().any(char::is_whitespace) {
        return Err(format!("path_prefix '{}' must not contain whitespace", prefix));
    }
    if is_version_segment(prefix) {
        return Err(format!(
            "path_prefix '{}' is reserved: '/{}/...' is the unified API root, whose platform is \
             chosen by the request's model id ('<platform>:<model>'). Pick another prefix.",
            prefix, prefix
        ));
    }
    if let Some(existing) = config
        .platforms
        .iter()
        .find(|p| p.path_prefix == prefix && Some(p.id.as_str()) != except_platform_id)
    {
        return Err(format!(
            "path_prefix '{}' is already used by platform '{}'",
            prefix, existing.name
        ));
    }
    Ok(())
}

// ── Platform CRUD ──

/// List all platforms (from AppConfig)
pub fn list_platforms(config: &crate::models::AppConfig) -> Vec<Platform> {
    let mut platforms = config.platforms.clone();
    platforms.sort_by_key(|p| p.sort_order);
    platforms
}

/// Add a new platform
pub fn add_platform(
    config: &mut crate::models::AppConfig,
    name: String,
    base_url: String,
    path_prefix: String,
    notes: Option<String>,
) -> Result<Platform, String> {
    validate_path_prefix(config, &path_prefix, None)?;
    let id = Uuid::new_v4().to_string();
    let mut platform = Platform::new(id, name, base_url, path_prefix.trim().to_string());
    platform.notes = notes;
    platform.sort_order = config.platforms.len() as i32;
    config.platforms.push(platform.clone());
    Ok(platform)
}

/// Update a platform
pub fn update_platform(
    config: &mut crate::models::AppConfig,
    platform_id: &str,
    name: Option<String>,
    base_url: Option<String>,
    path_prefix: Option<String>,
    notes: Option<String>,
) -> Result<Platform, String> {
    // Validate against the CURRENT config before borrowing the platform mutably.
    if let Some(ref prefix) = path_prefix {
        validate_path_prefix(config, prefix, Some(platform_id))?;
    }

    let platform = config.platforms.iter_mut()
        .find(|p| p.id == platform_id)
        .ok_or_else(|| format!("Platform not found: {}", platform_id))?;
    
    if let Some(name) = name {
        platform.name = name;
    }
    if let Some(base_url) = base_url {
        platform.base_url = base_url;
    }
    if let Some(path_prefix) = path_prefix {
        platform.path_prefix = path_prefix.trim().to_string();
    }
    platform.notes = notes;
    
    Ok(platform.clone())
}

/// Delete a platform and its keys
pub fn delete_platform(
    config: &mut crate::models::AppConfig,
    platform_id: &str,
) -> Result<(), String> {
    let pos = config.platforms.iter().position(|p| p.id == platform_id)
        .ok_or_else(|| format!("Platform not found: {}", platform_id))?;
    config.platforms.remove(pos);
    
    // Also delete all API keys for this platform
    if let Ok(mut key_store) = super::keystore::load_key_store() {
        key_store.keys.retain(|k| k.platform_id != platform_id);
        let _ = super::keystore::save_key_store(&key_store);
    }
    
    Ok(())
}

/// Reorder platforms
pub fn reorder_platforms(
    config: &mut crate::models::AppConfig,
    platform_ids: Vec<String>,
) -> Result<(), String> {
    let mut reordered = Vec::new();
    for (i, id) in platform_ids.iter().enumerate() {
        if let Some(platform) = config.platforms.iter().find(|p| &p.id == id) {
            let mut p = platform.clone();
            p.sort_order = i as i32;
            reordered.push(p);
        }
    }
    // Add any platforms not in the list
    for p in &config.platforms {
        if !platform_ids.contains(&p.id) {
            let mut p = p.clone();
            p.sort_order = reordered.len() as i32;
            reordered.push(p);
        }
    }
    config.platforms = reordered;
    Ok(())
}

/// Get data directory
pub fn get_data_dir() -> Result<PathBuf, String> {
    if let Ok(env_path) = std::env::var("ABV_DATA_DIR") {
        if !env_path.trim().is_empty() {
            let data_dir = PathBuf::from(env_path);
            if !data_dir.exists() {
                fs::create_dir_all(&data_dir)
                    .map_err(|e| format!("failed_to_create_custom_data_dir: {}", e))?;
            }
            return Ok(data_dir);
        }
    }

    let home = dirs::home_dir().ok_or("failed_to_get_home_dir")?;
    let data_dir = home.join(".antigravity_tools");
    if !data_dir.exists() {
        fs::create_dir_all(&data_dir).map_err(|e| format!("failed_to_create_data_dir: {}", e))?;
    }
    Ok(data_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AppConfig;

    fn config_with(prefixes: &[&str]) -> AppConfig {
        let mut config = AppConfig::new();
        for (i, prefix) in prefixes.iter().enumerate() {
            config.platforms.push(Platform::new(
                format!("id-{}", i),
                format!("Platform {}", i),
                "https://example.com".to_string(),
                (*prefix).to_string(),
            ));
        }
        config
    }

    #[test]
    fn test_accepts_ordinary_prefixes() {
        let config = config_with(&["sensenova"]);
        for ok in ["glm", "videos", "v", "codex-2", "my_platform"] {
            assert!(
                validate_path_prefix(&config, ok, None).is_ok(),
                "'{}' must be a valid path prefix",
                ok
            );
        }
    }

    /// A prefix like `v1` would collide with the unified API root: the proxy
    /// treats `/v1/...` as "platform comes from the model namespace", so a
    /// platform reachable at `/v1` could never be addressed reliably.
    #[test]
    fn test_rejects_version_like_prefix() {
        let config = config_with(&[]);
        for bad in ["v1", "v2", "v1beta", "v10"] {
            let err = validate_path_prefix(&config, bad, None)
                .expect_err(&format!("'{}' must be rejected", bad));
            assert!(err.contains("reserved"), "error must explain why, got: {}", err);
        }
    }

    #[test]
    fn test_rejects_colon_slash_whitespace_and_empty() {
        let config = config_with(&[]);
        for bad in [r"a:b", r"a/b", r"a\b", "a b", ""] {
            assert!(
                validate_path_prefix(&config, bad, None).is_err(),
                "'{}' must be rejected",
                bad
            );
        }
    }

    #[test]
    fn test_rejects_duplicate_prefix_but_allows_self() {
        let config = config_with(&["sensenova", "glm"]);
        assert!(
            validate_path_prefix(&config, "glm", None).is_err(),
            "a prefix already in use must be rejected"
        );
        // Updating the platform that owns the prefix with the same value is fine.
        assert!(
            validate_path_prefix(&config, "glm", Some("id-1")).is_ok(),
            "a platform may keep its own prefix"
        );
    }

    #[test]
    fn test_update_platform_validates_prefix() {
        let mut config = config_with(&["sensenova", "glm"]);
        let err = update_platform(
            &mut config,
            "id-0",
            None,
            None,
            Some("glm".to_string()),
            None,
        )
        .expect_err("renaming onto another platform's prefix must fail");
        assert!(err.contains("already used"), "got: {}", err);

        // The rejected update must not have mutated the config.
        assert_eq!(config.platforms[0].path_prefix, "sensenova");
    }

    #[test]
    fn test_add_platform_validates_and_trims_prefix() {
        let mut config = config_with(&[]);
        assert!(
            add_platform(
                &mut config,
                "SenseNova".into(),
                "https://example.com".into(),
                " v1 ".into(),
                None
            )
            .is_err(),
            "a version-like prefix must be rejected on add"
        );

        let added = add_platform(
            &mut config,
            "SenseNova".into(),
            "https://example.com".into(),
            " sensenova ".into(),
            None,
        )
        .expect("ordinary prefix must be accepted");
        assert_eq!(added.path_prefix, "sensenova", "prefix must be trimmed");
    }
}
