import { useEffect, useMemo, useState, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { usePlatformStore } from '../stores/usePlatformStore';
import { useConfigStore } from '../stores/useConfigStore';
import { showToast } from '../components/common/ToastContainer';
import { Server, Globe, Key, Activity, AlertTriangle, RefreshCw, ArrowRight, Shield, Plus, Terminal, Power, PowerOff, Copy, Check } from 'lucide-react';
import { getLanIp } from '../services/platformService';
import { isTauri } from '../utils/env';

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

        {/* Model Overview with Quota Limits */}
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
            <div className="space-y-3">
              {platforms.map(p => {
                const platformModels = models[p.id] || [];
                if (platformModels.length === 0) return null;
                return (
                  <div key={p.id}>
                    <div className="flex items-center gap-2 mb-2">
                      <span className="text-xs font-semibold text-gray-700 dark:text-gray-300">{p.name}</span>
                      <span className="text-[10px] font-mono bg-gray-100 dark:bg-base-300 px-1.5 py-0.5 rounded text-gray-500 dark:text-gray-400">
                        /{p.path_prefix}
                      </span>
                      <span className="text-[10px] text-gray-400">{platformModels.length} models</span>
                    </div>
                    <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-2">
                      {platformModels.map(m => {
                        // Calculate total usage across all keys for this model
                        const usageEntries = modelUsage[m.id] || [];
                        const totalUsed5h = usageEntries.reduce((s, u) => s + (u.five_hour?.count || 0), 0);
                        const totalUsedDay = usageEntries.reduce((s, u) => s + (u.day?.count || 0), 0);
                        const totalUsedMonth = usageEntries.reduce((s, u) => s + (u.month?.count || 0), 0);
                        const usageLoaded = usageEntries.length > 0;

                        const quotas = [
                          { label: '5h', used: totalUsed5h, limit: m.per_5hour },
                          { label: 'Day', used: totalUsedDay, limit: m.per_day },
                          { label: 'Mon', used: totalUsedMonth, limit: m.per_month },
                        ].filter(q => q.limit > 0);

                        return (
                          <div key={m.id} className="bg-gray-50 dark:bg-base-200/50 rounded-lg p-3 border border-gray-100 dark:border-base-300">
                            <div className="text-xs font-medium text-gray-800 dark:text-gray-200 truncate mb-2" title={m.model_name}>
                              {m.display_name || m.model_name}
                            </div>
                            {quotas.length > 0 ? (
                              <div className="space-y-1.5 text-[11px]">
                                {quotas.map(q => {
                                  const pct = usageLoaded ? Math.min(100, Math.round((q.used / q.limit) * 100)) : 0;
                                  const barColor = pct >= 80 ? 'bg-red-500 dark:bg-red-400'
                                    : pct >= 50 ? 'bg-amber-400 dark:bg-amber-400'
                                    : 'bg-emerald-400 dark:bg-emerald-500';
                                  return (
                                    <div key={q.label}>
                                      <div className="flex justify-between mb-0.5">
                                        <span className="text-gray-400 dark:text-gray-500">{q.label}</span>
                                        <span className="text-gray-600 dark:text-gray-400 font-mono">
                                          {usageLoaded ? `${q.used}/` : ''}{formatLimit(q.limit)}
                                        </span>
                                      </div>
                                      <div className="h-1.5 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
                                        <div
                                          className={`h-full rounded-full transition-all duration-500 ${barColor}`}
                                          style={{ width: `${pct}%` }}
                                        />
                                      </div>
                                    </div>
                                  );
                                })}
                              </div>
                            ) : (
                              <span className="text-[11px] text-gray-400 dark:text-gray-500">No limits set</span>
                            )}
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
