import { request as invoke } from '../utils/request';

export interface CodexStatus {
  installed: boolean;
  config_path: string | null;
  has_backup: boolean;
  current_config_preview: string | null;
  is_antigravity_configured: boolean;
}

export interface ApplyResult {
  success: boolean;
  message: string;
  config_path: string;
  backup_path: string | null;
}

export interface EnvConflictResult {
  has_openai_api_key: boolean;
  has_openai_base_url: boolean;
  has_openai_org_id: boolean;
  has_codex_home: boolean;
  messages: string[];
}

export interface ApplyCodexConfigParams {
  proxyHost: string;
  proxyPort: number;
  pathPrefix: string;
  modelName: string;
  reasoningEffort?: string | null;
  disableResponseStorage?: boolean | null;
  apiKey?: string | null;
}

export async function checkCodexStatus(): Promise<CodexStatus> {
  return await invoke('check_codex_status');
}

export async function applyCodexConfig(params: ApplyCodexConfigParams): Promise<ApplyResult> {
  return await invoke('apply_codex_config', {
    proxyHost: params.proxyHost,
    proxyPort: params.proxyPort,
    pathPrefix: params.pathPrefix,
    modelName: params.modelName,
    reasoningEffort: params.reasoningEffort ?? null,
    disableResponseStorage: params.disableResponseStorage ?? null,
    apiKey: params.apiKey ?? null,
  });
}

export async function restoreCodexConfig(): Promise<ApplyResult> {
  return await invoke('restore_codex_config');
}

export async function clearCodexAuth(): Promise<ApplyResult> {
  return await invoke('clear_codex_auth');
}

export async function checkCodexEnvConflicts(): Promise<EnvConflictResult> {
  return await invoke('check_codex_env_conflicts');
}

// ── Codex Provider Profiles ──

export interface CodexProfile {
  id: string;
  name: string;
  platform_id: string;
  model_name: string;
  proxy_host: string;
  proxy_port: number;
  path_prefix: string;
  reasoning_effort: string | null;
  disable_response_storage: boolean | null;
  api_key: string | null;
  created_at: number;
  updated_at: number;
}

export interface SaveCodexProfileParams {
  id?: string | null;
  name: string;
  platform_id: string;
  model_name: string;
  proxy_host: string;
  proxy_port: number;
  path_prefix: string;
  reasoning_effort?: string | null;
  disable_response_storage?: boolean | null;
  api_key?: string | null;
}

export async function listCodexProfiles(): Promise<CodexProfile[]> {
  return await invoke('list_codex_profiles');
}

export async function saveCodexProfile(params: SaveCodexProfileParams): Promise<CodexProfile> {
  return await invoke('save_codex_profile', {
    id: params.id ?? null,
    name: params.name,
    platformId: params.platform_id,
    modelName: params.model_name,
    proxyHost: params.proxy_host,
    proxyPort: params.proxy_port,
    pathPrefix: params.path_prefix,
    reasoningEffort: params.reasoning_effort ?? null,
    disableResponseStorage: params.disable_response_storage ?? null,
    apiKey: params.api_key ?? null,
  });
}

export async function deleteCodexProfile(profileId: string): Promise<void> {
  return await invoke('delete_codex_profile', {
    profileId,
  });
}

export async function applyCodexProfile(profileId: string): Promise<ApplyResult> {
  return await invoke('apply_codex_profile', {
    profileId,
  });
}