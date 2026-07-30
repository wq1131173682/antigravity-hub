import { request } from '../utils/request';

export interface ProviderProfile {
  id: string;
  name: string;
  platform_id: string;
  model_id: string;
  notes?: string | null;
  created_at: number;
  updated_at: number;
}

export async function listProfiles(): Promise<ProviderProfile[]> {
  return request<ProviderProfile[]>('list_profiles');
}

export async function saveProfile(params: {
  id?: string;
  name: string;
  platformId: string;
  modelId: string;
  notes?: string;
}): Promise<ProviderProfile> {
  // Map camelCase from frontend to snake_case for Rust backend
  return request<ProviderProfile>('save_profile', {
    id: params.id || null,
    name: params.name,
    platform_id: params.platformId,
    model_id: params.modelId,
    notes: params.notes || null,
  });
}

export async function deleteProfile(profileId: string): Promise<void> {
  return request<void>('delete_profile', { profileId });
}
