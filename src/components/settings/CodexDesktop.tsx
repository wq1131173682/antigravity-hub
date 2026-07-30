import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { usePlatformStore } from '../../stores/usePlatformStore';
import { Terminal, CheckCircle2, XCircle, Loader2, RefreshCw, AlertTriangle } from 'lucide-react';
import { showToast } from '../common/ToastContainer';
import { request as invoke } from '../../utils/request';

interface CodexStatus {
  installed: boolean;
  config_path: string | null;
  has_backup: boolean;
  version: string | null;
  message: string;
}

interface CodexApplyResult {
  success: boolean;
  catalog_path: string | null;
  config_path: string | null;
  message: string;
}

export default function CodexDesktop() {
  const { t } = useTranslation();
  const { platforms, models, fetchPlatforms, fetchModels } = usePlatformStore();
  const [status, setStatus] = useState<CodexStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [selectedPlatformId, setSelectedPlatformId] = useState('');
  const [selectedModel, setSelectedModel] = useState('');
  const [applying, setApplying] = useState(false);
  const [result, setResult] = useState<CodexApplyResult | null>(null);
  const [restoring, setRestoring] = useState(false);

  useEffect(() => {
    fetchPlatforms();
  }, []);

  useEffect(() => {
    if (selectedPlatformId) {
      fetchModels(selectedPlatformId);
    }
  }, [selectedPlatformId]);

  const platformModels = selectedPlatformId ? (models[selectedPlatformId] || []) : [];

  const handleCheckStatus = async () => {
    setChecking(true);
    setStatus(null);
    try {
      const result = await invoke<CodexStatus>('check_codex_status');
      setStatus(result);
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally {
      setChecking(false);
    }
  };

  const handleApply = async () => {
    if (!selectedPlatformId || !selectedModel) {
      showToast(t('codex_desktop.select_required'), 'error');
      return;
    }
    setApplying(true);
    setResult(null);
    try {
      const res = await invoke<CodexApplyResult>('apply_codex_config', {
        platform_id: selectedPlatformId,
        model_name: selectedModel,
      });
      setResult(res);
      showToast(t('common.success'), 'success');
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally {
      setApplying(false);
    }
  };

  const handleRestore = async () => {
    setRestoring(true);
    try {
      const msg = await invoke<string>('restore_codex_config');
      showToast(msg, 'success');
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally {
      setRestoring(false);
    }
  };

  return (
    <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
      <div className="flex items-center gap-2 mb-4">
        <Terminal className="w-4 h-4 text-orange-500" />
        <h2 className="font-semibold text-gray-900 dark:text-base-content">
          {t('codex_desktop.title')}
        </h2>
      </div>
      <p className="text-xs text-gray-400 dark:text-gray-500 mb-4">
        {t('codex_desktop.description')}
      </p>

      {/* Status Check */}
      <div className="mb-4">
        <button
          className="px-4 py-2 bg-gray-100 dark:bg-base-300 text-gray-700 dark:text-gray-300 text-sm rounded-lg hover:bg-gray-200 dark:hover:bg-base-200 transition-colors flex items-center gap-1.5"
          onClick={handleCheckStatus}
          disabled={checking}
        >
          {checking ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <RefreshCw className="w-3.5 h-3.5" />}
          {t('codex_desktop.check_status')}
        </button>
      </div>

      {/* Status Result */}
      {status && (
        <div className={`px-4 py-3 rounded-lg border mb-4 text-sm ${
          status.installed
            ? 'bg-green-50 dark:bg-green-900/20 border-green-100 dark:border-green-900/30 text-green-700 dark:text-green-300'
            : 'bg-amber-50 dark:bg-amber-900/20 border-amber-100 dark:border-amber-900/30 text-amber-700 dark:text-amber-300'
        }`}>
          <div className="flex items-center gap-2 mb-1">
            {status.installed
              ? <CheckCircle2 className="w-4 h-4 shrink-0" />
              : <AlertTriangle className="w-4 h-4 shrink-0" />
            }
            <span className="font-medium">
              {status.installed ? '✅ Codex Desktop 已安装' : '⚠️ 未检测到 Codex Desktop'}
            </span>
          </div>
          {status.version && (
            <p className="text-xs opacity-80 ml-6">版本: {status.version}</p>
          )}
          {status.config_path && (
            <p className="text-xs opacity-80 ml-6 break-all font-mono">{status.config_path}</p>
          )}
          {status.has_backup && (
            <p className="text-xs opacity-80 ml-6 mt-1">📦 有备份可恢复</p>
          )}
        </div>
      )}

      {/* Configuration Form */}
      <div className="space-y-3 mb-4">
        <div>
          <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
            {t('codex_desktop.select_platform')}
          </label>
          <select
            className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-orange-500 outline-none"
            value={selectedPlatformId}
            onChange={e => { setSelectedPlatformId(e.target.value); setSelectedModel(''); }}
          >
            <option value="">{t('codex_desktop.select_platform_hint')}</option>
            {platforms.map(p => (
              <option key={p.id} value={p.id}>{p.name} ({p.path_prefix})</option>
            ))}
          </select>
        </div>

        <div>
          <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
            {t('codex_desktop.default_model')}
          </label>
          <select
            className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-orange-500 outline-none"
            value={selectedModel}
            onChange={e => setSelectedModel(e.target.value)}
            disabled={!selectedPlatformId}
          >
            <option value="">{t('codex_desktop.select_model_hint')}</option>
            {platformModels.map(m => (
              <option key={m.id} value={m.model_name}>{m.display_name || m.model_name}</option>
            ))}
          </select>
        </div>

        {/* Apply Button */}
        <button
          className={`w-full px-4 py-2.5 text-sm font-medium text-white rounded-lg transition-colors flex items-center justify-center gap-2 ${
            applying
              ? 'bg-orange-400 cursor-not-allowed'
              : 'bg-orange-500 hover:bg-orange-600'
          }`}
          onClick={handleApply}
          disabled={applying || !selectedPlatformId || !selectedModel}
        >
          {applying ? <Loader2 className="w-4 h-4 animate-spin" /> : <Terminal className="w-4 h-4" />}
          {applying ? t('codex_desktop.applying') : t('codex_desktop.apply')}
        </button>
      </div>

      {/* Apply Result */}
      {result && (
        <div className={`px-4 py-3 rounded-lg border mb-4 ${
          result.success
            ? 'bg-green-50 dark:bg-green-900/20 border-green-100 dark:border-green-900/30'
            : 'bg-red-50 dark:bg-red-900/20 border-red-100 dark:border-red-900/30'
        }`}>
          <div className="flex items-center gap-2 mb-1">
            {result.success
              ? <CheckCircle2 className="w-4 h-4 text-green-500 shrink-0" />
              : <XCircle className="w-4 h-4 text-red-500 shrink-0" />
            }
            <span className={`text-sm font-medium ${
              result.success ? 'text-green-700 dark:text-green-400' : 'text-red-700 dark:text-red-400'
            }`}>
              {result.success ? '应用成功' : '应用失败'}
            </span>
          </div>
          <p className="text-xs text-green-600 dark:text-green-300 whitespace-pre-line mt-1">
            {result.message}
          </p>
        </div>
      )}

      {/* Restore Button */}
      {status?.has_backup && (
        <button
          className="w-full px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-base-300 rounded-lg transition-colors flex items-center justify-center gap-1.5"
          onClick={handleRestore}
          disabled={restoring}
        >
          {restoring ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <RefreshCw className="w-3.5 h-3.5" />}
          {t('codex_desktop.restore')}
        </button>
      )}
    </div>
  );
}
