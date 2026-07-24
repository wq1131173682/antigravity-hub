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

export async function checkCodexStatus(): Promise<CodexStatus> {
  return await invoke('check_codex_status');
}

export async function applyCodexConfig(
  proxyHost: string,
  proxyPort: number,
  pathPrefix: string,
  modelName: string,
): Promise<ApplyResult> {
  return await invoke('apply_codex_config', {
    proxyHost,
    proxyPort,
    pathPrefix,
    modelName,
  });
}

export async function restoreCodexConfig(): Promise<ApplyResult> {
  return await invoke('restore_codex_config');
}