import { request } from '../utils/request';

export interface ProviderProfile {
  id: string;
  name: string;
  platform_id: string;
  model_id: string;
  notes?: string | null;
  active: boolean;
  priority: number;
  rotation_strategy: string;
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
  active?: boolean;
  priority?: number;
  rotationStrategy?: string;
}): Promise<ProviderProfile> {
  return request<ProviderProfile>('save_profile', {
    id: params.id || null,
    name: params.name,
    platform_id: params.platformId,
    model_id: params.modelId,
    notes: params.notes || null,
    active: params.active ?? null,
    priority: params.priority ?? null,
    rotation_strategy: params.rotationStrategy || null,
  });
}

export async function deleteProfile(profileId: string): Promise<void> {
  return request<void>('delete_profile', { profileId });
}

export async function toggleProfile(profileId: string, active: boolean): Promise<ProviderProfile> {
  return request<ProviderProfile>('toggle_profile', { profileId, active });
}
