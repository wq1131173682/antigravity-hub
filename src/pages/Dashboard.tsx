import { useEffect, useMemo, useState, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { usePlatformStore } from '../stores/usePlatformStore';
import { useConfigStore } from '../stores/useConfigStore';
import { showToast } from '../components/common/ToastContainer';
import { Server, Globe, Key, Activity, Shield, RefreshCw, ArrowRight, Power, PowerOff, Copy } from 'lucide-react';
import { getLanIp } from '../services/platformService';

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
  const startTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    fetchPlatforms();
    fetchProxyStatus();
    loadConfig();
    return () => {
      if (startTimeoutRef.current) clearTimeout(startTimeoutRef.current);
    };
  }, []);

  useEffect(() => {
    platforms.forEach(p => { fetchKeys(p.id); fetchModels(p.id); });
  }, [platforms.length]);

  useEffect(() => {
    const allIds = Object.values(models).flat().map(m => m.id);
    const loadedIds = Object.keys(modelUsage);
    allIds.forEach(id => { if (!loadedIds.includes(id)) fetchModelUsage(id); });
  }, [models]);

  useEffect(() => {
    if (config?.proxy_host === '0.0.0.0') {
      getLanIp().then(setLanIp).catch(() => {});
    }
  }, [config?.proxy_host]);

  const allKeys = useMemo(() => Object.values(keys).flat(), [keys]);
  const activeKeyCount = allKeys.filter(k => !k.disabled).length;
  const allModels = useMemo(() => Object.values(models).flat(), [models]);

  const proxyHost = config?.proxy_host || '127.0.0.1';
  const displayHost = proxyHost === '0.0.0.0' ? (lanIp || proxyHost) : proxyHost;

  const handleToggleProxy = async () => {
    setStarting(true);
    try {
      if (proxyRunning) {
        await stopProxy();
        showToast(t('common.success'), 'success');
      } else {
        await Promise.race([
          startProxy(),
          new Promise<never>((_, reject) => {
            startTimeoutRef.current = setTimeout(() => reject(new Error(t('dashboard.start_timeout'))), 10000);
          })
        ]);
        showToast(t('common.success'), 'success');
      }
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally {
      setStarting(false);
      if (startTimeoutRef.current) clearTimeout(startTimeoutRef.current);
      fetchProxyStatus();
    }
  };

  return (
    <div className="h-full w-full overflow-y-auto">
      <div className="p-5 space-y-5 max-w-4xl mx-auto">
        {/* Header */}
        <div className="flex justify-between items-center">
          <h1 className="text-2xl font-bold text-gray-900 dark:text-base-content">{t('dashboard.hello')}</h1>
          <button className="px-3 py-1.5 bg-blue-500 text-white text-xs font-medium rounded-lg hover:bg-blue-600 transition-colors flex items-center gap-1.5 shadow-sm" onClick={() => { fetchPlatforms(); fetchProxyStatus(); showToast(t('common.success'), 'success'); }}>
            <RefreshCw className="w-3.5 h-3.5" />
            <span className="hidden sm:inline">{t('dashboard.refresh_quota')}</span>
          </button>
        </div>

        {/* Proxy Status */}
        <div className={`rounded-xl p-4 shadow-sm border-2 transition-all ${
          proxyRunning ? 'bg-gradient-to-r from-green-50 to-emerald-50 dark:from-green-950/30 dark:to-emerald-950/20 border-green-300 dark:border-green-700/50' : 'bg-white dark:bg-base-100 border-gray-200 dark:border-base-200'
        }`}>
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className={`p-2 rounded-lg ${proxyRunning ? 'bg-green-100 dark:bg-green-900/40' : 'bg-gray-100 dark:bg-base-300'}`}>
                <Shield className={`w-5 h-5 ${proxyRunning ? 'text-green-600 dark:text-green-400' : 'text-gray-400'}`} />
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <span className="text-sm font-semibold text-gray-800 dark:text-gray-200">{t('dashboard.proxy_status')}</span>
                  {proxyRunning && <span className="text-[10px] font-semibold uppercase bg-green-100 dark:bg-green-900/40 text-green-700 dark:text-green-400 px-2 py-0.5 rounded-full">Live</span>}
                </div>
                <div className="flex items-center gap-2 mt-0.5">
                  {proxyRunning ? (
                    <>
                      <span className="relative flex h-2 w-2">
                        <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"></span>
                        <span className="relative inline-flex rounded-full h-2 w-2 bg-green-500"></span>
                      </span>
                      <code className="text-sm font-mono font-bold text-green-700 dark:text-green-300">http://{displayHost}:{proxyPort}</code>
                      <button onClick={() => { navigator.clipboard.writeText(`http://${displayHost}:${proxyPort}`); showToast(t('common.copied'), 'success'); }} className="p-1 hover:bg-green-100 dark:hover:bg-green-900/30 rounded text-green-500">
                        <Copy className="w-3.5 h-3.5" />
                      </button>
                    </>
                  ) : (
                    <><span className="inline-flex rounded-full h-2 w-2 bg-gray-400"></span><span className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.proxy_stopped')}</span></>
                  )}
                </div>
              </div>
            </div>
            <button
              className={`px-4 py-2 text-xs font-semibold rounded-lg transition-all flex items-center gap-1.5 ${proxyRunning ? 'bg-red-50 text-red-600 hover:bg-red-100 dark:bg-red-950/50 dark:text-red-400 dark:hover:bg-red-950/70 border border-red-200 dark:border-red-800/50' : 'bg-green-500 text-white hover:bg-green-600'}`}
              onClick={handleToggleProxy}
              disabled={starting}
            >
              {starting ? <RefreshCw className="w-3.5 h-3.5 animate-spin" /> : proxyRunning ? <PowerOff className="w-3.5 h-3.5" /> : <Power className="w-3.5 h-3.5" />}
              {starting ? '...' : proxyRunning ? t('dashboard.stop_proxy') : t('dashboard.start_proxy')}
            </button>
          </div>
          {/* Platform prefixes */}
          {proxyRunning && platforms.length > 0 && (
            <div className="flex flex-wrap gap-1 mt-2 ml-1">
              {platforms.map(p => (
                <code key={p.id} className="text-[11px] font-mono px-1.5 py-0.5 rounded bg-green-50/80 dark:bg-green-950/30 text-green-600 dark:text-green-400 border border-green-200/60 dark:border-green-800/30">
                  /{p.path_prefix}
                </code>
              ))}
            </div>
          )}
        </div>

        {/* Stats Row */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
            <div className="flex items-center gap-2 mb-2">
              <Server className="w-4 h-4 text-blue-500" />
              <span className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.total_platforms')}</span>
            </div>
            <div className="text-2xl font-bold text-gray-900 dark:text-base-content">{platforms.length}</div>
          </div>
          <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
            <div className="flex items-center gap-2 mb-2">
              <Key className="w-4 h-4 text-green-500" />
              <span className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.total_keys')}</span>
            </div>
            <div className="text-2xl font-bold text-gray-900 dark:text-base-content">{allKeys.length}</div>
          </div>
          <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
            <div className="flex items-center gap-2 mb-2">
              <Activity className="w-4 h-4 text-cyan-500" />
              <span className="text-xs text-gray-500 dark:text-gray-400">{t('dashboard.active_keys')}</span>
            </div>
            <div className="text-2xl font-bold text-gray-900 dark:text-base-content">{activeKeyCount}</div>
          </div>
          <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
            <div className="flex items-center gap-2 mb-2">
              <Globe className="w-4 h-4 text-purple-500" />
              <span className="text-xs text-gray-500 dark:text-gray-400">{t('accounts.models')}</span>
            </div>
            <div className="text-2xl font-bold text-gray-900 dark:text-base-content">{allModels.length}</div>
          </div>
        </div>

        {/* Platform Overview — simple card per platform */}
        {platforms.length > 0 && (
          <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
            <h2 className="font-semibold text-gray-900 dark:text-base-content text-sm mb-4">{t('dashboard.platforms_title')}</h2>
            <div className="space-y-3">
              {platforms.map(p => {
                const platformModels = models[p.id] || [];
                return (
                  <div key={p.id} className="bg-gray-50 dark:bg-base-200/50 rounded-lg p-3 border border-gray-100 dark:border-base-300">
                    <div className="flex items-center justify-between mb-2">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-gray-800 dark:text-gray-200">{p.name}</span>
                        <span className="text-[10px] font-mono bg-gray-200 dark:bg-base-300 px-1.5 py-0.5 rounded text-gray-500 dark:text-gray-400">/{p.path_prefix}</span>
                      </div>
                      <span className="text-xs text-gray-400">{platformModels.length} {t('dashboard.model_count')}</span>
                    </div>
                    <div className="flex flex-wrap gap-1.5">
                      {platformModels.map(m => {
                        const usage = modelUsage[m.id] || [];
                        const available = usage.filter(u => u.is_available).length;
                        const total = usage.length;
                        return (
                          <span key={m.id} className="inline-flex items-center gap-1 px-2 py-0.5 text-[10px] bg-white dark:bg-base-100 rounded border border-gray-200 dark:border-base-300">
                            <span className={`inline-block w-1.5 h-1.5 rounded-full ${total === 0 ? 'bg-gray-300' : available === 0 ? 'bg-red-500' : available < total ? 'bg-amber-500' : 'bg-emerald-500'}`} />
                            {m.display_name || m.model_name}
                            {total > 0 && <span className="text-gray-400 ml-0.5">({available}/{total})</span>}
                          </span>
                        );
                      })}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {/* Empty State */}
        {platforms.length === 0 && (
          <div className="bg-white dark:bg-base-100 rounded-xl p-10 shadow-sm border-2 border-dashed border-gray-200 dark:border-base-300 text-center">
            <Server className="w-10 h-10 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
            <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">{t('dashboard.no_platforms')}</p>
            <button className="px-4 py-2 bg-blue-500 text-white text-sm rounded-lg hover:bg-blue-600 transition-colors" onClick={() => navigate('/accounts')}>
              {t('accounts.add_platform')}
            </button>
          </div>
        )}

        {/* Quick Actions */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <button className="bg-indigo-50 dark:bg-indigo-900/20 rounded-lg p-3 border border-indigo-100 dark:border-indigo-900/30 hover:border-indigo-300 hover:shadow-md transition-all flex items-center justify-between group" onClick={() => navigate('/accounts')}>
            <span className="text-indigo-700 dark:text-indigo-300 font-medium text-sm">{t('dashboard.manage_keys')}</span>
            <ArrowRight className="w-4 h-4 text-indigo-400 group-hover:translate-x-1 transition-all" />
          </button>
          <button className="bg-purple-50 dark:bg-purple-900/20 rounded-lg p-3 border border-purple-100 dark:border-purple-900/30 hover:border-purple-300 hover:shadow-md transition-all flex items-center justify-between group" onClick={() => navigate('/settings')}>
            <span className="text-purple-700 dark:text-purple-300 font-medium text-sm">{t('settings.title')}</span>
            <ArrowRight className="w-4 h-4 text-purple-400 group-hover:translate-x-1 transition-all" />
          </button>
        </div>
      </div>
    </div>
  );
}

export default Dashboard;
