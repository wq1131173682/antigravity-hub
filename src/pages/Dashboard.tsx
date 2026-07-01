import { useEffect, useMemo, useState, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { usePlatformStore } from '../stores/usePlatformStore';
import { useConfigStore } from '../stores/useConfigStore';
import { showToast } from '../components/common/ToastContainer';
import { Server, Globe, Key, Activity, AlertTriangle, RefreshCw, ArrowRight, Shield, Plus, Terminal, Power, PowerOff, Copy, Check, ArrowDownToLine, ArrowUpFromLine, Hash, RotateCcw } from 'lucide-react';
import { getLanIp, getTokenStats, resetTokenStats, TokenStats } from '../services/platformService';
import { isTauri } from '../utils/env';
import { MODEL_CONFIG } from '../config/modelConfig';

interface KeySwitchedPayload {
  platformId: string;
  platformName: string;
  modelName: string;
  disabledKeyId: string;
  nextKeyId: string;
  reason: string;
  disabledUntil: number;
}

function formatLimit(v: number): string {
  if (v <= 0) return '∞';
  if (v >= 10000) return `${(v / 1000).toFixed(1)}k`;
  return String(v);
}

/** Compact token counter (1234 → "1.2k", 1234567 → "1.2M"). */
function formatTokens(v: number): string {
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(2)}M`;
  if (v >= 10_000) return `${(v / 1_000).toFixed(1)}k`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(2)}k`;
  return String(v);
}

function Dashboard() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const {
    platforms, keys, models, modelUsage, proxyRunning, proxyPort,
    fetchPlatforms, fetchKeys, fetchModels, fetchModelUsage, fetchProxyStatus,
    startProxy, stopProxy
  } = usePlatformStore();
  const { config, loadConfig } = useConfigStore();
  const [starting, setStarting] = useState(false);
  const [lanIp, setLanIp] = useState('');
  const [copiedPath, setCopiedPath] = useState<string | null>(null);
  const [tokenStats, setTokenStats] = useState<TokenStats | null>(null);
  const [resettingStats, setResettingStats] = useState(false);
  const startTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Cleanup timeout on unmount
  useEffect(() => {
    return () => {
      if (startTimeoutRef.current) clearTimeout(startTimeoutRef.current);
    };
  }, []);

  useEffect(() => {
    fetchPlatforms();
    fetchProxyStatus();
    loadConfig();
  }, []);

  // Fetch keys and models for all platforms
  useEffect(() => {
    platforms.forEach(p => {
      fetchKeys(p.id);
      fetchModels(p.id);
    });
  }, [platforms.length]);

  // Fetch model usage for all models
  useEffect(() => {
    const allModelIds = Object.values(models).flat().map(m => m.id);
    const loadedIds = Object.keys(modelUsage);
    allModelIds.forEach(id => {
      if (!loadedIds.includes(id)) {
        fetchModelUsage(id);
      }
    });
  }, [models]);

  // Listen for key-switched events from the proxy backend
  useEffect(() => {
    if (!isTauri()) return;
    let unlistenFn: (() => void) | null = null;
    const setup = async () => {
      const { listen } = await import('@tauri-apps/api/event');
      unlistenFn = await listen<KeySwitchedPayload>('key-switched', (event) => {
        const { platformName, modelName, disabledKeyId, nextKeyId, reason } = event.payload;
        // Refresh model usage for the affected model
        const allModels = Object.values(models).flat();
        const matched = allModels.find(m => m.model_name === modelName);
        if (matched) fetchModelUsage(matched.id);
        // Show toast notification
        const shortDisabled = disabledKeyId.slice(0, 8);
        const shortNext = nextKeyId.slice(0, 8);
        const is429 = reason.includes('Rate limited');
        showToast(
          `${platformName} · ${modelName}：${is429 ? '429 限流' : '服务错误'}，Key ${shortDisabled}… 已切换至 ${shortNext}…`,
          'warning',
          5000
        );
      });
    };
    setup();
    return () => { if (unlistenFn) unlistenFn(); };
  }, [models, fetchModelUsage]);

  // Poll aggregate token stats every 3s. Counters are in-memory on the
  // backend, so this is the only way to keep the dashboard live without
  // re-emitting events from proxy.rs on every successful call.
  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      try {
        const s = await getTokenStats();
        if (!cancelled) setTokenStats(s);
      } catch {
        /* non-fatal: backend may not be running yet */
      }
    };
    refresh();
    const id = setInterval(refresh, 3000);
    return () => { cancelled = true; clearInterval(id); };
  }, []);

  const handleResetTokenStats = async () => {
    if (resettingStats) return;
    setResettingStats(true);
    try {
      await resetTokenStats();
      const fresh = await getTokenStats();
      setTokenStats(fresh);
    } catch (e) {
      console.error('Failed to reset token stats', e);
    } finally {
      setResettingStats(false);
    }
  };

  // Count keys across all platforms
  const allKeys = useMemo(() => {
    return Object.values(keys).flat();
  }, [keys]);

  const activeKeyCount = allKeys.filter(k => !k.disabled).length;
  const exhaustedKeyCount = allKeys.filter(k => k.disabled && k.disabled_reason?.includes('quota')).length;

  // Count models across all platforms
  const allModels = useMemo(() => {
    return Object.values(models).flat();
  }, [models]);

  // Detect LAN IP when proxy host is 0.0.0.0
  useEffect(() => {
    if (config?.proxy_host === '0.0.0.0') {
      getLanIp().then(setLanIp).catch(() => {});
    }
  }, [config?.proxy_host]);

  const proxyHost = config?.proxy_host || '127.0.0.1';
  const displayHost = proxyHost === '0.0.0.0' ? (lanIp || proxyHost) : proxyHost;

  const handleToggleProxy = async () => {
    setStarting(true);
    try {
      if (proxyRunning) {
        await stopProxy();
        showToast(t('common.success'), 'success');
      } else {
        const timeoutPromise = new Promise<never>((_, reject) => {
          startTimeoutRef.current = setTimeout(
            () => reject(new Error(t('dashboard.start_timeout') || 'start timeout, please check if the port is in use')),
            10000
          );
        });
        await Promise.race([startProxy(), timeoutPromise]);
        if (startTimeoutRef.current) clearTimeout(startTimeoutRef.current);
        showToast(t('common.success'), 'success');
      }
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally {
      setStarting(false);
      if (startTimeoutRef.current) {
        clearTimeout(startTimeoutRef.current);
        startTimeoutRef.current = null;
      }
      fetchProxyStatus();
    }
  };

  const handleRefresh = () => {
    fetchPlatforms();
    fetchProxyStatus();
    platforms.forEach(p => fetchModels(p.id));
    // Refresh quota usage for all models so the bars reflect latest data
    const allModelIds = Object.values(models).flat().map(m => m.id);
    allModelIds.forEach(id => fetchModelUsage(id));
    showToast(t('common.success'), 'success');
  };

  const copyAddress = (url?: string) => {
    const text = url || `http://${displayHost}:${proxyPort}`;
    navigator.clipboard.writeText(text);
    if (url) {
      setCopiedPath(url);
      setTimeout(() => setCopiedPath(null), 1500);
    }
    showToast(t('common.copied', '已复制'), 'success');
  };

  return (
    <div className="h-full w-full overflow-y-auto">
      <div className="p-5 sm:p-6 md:p-8 space-y-5 w-full">
        {/* Header */}
        <div className="flex justify-between items-center">
          <h1 className="text-2xl font-bold text-gray-900 dark:text-base-content">
            {t('dashboard.hello')}
          </h1>
          <div className="flex gap-2">
            <button
              className="px-3 py-1.5 bg-blue-500 text-white text-xs font-medium rounded-lg hover:bg-blue-600 transition-colors flex items-center gap-1.5 shadow-sm"
              onClick={handleRefresh}
            >
              <RefreshCw className="w-3.5 h-3.5" />
              <span className="hidden sm:inline">{t('dashboard.refresh_quota')}</span>
            </button>
          </div>
        </div>

        {/* Proxy Status - Full width at top */}
        <div className={`rounded-xl p-5 shadow-sm border-2 transition-all ${
          proxyRunning
            ? 'bg-gradient-to-br from-green-50 to-emerald-50 dark:from-green-950/40 dark:to-emerald-950/20 border-green-300 dark:border-green-700/60'
            : 'bg-white dark:bg-base-100 border-gray-200 dark:border-base-200'
        }`}>
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-4">
              <div className={`p-2.5 rounded-xl transition-all ${
                proxyRunning
                  ? 'bg-green-100 dark:bg-green-900/40 shadow-sm ring-1 ring-green-200 dark:ring-green-800/40'
                  : 'bg-gray-100 dark:bg-base-300'
              }`}>
                <Shield className={`w-5 h-5 transition-colors ${
                  proxyRunning ? 'text-green-600 dark:text-green-400' : 'text-gray-400 dark:text-gray-500'
                }`} />
              </div>
              <div>
                <div className="flex items-center gap-2 mb-0.5">
                  <span className="text-sm font-semibold text-gray-800 dark:text-gray-200">
                    {t('dashboard.proxy_status')}
                  </span>
                  {proxyRunning && (
                    <span className="text-[10px] font-semibold uppercase tracking-wider bg-green-100 dark:bg-green-900/40 text-green-700 dark:text-green-400 px-2 py-0.5 rounded-full border border-green-200 dark:border-green-800/40">
                      Live
                    </span>
                  )}
                </div>
                <div>
                  <div className="flex items-center gap-2">
                    {proxyRunning ? (
                      <>
                        <span className="relative flex h-2.5 w-2.5">
                          <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"></span>
                          <span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-green-500"></span>
                        </span>
                        <span className="text-xs font-semibold text-green-700 dark:text-green-400">{t('dashboard.proxy_running', '运行中', { port: proxyPort })}</span>
                        <code className="text-sm font-mono font-bold ml-2 px-2 py-0.5 rounded bg-green-50 dark:bg-green-950/40 text-green-700 dark:text-green-300">
                          http://{displayHost}:{proxyPort}
                        </code>
                        <button
                          onClick={() => copyAddress()}
                          className="p-1 rounded-md hover:bg-green-100 dark:hover:bg-green-900/30 text-green-500 hover:text-green-700 dark:hover:text-green-300 transition-colors"
                          title={t('common.copy', '复制')}
                        >
                          <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
                          </svg>
                        </button>
                      </>
                    ) : (
                      <>
                        <span className="inline-flex rounded-full h-2 w-2 bg-gray-400"></span>
                        <span className="text-xs font-medium text-gray-500 dark:text-gray-400">{t('dashboard.proxy_stopped', '已停止')}</span>
                      </>
                    )}
                  </div>
                  {/* Platform path mappings - clickable to copy per-platform URL */}
                  {proxyRunning && platforms.length > 0 && (
                    <div className="flex flex-wrap gap-x-1 gap-y-0.5 mt-1.5 ml-1">
                      {platforms.map(p => {
                        const fullUrl = `http://${displayHost}:${proxyPort}/${p.path_prefix}`;
                        const isCopied = copiedPath === fullUrl;
                        return (
                          <code
                            key={p.id}
                            onClick={() => copyAddress(fullUrl)}
                            className="group text-[11px] font-mono px-1.5 py-0.5 rounded bg-green-50/80 dark:bg-green-950/30 text-green-600 dark:text-green-400 border border-green-200/60 dark:border-green-800/30 cursor-pointer hover:bg-green-100 dark:hover:bg-green-950/50 hover:text-green-700 dark:hover:text-green-300 transition-all flex items-center gap-1"
                            title={fullUrl}
                          >
                            {isCopied ? (
                              <Check className="w-3 h-3" />
                            ) : (
                              <Copy className="w-3 h-3 opacity-0 group-hover:opacity-100 transition-opacity" />
                            )}
                            /{p.path_prefix}
                          </code>
                        );
                      })}
                    </div>
                  )}
                </div>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <button
                className={`relative inline-flex items-center gap-1.5 px-4 py-2 text-xs font-semibold rounded-lg transition-all select-none shadow-sm ${
                  proxyRunning
                    ? 'bg-red-50 text-red-600 hover:bg-red-100 dark:bg-red-950/50 dark:text-red-400 dark:hover:bg-red-950/70 border border-red-200 dark:border-red-800/50'
                    : 'bg-green-500 text-white hover:bg-green-600 dark:bg-green-600 dark:hover:bg-green-700 border border-green-500 dark:border-green-600'
                } ${starting ? 'opacity-60 cursor-not-allowed' : 'active:scale-95'}`}
                onClick={handleToggleProxy}
                disabled={starting}
              >
                {starting ? (
                  <RefreshCw className="w-3.5 h-3.5 animate-spin" />
                ) : proxyRunning ? (
                  <PowerOff className="w-3.5 h-3.5" />
                ) : (
                  <Power className="w-3.5 h-3.5" />
                )}
                {starting ? '...' : proxyRunning ? t('dashboard.stop_proxy') : t('dashboard.start_proxy')}
              </button>
            </div>
          </div>
        </div>

        {/* Stats Cards */}
        <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
          <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
            <div className="flex items-center justify-between mb-2">
              <div className="p-1.5 bg-blue-50 dark:bg-blue-900/20 rounded-md">
                <Server className="w-4 h-4 text-blue-500 dark:text-blue-400" />
              </div>
            </div>
            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{platforms.length}</div>
            <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.total_platforms')}</div>
          </div>

          <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
            <div className="flex items-center justify-between mb-2">
              <div className="p-1.5 bg-green-50 dark:bg-green-900/20 rounded-md">
                <Key className="w-4 h-4 text-green-500 dark:text-green-400" />
              </div>
            </div>
            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{allKeys.length}</div>
            <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.total_keys')}</div>
          </div>

          <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
            <div className="flex items-center justify-between mb-2">
              <div className="p-1.5 bg-cyan-50 dark:bg-cyan-900/20 rounded-md">
                <Activity className="w-4 h-4 text-cyan-500 dark:text-cyan-400" />
              </div>
            </div>
            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{activeKeyCount}</div>
            <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.active_keys')}</div>
          </div>

          <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
            <div className="flex items-center justify-between mb-2">
              <div className="p-1.5 bg-orange-50 dark:bg-orange-900/20 rounded-md">
                <AlertTriangle className="w-4 h-4 text-orange-500 dark:text-orange-400" />
              </div>
            </div>
            <div className="text-2xl font-bold text-gray-900 dark:text-base-content mb-0.5">{exhaustedKeyCount}</div>
            <div className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.usage_warning')}</div>
          </div>
        </div>

        {/* Token Usage — live in-memory session counters from the proxy. */}
        <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
          <div className="flex items-center justify-between mb-4">
            <div className="flex items-center gap-2">
              <div className="p-1.5 bg-violet-50 dark:bg-violet-900/20 rounded-md">
                <Activity className="w-4 h-4 text-violet-500 dark:text-violet-400" />
              </div>
              <h2 className="font-semibold text-gray-900 dark:text-base-content text-sm">
                Token 用量 / Token Usage
              </h2>
              <span className="text-[11px] text-gray-400 dark:text-gray-500">本次会话</span>
              {tokenStats && tokenStats.last_updated > 0 && (
                <span className="flex items-center gap-1 text-[10px] text-emerald-600 dark:text-emerald-400">
                  <span className="relative flex h-1.5 w-1.5">
                    <span className="absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75 animate-ping" />
                    <span className="relative inline-flex rounded-full h-1.5 w-1.5 bg-emerald-500" />
                  </span>
                  live
                </span>
              )}
            </div>
            <button
              className="text-[11px] px-2 py-1 rounded-md border border-gray-200 dark:border-base-300 text-gray-600 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-base-200 transition-colors flex items-center gap-1 disabled:opacity-50"
              onClick={handleResetTokenStats}
              disabled={resettingStats || !tokenStats || tokenStats.request_count === 0}
              title="清空当前会话的 token 统计"
            >
              <RotateCcw className={`w-3 h-3 ${resettingStats ? 'animate-spin' : ''}`} />
              Reset
            </button>
          </div>

          {tokenStats && tokenStats.request_count > 0 ? (
            <>
              {/* Top row: total + request count */}
              <div className="grid grid-cols-2 gap-3 mb-3">
                <div className="rounded-lg p-3 bg-gradient-to-br from-violet-50 to-fuchsia-50 dark:from-violet-900/20 dark:to-fuchsia-900/20 border border-violet-100 dark:border-violet-900/30">
                  <div className="text-[10px] uppercase tracking-wide text-violet-600 dark:text-violet-400 font-semibold mb-1">Total Tokens</div>
                  <div className="text-2xl font-bold text-gray-900 dark:text-base-content tabular-nums">
                    {formatTokens(tokenStats.total_tokens)}
                  </div>
                  <div className="text-[10px] text-gray-500 dark:text-gray-400 mt-0.5">prompt + completion</div>
                </div>
                <div className="rounded-lg p-3 bg-gray-50 dark:bg-base-200/50 border border-gray-100 dark:border-base-300">
                  <div className="text-[10px] uppercase tracking-wide text-gray-500 dark:text-gray-400 font-semibold mb-1">Requests</div>
                  <div className="text-2xl font-bold text-gray-900 dark:text-base-content tabular-nums">
                    {tokenStats.request_count.toLocaleString()}
                  </div>
                  <div className="text-[10px] text-gray-500 dark:text-gray-400 mt-0.5">
                    {tokenStats.streaming_request_count > 0
                      ? `含 ${tokenStats.streaming_request_count} 流式`
                      : '全部非流式'}
                  </div>
                </div>
              </div>

              {/* Bottom row: prompt vs completion breakdown bar */}
              <div>
                <div className="flex items-center justify-between text-[11px] mb-1.5">
                  <div className="flex items-center gap-1.5 text-blue-600 dark:text-blue-400">
                    <ArrowDownToLine className="w-3 h-3" />
                    <span className="font-medium">Prompt (input)</span>
                    <span className="font-mono tabular-nums text-gray-700 dark:text-gray-300">
                      {formatTokens(tokenStats.prompt_tokens)}
                    </span>
                  </div>
                  <div className="flex items-center gap-1.5 text-emerald-600 dark:text-emerald-400">
                    <span className="font-mono tabular-nums text-gray-700 dark:text-gray-300">
                      {formatTokens(tokenStats.completion_tokens)}
                    </span>
                    <span className="font-medium">Completion (output)</span>
                    <ArrowUpFromLine className="w-3 h-3" />
                  </div>
                </div>
                <div className="h-2 w-full rounded-full overflow-hidden bg-gray-100 dark:bg-base-300 flex">
                  {(() => {
                    const total = Math.max(1, tokenStats.total_tokens);
                    const promptPct = (tokenStats.prompt_tokens / total) * 100;
                    const completionPct = 100 - promptPct;
                    return (
                      <>
                        <div
                          className="h-full bg-gradient-to-r from-blue-400 to-blue-500 dark:from-blue-500 dark:to-blue-400 transition-all duration-500"
                          style={{ width: `${promptPct}%` }}
                          title={`Prompt: ${formatTokens(tokenStats.prompt_tokens)} (${promptPct.toFixed(1)}%)`}
                        />
                        <div
                          className="h-full bg-gradient-to-r from-emerald-400 to-emerald-500 dark:from-emerald-500 dark:to-emerald-400 transition-all duration-500"
                          style={{ width: `${completionPct}%` }}
                          title={`Completion: ${formatTokens(tokenStats.completion_tokens)} (${completionPct.toFixed(1)}%)`}
                        />
                      </>
                    );
                  })()}
                </div>
              </div>
            </>
          ) : (
            <div className="flex items-center gap-2 text-xs text-gray-400 dark:text-gray-500 py-3">
              <Hash className="w-3.5 h-3.5" />
              代理尚未处理任何请求 · proxy has not processed any request yet
            </div>
          )}
        </div>

        {/* Model Overview with Quota Limits — Redesigned */}
        {platforms.length > 0 && (
          <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
            <div className="flex items-center gap-2 mb-4">
              <div className="p-1.5 bg-cyan-50 dark:bg-cyan-900/20 rounded-md">
                <Activity className="w-4 h-4 text-cyan-500 dark:text-cyan-400" />
              </div>
              <h2 className="font-semibold text-gray-900 dark:text-base-content text-sm">
                模型与限额 / Models & Quotas
              </h2>
              <span className="text-[11px] text-gray-400 dark:text-gray-500">
                {allModels.length} models
              </span>
            </div>

            <div className="space-y-4">
              {platforms.map(p => {
                const platformModels = models[p.id] || [];
                if (platformModels.length === 0) return null;

                return (
                  <div key={p.id}>
                    {/* Platform divider */}
                    <div className="flex items-center gap-2 mb-3">
                      <Globe className="w-3.5 h-3.5 text-gray-400 dark:text-gray-500" />
                      <span className="text-xs font-semibold text-gray-700 dark:text-gray-300">{p.name}</span>
                      <span className="text-[10px] font-mono bg-gray-100 dark:bg-base-300 px-1.5 py-0.5 rounded text-gray-500 dark:text-gray-400">
                        /{p.path_prefix}
                      </span>
                      <span className="text-[10px] text-gray-400 dark:text-gray-500">{platformModels.length} models</span>
                    </div>

                    {/* Model cards grid */}
                    <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-2 gap-3">
                      {platformModels.map(m => {
                        const usageEntries = modelUsage[m.id] || [];
                        const usageLoaded = usageEntries.length > 0;
                        const availableCount = usageEntries.filter(u => u.is_available).length;
                        const totalCount = usageEntries.length;
                        const allExhausted = usageLoaded && availableCount === 0;

                        // Sort entries: available first, then exhausted/disabled
                        const sortedEntries = [...usageEntries].sort((a, b) => {
                          if (a.is_available && !b.is_available) return -1;
                          if (!a.is_available && b.is_available) return 1;
                          return 0;
                        });

                        // Get model icon from config
                        const modelConfig = MODEL_CONFIG[m.id.toLowerCase()];
                        const ModelIcon = modelConfig?.Icon;

                        return (
                          <div
                            key={m.id}
                            className="bg-gray-50 dark:bg-base-200/50 rounded-lg border border-gray-100 dark:border-base-300 overflow-hidden"
                          >
                            {/* Card header: icon + model name + availability badge */}
                            <div className="px-3 py-2.5 border-b border-gray-100/60 dark:border-base-300/60 bg-white/50 dark:bg-base-100/30">
                              <div className="flex items-center gap-2.5">
                                {/* Model icon */}
                                {ModelIcon ? (
                                  <ModelIcon size={18} className="flex-shrink-0" />
                                ) : (
                                  <div className="w-[18px] h-[18px] flex-shrink-0 rounded bg-gray-200 dark:bg-base-300 flex items-center justify-center">
                                    <span className="text-[9px] font-bold text-gray-500 dark:text-gray-400">M</span>
                                  </div>
                                )}
                                {/* Model name */}
                                <div className="flex-1 min-w-0">
                                  <div className="text-xs font-medium text-gray-800 dark:text-gray-200 truncate" title={m.model_name}>
                                    {m.display_name || m.model_name}
                                  </div>
                                </div>
                                {/* Availability badge */}
                                {usageLoaded ? (
                                  <div
                                    className={`text-[10px] font-mono px-2 py-0.5 rounded-full flex-shrink-0 ${
                                      allExhausted
                                        ? 'bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400'
                                        : availableCount < totalCount
                                          ? 'bg-amber-100 dark:bg-amber-900/30 text-amber-600 dark:text-amber-400'
                                          : 'bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-400'
                                    }`}
                                    title={`${availableCount} of ${totalCount} keys available`}
                                  >
                                    {availableCount}/{totalCount}
                                  </div>
                                ) : (
                                  <div className="text-[10px] text-gray-400 dark:text-gray-500 flex-shrink-0">…</div>
                                )}
                              </div>
                            </div>

                            {/* Key rows */}
                            <div className="px-3 py-2">
                              {usageLoaded ? (
                                <div className="space-y-2">
                                  {sortedEntries.map(u => {
                                    const over5h = m.per_5hour > 0 && u.five_hour.count > m.per_5hour;
                                    const overDay = m.per_day > 0 && u.day.count > m.per_day;
                                    const overMon = m.per_month > 0 && u.month.count > m.per_month;
                                    const isDisabled = !u.is_available;
                                    const overAny = over5h || overDay || overMon;

                                    const rowDotCls = isDisabled
                                      ? 'bg-red-500'
                                      : overAny
                                        ? 'bg-amber-500'
                                        : 'bg-emerald-500';

                                    const rowBgCls = isDisabled
                                      ? 'bg-red-50/40 dark:bg-red-900/10'
                                      : overAny
                                        ? 'bg-amber-50/30 dark:bg-amber-900/10'
                                        : 'bg-emerald-50/20 dark:bg-emerald-900/5';

                                    const ratioOf = (count: number, limit: number) => {
                                      if (limit <= 0) return 0;
                                      return Math.min(1, count / limit);
                                    };
                                    const r5h = ratioOf(u.five_hour.count, m.per_5hour);
                                    const rDay = ratioOf(u.day.count, m.per_day);
                                    const rMon = ratioOf(u.month.count, m.per_month);

                                    const barColor = (ratio: number, over: boolean) =>
                                      over ? 'bg-red-500' : ratio > 0.8 ? 'bg-amber-500' : 'bg-emerald-500';

                                    const disabledInfo = isDisabled && u.disabled_until
                                      ? `Disabled until ${new Date(u.disabled_until * 1000).toLocaleString()}`
                                      : isDisabled ? 'Disabled (5xx backoff)' : '';

                                    return (
                                      <div
                                        key={u.key_id}
                                        className={`rounded-md px-2 py-1.5 transition-colors ${rowBgCls}`}
                                        title={`
Key: ${u.key_id}
${disabledInfo ? disabledInfo + '\n' : ''}Status: ${isDisabled ? 'Disabled' : overAny ? 'Over quota' : 'Available'}
5h: ${u.five_hour.count}/${m.per_5hour <= 0 ? '∞' : m.per_5hour}
Day: ${u.day.count}/${m.per_day <= 0 ? '∞' : m.per_day}
Month: ${u.month.count}/${m.per_month <= 0 ? '∞' : m.per_month}
`.trim()}
                                      >
                                        {/* Key identifier row */}
                                        <div className="flex items-center gap-1.5 mb-1">
                                          <span className={`inline-block w-1.5 h-1.5 rounded-full flex-shrink-0 ${rowDotCls}`} />
                                          <span className="text-[11px] font-mono text-gray-600 dark:text-gray-400 truncate">
                                            {u.key_id.slice(0, 10)}…{u.key_id.slice(-4)}
                                          </span>
                                        </div>

                                        {/* Progress bars row */}
                                        <div className="grid grid-cols-3 gap-2">
                                          {/* 5h */}
                                          <div>
                                            <div className="flex items-baseline justify-between mb-0.5">
                                              <span className="text-[9px] text-gray-400 dark:text-gray-500">5h</span>
                                              <span className={`text-[10px] font-mono ${over5h ? 'text-red-600 dark:text-red-400 font-semibold' : 'text-gray-500 dark:text-gray-400'}`}>
                                                {u.five_hour.count}/{formatLimit(m.per_5hour)}
                                              </span>
                                            </div>
                                            <div className="w-full h-1 bg-gray-200/60 dark:bg-base-300/60 rounded-full overflow-hidden">
                                              <div
                                                className={`h-full transition-all ${barColor(r5h, over5h)}`}
                                                style={{ width: m.per_5hour > 0 ? `${r5h * 100}%` : '0%' }}
                                              />
                                            </div>
                                          </div>

                                          {/* Day */}
                                          <div>
                                            <div className="flex items-baseline justify-between mb-0.5">
                                              <span className="text-[9px] text-gray-400 dark:text-gray-500">Day</span>
                                              <span className={`text-[10px] font-mono ${overDay ? 'text-red-600 dark:text-red-400 font-semibold' : 'text-gray-500 dark:text-gray-400'}`}>
                                                {u.day.count}/{formatLimit(m.per_day)}
                                              </span>
                                            </div>
                                            <div className="w-full h-1 bg-gray-200/60 dark:bg-base-300/60 rounded-full overflow-hidden">
                                              <div
                                                className={`h-full transition-all ${barColor(rDay, overDay)}`}
                                                style={{ width: m.per_day > 0 ? `${rDay * 100}%` : '0%' }}
                                              />
                                            </div>
                                          </div>

                                          {/* Month */}
                                          <div>
                                            <div className="flex items-baseline justify-between mb-0.5">
                                              <span className="text-[9px] text-gray-400 dark:text-gray-500">Mon</span>
                                              <span className={`text-[10px] font-mono ${overMon ? 'text-red-600 dark:text-red-400 font-semibold' : 'text-gray-500 dark:text-gray-400'}`}>
                                                {u.month.count}/{formatLimit(m.per_month)}
                                              </span>
                                            </div>
                                            <div className="w-full h-1 bg-gray-200/60 dark:bg-base-300/60 rounded-full overflow-hidden">
                                              <div
                                                className={`h-full transition-all ${barColor(rMon, overMon)}`}
                                                style={{ width: m.per_month > 0 ? `${rMon * 100}%` : '0%' }}
                                              />
                                            </div>
                                          </div>
                                        </div>
                                      </div>
                                    );
                                  })}
                                </div>
                              ) : (
                                <div className="text-[11px] text-gray-400 dark:text-gray-500 py-2 text-center">
                                  {t('dashboard.no_usage_data', 'No usage data yet')}
                                </div>
                              )}
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {/* No Platforms Hint */}
        {platforms.length === 0 && (
          <div className="bg-white dark:bg-base-100 rounded-xl p-8 shadow-sm border border-gray-100 dark:border-base-200 text-center">
            <Server className="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
            <p className="text-gray-500 dark:text-gray-400 text-sm">{t('dashboard.no_platforms')}</p>
            <button
              className="mt-4 px-4 py-2 bg-blue-500 text-white text-sm rounded-lg hover:bg-blue-600 transition-colors"
              onClick={() => navigate('/accounts')}
            >
              <Plus className="w-4 h-4 inline mr-1" />
              {t('accounts.add_platform')}
            </button>
          </div>
        )}

        {/* Quick Actions */}
        <div className="grid grid-cols-1 gap-3">
          <button
            className="bg-indigo-50 dark:bg-indigo-900/20 rounded-lg p-3 shadow-sm border border-indigo-100 dark:border-indigo-900/30 hover:border-indigo-300 dark:hover:border-indigo-700 hover:shadow-md transition-all flex items-center justify-between group"
            onClick={() => navigate('/accounts')}
          >
            <span className="text-indigo-700 dark:text-indigo-300 font-medium text-sm">
              {t('dashboard.manage_keys')}
            </span>
            <ArrowRight className="w-4 h-4 text-indigo-400 dark:text-indigo-500 group-hover:text-indigo-600 dark:group-hover:text-indigo-300 group-hover:translate-x-1 transition-all" />
          </button>
        </div>

        {/* Quick Start Guide */}
        <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
          <div className="flex items-center gap-2 mb-4">
            <div className="p-1.5 bg-amber-50 dark:bg-amber-900/20 rounded-md">
              <Terminal className="w-4 h-4 text-amber-500 dark:text-amber-400" />
            </div>
            <h2 className="font-semibold text-gray-900 dark:text-base-content text-sm">
              快速使用
            </h2>
            <span className="text-[11px] text-gray-400 dark:text-gray-500 ml-1">Quick Start</span>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
            {/* Step 1 */}
            <div className="relative bg-gray-50 dark:bg-base-200/50 rounded-lg p-3.5 border border-gray-100 dark:border-base-300 hover:border-amber-200 dark:hover:border-amber-800/50 transition-colors group">
              <div className="flex items-start gap-3">
                <div className="flex-shrink-0 w-7 h-7 rounded-full bg-amber-100 dark:bg-amber-900/30 flex items-center justify-center text-xs font-bold text-amber-600 dark:text-amber-400 group-hover:scale-110 transition-transform">
                  1
                </div>
                <div className="min-w-0">
                  <div className="flex items-center gap-1.5 mb-1">
                    <Globe className="w-3.5 h-3.5 text-amber-500 dark:text-amber-400 flex-shrink-0" />
                    <span className="text-xs font-semibold text-gray-800 dark:text-gray-200">添加平台</span>
                  </div>
                  <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-relaxed">
                    在「账户管理」中添加你的 API 平台和 base URL
                  </p>
                </div>
              </div>
            </div>

            {/* Step 2 */}
            <div className="relative bg-gray-50 dark:bg-base-200/50 rounded-lg p-3.5 border border-gray-100 dark:border-base-300 hover:border-amber-200 dark:hover:border-amber-800/50 transition-colors group">
              <div className="flex items-start gap-3">
                <div className="flex-shrink-0 w-7 h-7 rounded-full bg-amber-100 dark:bg-amber-900/30 flex items-center justify-center text-xs font-bold text-amber-600 dark:text-amber-400 group-hover:scale-110 transition-transform">
                  2
                </div>
                <div className="min-w-0">
                  <div className="flex items-center gap-1.5 mb-1">
                    <Key className="w-3.5 h-3.5 text-amber-500 dark:text-amber-400 flex-shrink-0" />
                    <span className="text-xs font-semibold text-gray-800 dark:text-gray-200">添加 Key</span>
                  </div>
                  <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-relaxed">
                    在对应平台下添加你的 API Key，可添加多个实现自动切换
                  </p>
                </div>
              </div>
            </div>

            {/* Step 3 */}
            <div className="relative bg-gray-50 dark:bg-base-200/50 rounded-lg p-3.5 border border-gray-100 dark:border-base-300 hover:border-amber-200 dark:hover:border-amber-800/50 transition-colors group">
              <div className="flex items-start gap-3">
                <div className="flex-shrink-0 w-7 h-7 rounded-full bg-amber-100 dark:bg-amber-900/30 flex items-center justify-center text-xs font-bold text-amber-600 dark:text-amber-400 group-hover:scale-110 transition-transform">
                  3
                </div>
                <div className="min-w-0">
                  <div className="flex items-center gap-1.5 mb-1">
                    <Power className="w-3.5 h-3.5 text-amber-500 dark:text-amber-400 flex-shrink-0" />
                    <span className="text-xs font-semibold text-gray-800 dark:text-gray-200">启动代理</span>
                  </div>
                  <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-relaxed">
                    在「设置」中配置代理端口，然后返回首页点击「启动代理」
                  </p>
                </div>
              </div>
            </div>

            {/* Step 4 */}
            <div className="relative bg-gray-50 dark:bg-base-200/50 rounded-lg p-3.5 border border-gray-100 dark:border-base-300 hover:border-amber-200 dark:hover:border-amber-800/50 transition-colors group">
              <div className="flex items-start gap-3">
                <div className="flex-shrink-0 w-7 h-7 rounded-full bg-amber-100 dark:bg-amber-900/30 flex items-center justify-center text-xs font-bold text-amber-600 dark:text-amber-400 group-hover:scale-110 transition-transform">
                  4
                </div>
                <div className="min-w-0">
                  <div className="flex items-center gap-1.5 mb-1">
                    <Terminal className="w-3.5 h-3.5 text-amber-500 dark:text-amber-400 flex-shrink-0" />
                    <span className="text-xs font-semibold text-gray-800 dark:text-gray-200">开始使用</span>
                  </div>
                  <p className="text-[11px] text-gray-500 dark:text-gray-400 leading-relaxed">
                    在 API 客户端中配置代理地址到 <code className="text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/20 px-1 rounded text-[10px]">{displayHost}:{proxyPort}</code>
                  </p>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

export default Dashboard;
