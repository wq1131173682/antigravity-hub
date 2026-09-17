import { isTauri } from './env';

// Module-level variable to prevent duplicate auth error events
let lastAuthErrorTime = 0;

// 命令到 API 的映射
const COMMAND_MAPPING: Record<string, { url: string; method: 'GET' | 'POST' | 'DELETE' | 'PATCH' }> = {
  // Platform
  'list_platforms': { url: '/api/platforms', method: 'GET' },
  'add_platform': { url: '/api/platforms', method: 'POST' },
  'delete_platform': { url: '/api/platforms/:platformId', method: 'DELETE' },
  'list_keys': { url: '/api/platforms/:platformId/keys', method: 'GET' },
  'add_key': { url: '/api/platforms/:platformId/keys', method: 'POST' },
  'delete_key': { url: '/api/keys/:keyId', method: 'DELETE' },
  'set_key_status': { url: '/api/keys/:keyId/status', method: 'POST' },
  'update_key': { url: '/api/keys/:keyId', method: 'PATCH' },
  'enable_key': { url: '/api/keys/:keyId/enable', method: 'POST' },
  'disable_key': { url: '/api/keys/:keyId/disable', method: 'POST' },

  // Model
  'list_models': { url: '/api/platforms/:platformId/models', method: 'GET' },
  'add_model': { url: '/api/platforms/:platformId/models', method: 'POST' },
  'update_model': { url: '/api/models/:modelId', method: 'PATCH' },
  'delete_model': { url: '/api/models/:modelId', method: 'DELETE' },
  'refresh_models_from_upstream': { url: '/api/platforms/:platformId/models/refresh', method: 'POST' },

  // Key-Model Association
  'get_keys_for_model': { url: '/api/models/:modelId/keys', method: 'GET' },
  'get_models_for_key': { url: '/api/keys/:keyId/models', method: 'GET' },
  'associate_key_with_model': { url: '/api/key-models', method: 'POST' },
  'disassociate_key_from_model': { url: '/api/key-models', method: 'DELETE' },

  // Quota
  'record_api_call_cmd': { url: '/api/quota/record', method: 'POST' },
  'record_429_error_cmd': { url: '/api/quota/429', method: 'POST' },
  'record_500_error_cmd': { url: '/api/quota/500', method: 'POST' },
  'get_quota_window_status': { url: '/api/quota/status', method: 'GET' },
  'get_key_usage': { url: '/api/quota/key-usage', method: 'POST' },
  'get_model_usage': { url: '/api/quota/model-usage/:modelId', method: 'GET' },
  'set_auto_switch_cmd': { url: '/api/quota/auto-switch', method: 'POST' },
  'get_auto_switch_cmd': { url: '/api/quota/auto-switch', method: 'GET' },
  'remove_quota_tracker': { url: '/api/quota/tracker/:keyId', method: 'DELETE' },
  'clean_expired_disabled_cmd': { url: '/api/quota/clean-expired', method: 'POST' },

  // Proxy
  'get_proxy_status': { url: '/api/proxy/status', method: 'GET' },
  'start_proxy': { url: '/api/proxy/start', method: 'POST' },
  'stop_proxy': { url: '/api/proxy/stop', method: 'POST' },

  // Config
  'load_config': { url: '/api/config', method: 'GET' },
  'save_config': { url: '/api/config', method: 'POST' },
  'get_token_stats': { url: '/api/stats/token/summary', method: 'GET' },
  'get_token_stats_by_platform': { url: '/api/stats/token/by-platform', method: 'GET' },
  'reset_token_stats': { url: '/api/stats/token/clear', method: 'POST' },
  'check_for_updates': { url: '/api/system/updates/check', method: 'POST' },
};

export async function request<T>(cmd: string, args?: any): Promise<T> {
  // 1. Tauri 环境：直接使用 invoke ...
  if (isTauri()) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke<T>(cmd, args);
    } catch (error) {
      console.error(`Tauri Invoke Error [${cmd}]:`, error);
      throw error;
    }
  }

  // 2. Web 环境：映射到 HTTP API
  const mapping = COMMAND_MAPPING[cmd];
  if (!mapping) {
    console.error(`Command [${cmd}] is not yet mapped for Web mode. Failing.`);
    throw new Error(`Command [${cmd}] not supported in Web mode.`);
  }

  let url = mapping.url;
  // [FIX] 创建 args 副本，用于移除已使用的路径参数
  let bodyArgs = args ? { ...args } : undefined;

  // 通用路径参数处理：替换 :key 为 args[key]
  if (args) {
    Object.keys(args).forEach(key => {
      const placeholder = `:${key}`;
      if (url.includes(placeholder)) {
        url = url.replace(placeholder, encodeURIComponent(String(args[key])));
        // [FIX] 从 body 参数中移除已用于路径的参数
        if (bodyArgs) {
          delete bodyArgs[key];
        }
      }
    });
  }

  const apiKey = typeof window !== 'undefined' ? sessionStorage.getItem('abv_admin_api_key') : null;

  const options: RequestInit = {
    method: mapping.method,
    headers: {
      'Content-Type': 'application/json',
      ...(apiKey ? {
        'Authorization': `Bearer ${apiKey}`,
        'x-api-key': apiKey
      } : {}),
    },
  };

  if ((mapping.method === 'GET' || mapping.method === 'DELETE') && args) {
    const params = new URLSearchParams();
    Object.entries(args).forEach(([key, value]) => {
      // [FIX] 跳过已用于路径替换的参数
      if (url.includes(encodeURIComponent(String(value)))) return;
      if (value !== undefined && value !== null) {
        params.append(key, String(value));
      }
    });
    const qs = params.toString();
    if (qs) url += `?${qs}`;
  } else if ((mapping.method === 'POST' || mapping.method === 'PATCH') && bodyArgs) {
    // [FIX] 如果有 request 包装，提取其内容作为 body
    const body = bodyArgs.request !== undefined ? bodyArgs.request : bodyArgs;
    options.body = JSON.stringify(body);
  }

  try {
    const response = await fetch(url, options);
    if (!response.ok) {
      if (!isTauri() && response.status === 401) {
        // [FIX #1163] 增加防抖锁，避免重复事件导致 UI 抖动
        const now = Date.now();
        if (now - lastAuthErrorTime > 2000) {
          lastAuthErrorTime = now;
          window.dispatchEvent(new CustomEvent('abv-unauthorized'));
        }
      }
      const errorData = await response.json().catch(() => ({}));
      throw errorData.error || `HTTP Error ${response.status}`;
    }

    // 如果是 204 No Content，直接返回 null
    if (response.status === 204) {
      return null as unknown as T;
    }

    const text = await response.text();
    if (!text) {
      return null as unknown as T;
    }

    try {
      return JSON.parse(text) as T;
    } catch (e) {
      console.warn(`Failed to parse JSON response for [${cmd}]:`, text);
      return text as unknown as T; // Fallback for plain text responses
    }
  } catch (error) {
    console.error(`Web Fetch Error [${cmd}]:`, error);
    throw error;
  }
}