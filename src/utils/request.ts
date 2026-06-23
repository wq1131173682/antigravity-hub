// 探测环境
const isTauri = typeof window !== 'undefined' && (!!(window as any).__TAURI_INTERNALS__ || !!(window as any).__TAURI__);

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

  // Legacy account commands (kept for compatibility with remaining UI)
  'list_accounts': { url: '/api/accounts', method: 'GET' },
  'get_current_account': { url: '/api/accounts/current', method: 'GET' },
  'switch_account': { url: '/api/accounts/switch', method: 'POST' },
  'add_account': { url: '/api/accounts', method: 'POST' },
  'delete_account': { url: '/api/accounts/:accountId', method: 'DELETE' },
  'delete_accounts': { url: '/api/accounts/bulk-delete', method: 'POST' },
  'fetch_account_quota': { url: '/api/accounts/:accountId/quota', method: 'GET' },
  'refresh_account_quota': { url: '/api/accounts/:accountId/quota', method: 'GET' },
  'refresh_all_quotas': { url: '/api/accounts/refresh', method: 'POST' },
  'reorder_accounts': { url: '/api/accounts/reorder', method: 'POST' },
  'toggle_proxy_status': { url: '/api/accounts/:accountId/toggle-proxy', method: 'POST' },
  'warm_up_accounts': { url: '/api/accounts/warmup', method: 'POST' },
  'warm_up_all_accounts': { url: '/api/accounts/warmup', method: 'POST' },
  'warm_up_account': { url: '/api/accounts/:accountId/warmup', method: 'POST' },
  'update_account_label': { url: '/api/accounts/:accountId/label', method: 'POST' },
  'export_accounts': { url: '/api/accounts/export', method: 'POST' },
  'bind_device_profile': { url: '/api/accounts/:accountId/bind-device', method: 'POST' },
  'get_device_profiles': { url: '/api/accounts/:accountId/device-profiles', method: 'GET' },
  'list_device_versions': { url: '/api/accounts/:accountId/device-versions', method: 'GET' },
  'preview_generate_profile': { url: '/api/accounts/device-preview', method: 'POST' },
  'bind_device_profile_with_profile': { url: '/api/accounts/:accountId/bind-device-profile', method: 'POST' },
  'restore_original_device': { url: '/api/accounts/restore-original', method: 'POST' },
  'restore_device_version': { url: '/api/accounts/:accountId/device-versions/:versionId/restore', method: 'POST' },
  'delete_device_version': { url: '/api/accounts/:accountId/device-versions/:versionId', method: 'DELETE' },
  'open_device_folder': { url: '/api/system/open-folder', method: 'POST' },

  // Proxy Control (legacy aliases)
  'start_proxy_service': { url: '/api/proxy/start', method: 'POST' },
  'stop_proxy_service': { url: '/api/proxy/stop', method: 'POST' },

  // Logs & Monitoring
  'get_proxy_logs_filtered': { url: '/api/logs', method: 'GET' },
  'get_proxy_logs_count_filtered': { url: '/api/logs/count', method: 'GET' },
  'clear_proxy_logs': { url: '/api/logs/clear', method: 'POST' },
  'get_proxy_log_detail': { url: '/api/logs/:logId', method: 'GET' },

  // Debug Console
  'enable_debug_console': { url: '/api/debug/enable', method: 'POST' },
  'disable_debug_console': { url: '/api/debug/disable', method: 'POST' },
  'is_debug_console_enabled': { url: '/api/debug/enabled', method: 'GET' },
  'get_debug_console_logs': { url: '/api/debug/logs', method: 'GET' },
  'clear_debug_console_logs': { url: '/api/debug/logs/clear', method: 'POST' },

  // CLI Sync
  'get_cli_sync_status': { url: '/api/proxy/cli/status', method: 'POST' },
  'execute_cli_sync': { url: '/api/proxy/cli/sync', method: 'POST' },
  'execute_cli_restore': { url: '/api/proxy/cli/restore', method: 'POST' },
  'get_cli_config_content': { url: '/api/proxy/cli/config', method: 'POST' },

  // OpenCode Sync
  'get_opencode_sync_status': { url: '/api/proxy/opencode/status', method: 'POST' },
  'execute_opencode_sync': { url: '/api/proxy/opencode/sync', method: 'POST' },
  'execute_opencode_restore': { url: '/api/proxy/opencode/restore', method: 'POST' },
  'execute_opencode_clear': { url: '/api/proxy/opencode/clear', method: 'POST' },
  'get_opencode_config_content': { url: '/api/proxy/opencode/config', method: 'POST' },

  // Stats
  'get_token_stats_hourly': { url: '/api/stats/token/hourly', method: 'GET' },
  'get_token_stats_daily': { url: '/api/stats/token/daily', method: 'GET' },
  'get_token_stats_weekly': { url: '/api/stats/token/weekly', method: 'GET' },
  'get_token_stats_by_account': { url: '/api/stats/token/by-account', method: 'GET' },
  'get_token_stats_summary': { url: '/api/stats/token/summary', method: 'GET' },
  'get_token_stats_by_model': { url: '/api/stats/token/by-model', method: 'GET' },
  'get_token_stats_model_trend_hourly': { url: '/api/stats/token/model-trend/hourly', method: 'GET' },
  'get_token_stats_model_trend_daily': { url: '/api/stats/token/model-trend/daily', method: 'GET' },
  'get_token_stats_account_trend_hourly': { url: '/api/stats/token/account-trend/hourly', method: 'GET' },
  'get_token_stats_account_trend_daily': { url: '/api/stats/token/account-trend/daily', method: 'GET' },
  'clear_token_stats': { url: '/api/stats/token/clear', method: 'POST' },
  'get_token_stats': { url: '/api/stats/token/summary', method: 'GET' },
  'reset_token_stats': { url: '/api/stats/token/clear', method: 'POST' },

  // System
  'get_data_dir_path': { url: '/api/system/data-dir', method: 'GET' },
  'get_update_settings': { url: '/api/system/updates/settings', method: 'GET' },
  'save_update_settings': { url: '/api/system/updates/save', method: 'POST' },
  'is_auto_launch_enabled': { url: '/api/system/autostart/status', method: 'GET' },
  'toggle_auto_launch': { url: '/api/system/autostart/toggle', method: 'POST' },
  'get_http_api_settings': { url: '/api/system/http-api/settings', method: 'GET' },
  'save_http_api_settings': { url: '/api/system/http-api/settings', method: 'POST' },
  'get_antigravity_path': { url: '/api/system/antigravity/path', method: 'GET' },
  'get_antigravity_args': { url: '/api/system/antigravity/args', method: 'GET' },

  // Cloudflared
  'cloudflared_install': { url: '/api/proxy/cloudflared/install', method: 'POST' },
  'cloudflared_start': { url: '/api/proxy/cloudflared/start', method: 'POST' },
  'cloudflared_stop': { url: '/api/proxy/cloudflared/stop', method: 'POST' },
  'cloudflared_get_status': { url: '/api/proxy/cloudflared/status', method: 'GET' },

  // Updates
  'should_check_updates': { url: '/api/system/updates/check-status', method: 'GET' },
  'check_for_updates': { url: '/api/system/updates/check', method: 'POST' },
  'update_last_check_time': { url: '/api/system/updates/touch', method: 'POST' },

  // OAuth
  'prepare_oauth_url': { url: '/api/auth/url', method: 'GET' },
  'start_oauth_login': { url: '/api/accounts/oauth/start', method: 'POST' },
  'complete_oauth_login': { url: '/api/accounts/oauth/complete', method: 'POST' },
  'cancel_oauth_login': { url: '/api/accounts/oauth/cancel', method: 'POST' },
  'submit_oauth_code': { url: '/api/accounts/oauth/submit-code', method: 'POST' },
  'list_oauth_clients': { url: '/api/accounts/oauth/clients', method: 'GET' },
  'get_active_oauth_client': { url: '/api/accounts/oauth/client', method: 'GET' },
  'set_active_oauth_client': { url: '/api/accounts/oauth/client', method: 'POST' },

  // Import
  'import_v1_accounts': { url: '/api/accounts/import/v1', method: 'POST' },
  'import_from_db': { url: '/api/accounts/import/db', method: 'POST' },
  'import_custom_db': { url: '/api/accounts/import/db-custom', method: 'POST' },
  'sync_account_from_db': { url: '/api/accounts/sync/db', method: 'POST' },

  // System Extra & Cache
  'open_data_folder': { url: '/api/system/open-folder', method: 'POST' },
  'clear_antigravity_cache': { url: '/api/system/cache/clear', method: 'POST' },
  'get_antigravity_cache_paths': { url: '/api/system/cache/paths', method: 'GET' },
  'clear_log_cache': { url: '/api/system/logs/clear-cache', method: 'POST' },

  // Security / IP Management
  'get_ip_access_logs': { url: '/api/security/logs', method: 'GET' },
  'clear_ip_access_logs': { url: '/api/security/logs/clear', method: 'POST' },
  'get_ip_stats': { url: '/api/security/stats', method: 'GET' },
  'get_ip_token_stats': { url: '/api/security/token-stats', method: 'GET' },
  'get_ip_blacklist': { url: '/api/security/blacklist', method: 'GET' },
  'add_ip_to_blacklist': { url: '/api/security/blacklist', method: 'POST' },
  'remove_ip_from_blacklist': { url: '/api/security/blacklist', method: 'DELETE' },
  'clear_ip_blacklist': { url: '/api/security/blacklist/clear', method: 'POST' },
  'check_ip_in_blacklist': { url: '/api/security/blacklist/check', method: 'GET' },
  'get_ip_whitelist': { url: '/api/security/whitelist', method: 'GET' },
  'add_ip_to_whitelist': { url: '/api/security/whitelist', method: 'POST' },
  'remove_ip_from_whitelist': { url: '/api/security/whitelist', method: 'DELETE' },
  'clear_ip_whitelist': { url: '/api/security/whitelist/clear', method: 'POST' },
  'check_ip_in_whitelist': { url: '/api/security/whitelist/check', method: 'GET' },
  'get_security_config': { url: '/api/security/config', method: 'GET' },
  'update_security_config': { url: '/api/security/config', method: 'POST' },
  // User Tokens
  'list_user_tokens': { url: '/api/user-tokens', method: 'GET' },
  'get_user_token_summary': { url: '/api/user-tokens/summary', method: 'GET' },
  'create_user_token': { url: '/api/user-tokens', method: 'POST' },
  'renew_user_token': { url: '/api/user-tokens/:id/renew', method: 'POST' },
  'delete_user_token': { url: '/api/user-tokens/:id', method: 'DELETE' },
  'update_user_token': { url: '/api/user-tokens/:id', method: 'PATCH' },

  // Proxy Pool (Web Mode Fix)
  'get_proxy_pool_config': { url: '/api/proxy/pool/config', method: 'GET' },
  'get_all_account_bindings': { url: '/api/proxy/pool/bindings', method: 'GET' },
  'bind_account_proxy': { url: '/api/proxy/pool/bind', method: 'POST' },
  'unbind_account_proxy': { url: '/api/proxy/pool/unbind', method: 'POST' },
  'get_account_proxy_binding': { url: '/api/proxy/pool/binding/:accountId', method: 'GET' },
};

export async function request<T>(cmd: string, args?: any): Promise<T> {
  // 1. Tauri 环境：直接使用 invoke ...
  if (isTauri) {
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
      if (!isTauri && response.status === 401) {
        // [FIX #1163] 增加防抖锁，避免重复事件导致 UI 抖动
        const now = Date.now();
        const lastAuthError = (window as any)._lastAuthErrorTime || 0;
        if (now - lastAuthError > 2000) {
          (window as any)._lastAuthErrorTime = now;
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