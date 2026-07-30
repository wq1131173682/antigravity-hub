use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use crate::models::profile::{ProviderProfile, RotationStrategy};

/// List all saved provider profiles
pub fn list_profiles(config: &crate::models::AppConfig) -> Vec<ProviderProfile> {
    config.provider_profiles.clone()
}

/// Get active profiles sorted by priority (ascending)
pub fn get_active_profiles(config: &crate::models::AppConfig) -> Vec<&ProviderProfile> {
    let mut profiles: Vec<&ProviderProfile> = config.provider_profiles.iter()
        .filter(|p| p.active)
        .collect();
    profiles.sort_by_key(|p| p.priority);
    profiles
}

/// Save a provider profile (create or update)
/// If `id` is Some and matches an existing profile, updates it.
/// If `id` is None or empty, creates a new profile.
pub fn save_profile(
    config: &mut crate::models::AppConfig,
    id: Option<String>,
    name: String,
    platform_id: String,
    model_id: String,
    notes: Option<String>,
    active: Option<bool>,
    priority: Option<i32>,
    rotation_strategy: Option<String>,
) -> Result<ProviderProfile, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // Check if updating an existing profile
    if let Some(ref existing_id) = id {
        if !existing_id.is_empty() {
            if let Some(profile) = config.provider_profiles.iter_mut()
                .find(|p| p.id == *existing_id)
            {
                profile.name = name;
                profile.platform_id = platform_id;
                profile.model_id = model_id;
                profile.notes = notes;
                if let Some(a) = active { profile.active = a; }
                if let Some(p) = priority { profile.priority = p; }
                if let Some(ref s) = rotation_strategy {
                    profile.rotation_strategy = serde_json::from_str(&format!("\"{}\"", s))
                        .unwrap_or_default();
                }
                profile.updated_at = now;
                return Ok(profile.clone());
            }
        }
    }

    // Create new profile
    let strategy = rotation_strategy.as_deref()
        .and_then(|s| serde_json::from_str::<RotationStrategy>(&format!("\"{}\"", s)).ok())
        .unwrap_or_default();

    let profile = ProviderProfile {
        id: Uuid::new_v4().to_string(),
        name,
        platform_id,
        model_id,
        notes,
        active: active.unwrap_or(false),
        priority: priority.unwrap_or(0),
        rotation_strategy: strategy,
        created_at: now,
        updated_at: now,
    };

    config.provider_profiles.push(profile.clone());
    Ok(profile)
}

/// Delete a provider profile by id
pub fn delete_profile(
    config: &mut crate::models::AppConfig,
    profile_id: &str,
) -> Result<(), String> {
    let len = config.provider_profiles.len();
    config.provider_profiles.retain(|p| p.id != profile_id);
    if config.provider_profiles.len() == len {
        return Err(format!("Profile not found: {}", profile_id));
    }
    Ok(())
}

/// Toggle a profile's active state
pub fn toggle_profile(
    config: &mut crate::models::AppConfig,
    profile_id: &str,
    active: bool,
) -> Result<ProviderProfile, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    if let Some(profile) = config.provider_profiles.iter_mut()
        .find(|p| p.id == profile_id)
    {
        profile.active = active;
        profile.updated_at = now;
        Ok(profile.clone())
    } else {
        Err(format!("Profile not found: {}", profile_id))
    }
}
