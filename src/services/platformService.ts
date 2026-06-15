import { request as invoke } from '../utils/request';
import { Platform, ApiKey, Model, ModelUsageEntry } from '../types/platform';

// ============================================================================
// Platform
// ============================================================================

export async function listPlatforms(): Promise<Platform[]> {
  return await invoke('list_platforms');
}

export async function addPlatform(name: string, baseUrl: string, pathPrefix: string, notes?: string): Promise<Platform> {
  return await invoke('add_platform', { name, baseUrl, pathPrefix, notes });
}

export async function updatePlatform(
  platformId: string,
  name?: string,
  baseUrl?: string,
  pathPrefix?: string,
  notes?: string
): Promise<Platform> {
  return await invoke('update_platform', { platformId, name, baseUrl, pathPrefix, notes });
}

export async function deletePlatform(platformId: string): Promise<void> {
  return await invoke('delete_platform', { platformId });
}

export async function reorderPlatforms(platformIds: string[]): Promise<void> {
  return await invoke('reorder_platforms', { platformIds });
}

// ============================================================================
// Model
// ============================================================================

export async function listModels(platformId: string): Promise<Model[]> {
  return await invoke('list_models', { platformId });
}

export async function addModel(
  platformId: string,
  modelName: string,
  displayName: string,
  per5hour?: number,
  perDay?: number,
  perMonth?: number,
): Promise<Model> {
  return await invoke('add_model', { platformId, modelName, displayName, per5hour, perDay, perMonth });
}

export async function updateModel(
  modelId: string,
  modelName?: string,
  displayName?: string,
  per5hour?: number,
  perDay?: number,
  perMonth?: number,
): Promise<Model> {
  return await invoke('update_model', { modelId, modelName, displayName, per5hour, perDay, perMonth });
}

export async function deleteModel(modelId: string): Promise<void> {
  return await invoke('delete_model', { modelId });
}

// ============================================================================
// API Key
// ============================================================================

export async function listKeys(platformId: string): Promise<ApiKey[]> {
  return await invoke('list_keys', { platformId });
}

export async function addKey(platformId: string, name: string, keyValue: string): Promise<ApiKey> {
  return await invoke('add_key', { platformId, name, keyValue });
}

export async function updateKey(keyId: string, name?: string, keyValue?: string): Promise<ApiKey> {
  return await invoke('update_key', { keyId, name, keyValue });
}

export async function deleteKey(keyId: string): Promise<void> {
  return await invoke('delete_key', { keyId });
}

export async function enableKey(keyId: string): Promise<ApiKey> {
  return await invoke('enable_key', { keyId });
}

export async function disableKey(keyId: string, reason?: string): Promise<ApiKey> {
  return await invoke('disable_key', { keyId, reason });
}

export async function setKeyStatus(
  keyId: string,
  disabled: boolean,
  disabledReason?: string,
  disabledUntil?: number
): Promise<ApiKey> {
  return await invoke('set_key_status', {
    keyId,
    disabled,
    disabledReason: disabledReason || null,
    disabledUntil: disabledUntil || null,
  });
}

// ============================================================================
// Key-Model Association
// ============================================================================

export async function getKeysForModel(modelId: string): Promise<string[]> {
  return await invoke('get_keys_for_model', { modelId });
}

export async function getModelsForKey(keyId: string): Promise<string[]> {
  return await invoke('get_models_for_key', { keyId });
}

export async function associateKeyWithModel(keyId: string, modelId: string): Promise<void> {
  return await invoke('associate_key_with_model', { keyId, modelId });
}

export async function disassociateKeyFromModel(keyId: string, modelId: string): Promise<void> {
  return await invoke('disassociate_key_from_model', { keyId, modelId });
}

// ============================================================================
// Quota / Usage
// ============================================================================

export async function getKeyUsage(keyId: string, modelId: string): Promise<ModelUsageEntry | null> {
  return await invoke('get_key_usage', { keyId, modelId });
}

export async function getModelUsage(modelId: string): Promise<ModelUsageEntry[]> {
  return await invoke('get_model_usage', { modelId });
}

export async function getQuotaWindowStatus(): Promise<any[]> {
  return await invoke('get_quota_window_status');
}

export async function setAutoSwitch(enabled: boolean): Promise<void> {
  return await invoke('set_auto_switch_cmd', { enabled });
}

export async function getAutoSwitch(): Promise<boolean> {
  return await invoke('get_auto_switch_cmd');
}

// ============================================================================
// Proxy
// ============================================================================

export async function getProxyStatus(): Promise<{ running: boolean; port: number }> {
  return await invoke('get_proxy_status');
}

export async function startProxy(): Promise<void> {
  return await invoke('start_proxy');
}

export async function stopProxy(): Promise<void> {
  return await invoke('stop_proxy');
}

export async function recordApiCall(keyId: string, modelId: string, platformId: string): Promise<void> {
  return await invoke('record_api_call_cmd', { keyId, modelId, platformId });
}

export async function record429Error(keyId: string, modelId: string, platformId: string): Promise<void> {
  return await invoke('record_429_error_cmd', { keyId, modelId, platformId });
}

export async function record500Error(keyId: string, modelId: string, platformId: string): Promise<void> {
  return await invoke('record_500_error_cmd', { keyId, modelId, platformId });
}

export async function removeQuotaTracker(keyId: string): Promise<void> {
  return await invoke('remove_quota_tracker', { keyId });
}

export async function cleanExpiredDisabled(): Promise<string[]> {
  return await invoke('clean_expired_disabled_cmd');
}

/** Get the primary LAN IP address (for proxy host 0.0.0.0 display) */
export async function getLanIp(): Promise<string> {
  return await invoke('get_lan_ip');
}
