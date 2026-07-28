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