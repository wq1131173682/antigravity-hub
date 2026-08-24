import { getAppVersion } from '../services/platformService';
import { useState, useEffect, useRef } from 'react';
import { Save, RefreshCw, Server, Globe, Shield, Download } from 'lucide-react';
import CodexIntegration from '../components/settings/CodexIntegration';
import { request as invoke } from '../utils/request';
import { useConfigStore } from '../stores/useConfigStore';
import { showToast } from '../components/common/ToastContainer';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { checkForUpdates } from '../services/platformService';

function Settings() {
  const { t, i18n } = useTranslation();
  const { config, loadConfig } = useConfigStore();
  const [portInput, setPortInput] = useState('8080');
  const [hostInput, setHostInput] = useState('127.0.0.1');
  const [autoSwitch, setAutoSwitch] = useState(true);
  const [proxyUrlInput, setProxyUrlInput] = useState('');
  const [saving, setSaving] = useState(false);
  const [portError, setPortError] = useState('');
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string>('');
  const [installing, setInstalling] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<{ version: string; current: string } | null>(null);
  const [updateRid, setUpdateRid] = useState<number | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    if (config) {
      setPortInput(String(config.proxy_port ?? 8080));
      setHostInput(config.proxy_host ?? '127.0.0.1');
      setAutoSwitch(config.auto_switch ?? true);
      setProxyUrlInput(config.upstream_proxy_url ?? '');
    }
  }, [config]);

  useEffect(() => {
    loadConfig();
    getAppVersion().then(setAppVersion).catch(() => {});
  }, []);

  const handleCheckUpdate = async () => {
    setCheckingUpdate(true);
    setUpdateStatus(null);
    try {
      await checkForUpdates();
      setUpdateStatus(t('settings.update.checking'));
    } catch (e) {
      setCheckingUpdate(false);
      setUpdateStatus(`${t('settings.update.check_failed')}: ${e}`);
    }
  };

  useEffect(() => {
    let cancelled = false;
    const setupListener = async () => {
      const unlisten = await listen<{
        status: string;
        version?: string;
        current_version?: string;
        message?: string;
        rid?: number;
      }>('update:check_result', (event) => {
        if (cancelled) return;
        const data = event.payload;
        if (data.status === 'available') {
          const v = data.version ?? '?';
          const cv = data.current_version ?? '?';
          setUpdateInfo({ version: v, current: cv });
          if (data.rid !== undefined) setUpdateRid(data.rid);
          setUpdateStatus(`${t('settings.update.new_version')}: ${v} (${cv})`);
        } else if (data.status === 'up_to_date') {
          setUpdateStatus(t('settings.update.current'));
        } else if (data.status === 'error') {
          setUpdateStatus(`${t('settings.update.check_failed')}: ${data.message ?? ''}`);
        } else {
          setUpdateStatus(t('settings.update.check_completed'));
        }
        setCheckingUpdate(false);
      });
      unlistenRef.current = unlisten;
    };
    setupListener();
    return () => {
      cancelled = true;
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
    };
  }, [t]);

  const handleInstallUpdate = async () => {
    if (updateRid === null) return;
    setInstalling(true);
    setUpdateStatus(null);
    try {
      // Call our backend wrapper which handles the install flow
      await tauriInvoke('install_update', { rid: updateRid });
      setUpdateStatus(t('update_notification.downloading'));
    } catch (e) {
      setUpdateStatus(`${t('settings.update.check_failed')}: ${e}`);
      setInstalling(false);
    }
  };

  const handleSave = async () => {
    const portNum = parseInt(portInput);
    if (isNaN(portNum) || portNum < 1024 || portNum > 65535) {
      setPortError(t('settings.proxy.port_invalid'));
      return;
    }
    setPortError('');

    setSaving(true);
    try {
      // Save port
      const confirmedPort = await invoke<number>('set_proxy_port', { port: portNum });
      setPortInput(String(confirmedPort));

      // Save host
      await invoke('set_proxy_host', { host: hostInput });

      // Save upstream proxy URL
      await invoke('set_upstream_proxy_url', { proxyUrl: proxyUrlInput.trim() || null });

      // Load updated config from backend
      loadConfig();
      showToast(t('common.success'), 'success');
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally {
      setSaving(false);
    }
  };

  const handleLanguageChange = (lang: string) => {
    i18n.changeLanguage(lang);
  };

  return (
    <div className="h-full w-full overflow-y-auto">
      <div className="p-5 space-y-4 max-w-3xl mx-auto">
        <h1 className="text-2xl font-bold text-gray-900 dark:text-base-content">
          {t('settings.title')}
        </h1>

        {/* Proxy Settings */}
        <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
          <div className="flex items-center gap-2 mb-4">
            <Server className="w-4 h-4 text-blue-500" />
            <h2 className="font-semibold text-gray-900 dark:text-base-content">
              {t('settings.proxy.title')}
            </h2>
          </div>
          <div className="space-y-4">
            {/* Proxy Host */}
            <div className="flex items-center justify-between">
              <div>
                <label className="text-sm text-gray-600 dark:text-gray-400">代理地址 / Host</label>
                <p className="text-xs text-gray-400 mt-0.5">监听地址，修改后重启代理生效</p>
              </div>
              <div className="flex flex-col items-end gap-1">
                <input
                  type="text"
                  className="w-36 px-3 py-1.5 text-sm border rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:outline-none focus:ring-2 focus:ring-blue-500 border-gray-200 dark:border-base-300 font-mono"
                  value={hostInput}
                  onChange={(e) => { setHostInput(e.target.value); }}
                  placeholder="127.0.0.1"
                />
              </div>
            </div>

            {/* Proxy Port */}
            <div className="flex items-center justify-between">
              <div>
                <label className="text-sm text-gray-600 dark:text-gray-400">{t('settings.proxy.port')}</label>
                <p className="text-xs text-gray-400 mt-0.5">{t('settings.proxy.port_desc')}</p>
              </div>
              <div className="flex flex-col items-end gap-1">
                <input
                  type="number"
                  className={`w-28 px-3 py-1.5 text-sm border rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:outline-none focus:ring-2 focus:ring-blue-500 ${
                    portError ? 'border-red-400 dark:border-red-500' : 'border-gray-200 dark:border-base-300'
                  }`}
                  value={portInput}
                  onChange={(e) => { setPortInput(e.target.value); setPortError(''); }}
                />
                {portError && <p className="text-xs text-red-500">{portError}</p>}
              </div>
            </div>

            {/* Auto Switch Keys */}
            <div className="flex items-center justify-between">
              <div>
                <label className="text-sm text-gray-600 dark:text-gray-400">{t('settings.proxy.auto_switch')}</label>
                <p className="text-xs text-gray-400 mt-0.5">{t('settings.proxy.auto_switch_desc')}</p>
              </div>
              <label className="relative inline-flex items-center cursor-pointer">
                <input
                  type="checkbox"
                  className="sr-only peer"
                  checked={autoSwitch}
                  onChange={(e) => setAutoSwitch(e.target.checked)}
                />
                <div className="w-9 h-5 bg-gray-200 dark:bg-base-300 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-blue-300 rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-blue-500" />
              </label>
            </div>
          </div>
        </div>

        {/* Upstream Proxy Settings */}
        <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
          <div className="flex items-center gap-2 mb-4">
            <Shield className="w-4 h-4 text-green-500" />
            <h2 className="font-semibold text-gray-900 dark:text-base-content">
              上游代理 / Upstream Proxy
            </h2>
          </div>
          <div className="space-y-4">
            {/* Upstream Proxy URL */}
            <div className="flex items-center justify-between">
              <div>
                <label className="text-sm text-gray-600 dark:text-gray-400">代理地址 / Proxy URL</label>
                <p className="text-xs text-gray-400 mt-0.5">
                  代理服务器发出的请求通过此地址转发。留空则直连，不需要重启代理，保存后立即生效
                </p>
              </div>
              <div className="flex flex-col items-end gap-1">
                <input
                  type="text"
                  className="w-52 px-3 py-1.5 text-sm border rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:outline-none focus:ring-2 focus:ring-blue-500 border-gray-200 dark:border-base-300 font-mono"
                  value={proxyUrlInput}
                  onChange={(e) => { setProxyUrlInput(e.target.value); }}
                  placeholder="http://127.0.0.1:7890"
                />
              </div>
            </div>
          </div>
        </div>

        {/* Language Settings */}
        <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
          <div className="flex items-center gap-2 mb-4">
            <Globe className="w-4 h-4 text-purple-500" />
            <h2 className="font-semibold text-gray-900 dark:text-base-content">
              {t('settings.general.language')}
            </h2>
          </div>
          <div className="flex gap-2">
            <button
              className={`px-4 py-2 text-sm rounded-lg border transition-colors ${
                i18n.language.startsWith('zh')
                  ? 'bg-blue-50 dark:bg-blue-900/20 border-blue-200 dark:border-blue-800 text-blue-700 dark:text-blue-300'
                  : 'bg-white dark:bg-base-200 border-gray-200 dark:border-base-300 text-gray-600 dark:text-gray-400 hover:border-gray-300'
              }`}
              onClick={() => handleLanguageChange('zh')}
            >
              中文
            </button>
            <button
              className={`px-4 py-2 text-sm rounded-lg border transition-colors ${
                i18n.language.startsWith('en')
                  ? 'bg-blue-50 dark:bg-blue-900/20 border-blue-200 dark:border-blue-800 text-blue-700 dark:text-blue-300'
                  : 'bg-white dark:bg-base-200 border-gray-200 dark:border-base-300 text-gray-600 dark:text-gray-400 hover:border-gray-300'
              }`}
              onClick={() => handleLanguageChange('en')}
            >
              English
            </button>
          </div>
        </div>

        {/* Codex CLI Integration */}
        <CodexIntegration />

        {/* Reload Config */}
        <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
          <div className="flex gap-2">
            <button
              className="px-4 py-2 bg-gray-100 dark:bg-base-300 text-gray-700 dark:text-gray-300 text-sm rounded-lg hover:bg-gray-200 dark:hover:bg-base-200 transition-colors flex items-center gap-1.5"
              onClick={loadConfig}
            >
              <RefreshCw className="w-3.5 h-3.5" />
              {t('common.refresh')}
            </button>
          </div>
        </div>

        {/* Check for Updates */}
        <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="font-semibold text-gray-900 dark:text-base-content">
                {t('settings.update.title')}
              </h2>
              <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
                {t('settings.update.description')}
              </p>
            </div>
            <div className="flex gap-2">
              <button
                className="px-4 py-2 bg-blue-500 text-white text-sm font-medium rounded-lg hover:bg-blue-600 transition-colors flex items-center gap-1.5 shadow-sm disabled:opacity-50 disabled:cursor-not-allowed"
                onClick={handleCheckUpdate}
                disabled={checkingUpdate || installing}
              >
                {checkingUpdate ? (
                  <RefreshCw className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <Download className="w-3.5 h-3.5" />
                )}
                {checkingUpdate ? t('settings.update.checking') : t('settings.update.check')}
              </button>
              {updateInfo && !installing && (
                <button
                  className="px-4 py-2 bg-green-600 text-white text-sm font-medium rounded-lg hover:bg-green-700 transition-colors flex items-center gap-1.5 shadow-sm"
                  onClick={handleInstallUpdate}
                >
                  {t('settings.about.check_update')}
                </button>
              )}
              {installing && (
                <span className="px-4 py-2 text-sm text-green-600 dark:text-green-400">
                  {t('settings.update.checking')}
                </span>
              )}
            </div>
          </div>
          {updateStatus && (
            <p className={`text-sm mt-3 ${updateStatus.includes('失败') || updateStatus.includes('failed') ? 'text-red-500' : 'text-green-600 dark:text-green-400'}`}>
              {updateStatus}
            </p>
          )}
        </div>

        {/* App Info */}
        <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
          <h2 className="font-semibold text-gray-900 dark:text-base-content mb-3">
            {t('settings.about.title')}
          </h2>
          <div className="text-sm text-gray-500 dark:text-gray-400 space-y-1">
            <p>{t('settings.about.version')}: {appVersion || '?'}</p>
            <p className="text-xs text-gray-400">{t('settings.update.auto_desc')}</p>
          </div>
        </div>

        {/* Save Button */}
        <div className="flex justify-end">
          <button
            className={`px-5 py-2 bg-blue-500 text-white text-sm font-medium rounded-lg hover:bg-blue-600 transition-colors flex items-center gap-1.5 shadow-sm ${
              saving ? 'opacity-70 cursor-not-allowed' : ''
            }`}
            onClick={handleSave}
            disabled={saving}
          >
            <Save className="w-3.5 h-3.5" />
            {saving ? t('common.saving') : t('common.save')}
          </button>
        </div>
      </div>
    </div>
  );
}

export default Settings;
