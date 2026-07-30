use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use crate::models::ProviderProfile;

/// List all saved provider profiles
pub fn list_profiles(config: &crate::models::AppConfig) -> Vec<ProviderProfile> {
    config.provider_profiles.clone()
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
                profile.updated_at = now;
                return Ok(profile.clone());
            }
        }
    }

    // Create new profile
    let profile = ProviderProfile {
        id: Uuid::new_v4().to_string(),
        name,
        platform_id,
        model_id,
        notes,
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
