import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useConfigStore } from '../../stores/useConfigStore';
import { usePlatformStore } from '../../stores/usePlatformStore';
import * as codexService from '../../services/codexService';
import { showToast } from '../common/ToastContainer';
import { Terminal, CheckCircle2, XCircle, AlertTriangle, RotateCcw, Loader2, Play, FileCode } from 'lucide-react';

type StatusType = 'idle' | 'checking' | 'ready' | 'applying' | 'success' | 'error';

export default function CodexIntegration() {
  const { t } = useTranslation();
  const { config } = useConfigStore();
  const { platforms, models, fetchPlatforms, fetchModels } = usePlatformStore();

  const [status, setStatus] = useState<StatusType>('idle');
  const [codexStatus, setCodexStatus] = useState<codexService.CodexStatus | null>(null);
  const [selectedPlatformId, setSelectedPlatformId] = useState<string>('');
  const [selectedModelName, setSelectedModelName] = useState<string>('');
  const [autoDetected, setAutoDetected] = useState(false);
  const [result, setResult] = useState<codexService.ApplyResult | null>(null);

  // Initialize
  useEffect(() => {
    fetchPlatforms();
  }, []);

  // Auto-detect when platforms load
  useEffect(() => {
    if (platforms.length > 0 && !selectedPlatformId) {
      setSelectedPlatformId(platforms[0].id);
    }
  }, [platforms]);

  // Fetch models when platform changes
  useEffect(() => {
    if (selectedPlatformId) {
      fetchModels(selectedPlatformId).then(() => {
        const currentModels = usePlatformStore.getState().models[selectedPlatformId];
        if (currentModels && currentModels.length > 0 && !autoDetected) {
          setSelectedModelName(currentModels[0].model_name);
          setAutoDetected(true);
        }
      });
    }
  }, [selectedPlatformId]);

  const handleCheck = useCallback(async () => {
    setStatus('checking');
    try {
      const cs = await codexService.checkCodexStatus();
      setCodexStatus(cs);
      setStatus('ready');
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
      setStatus('error');
    }
  }, [t]);

  const handleApply = useCallback(async () => {
    if (!selectedModelName || !config) return;

    const selectedPlatform = platforms.find(p => p.id === selectedPlatformId);
    const pathPrefix = selectedPlatform?.path_prefix || 'openai';

    setStatus('applying');
    setResult(null);
    try {
      const res = await codexService.applyCodexConfig(
        config.proxy_host || '127.0.0.1',
        config.proxy_port || 8045,
        pathPrefix,
        selectedModelName,
      );
      setResult(res);
      setStatus('success');
      showToast(t('codex.apply_success'), 'success');

      const cs = await codexService.checkCodexStatus();
      setCodexStatus(cs);
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
      setStatus('error');
    }
  }, [selectedModelName, config, platforms, selectedPlatformId, t]);

  const handleRestore = useCallback(async () => {
    setStatus('applying');
    try {
      const res = await codexService.restoreCodexConfig();
      setResult(res);
      setStatus('success');
      showToast(t('codex.restore_success'), 'success');

      const cs = await codexService.checkCodexStatus();
      setCodexStatus(cs);
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
      setStatus('error');
    }
  }, [t]);

  const platformModels = selectedPlatformId ? (models[selectedPlatformId] || []) : [];

  return (
    <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
      <div className="flex items-center gap-2 mb-4">
        <Terminal className="w-4 h-4 text-emerald-500" />
        <h2 className="font-semibold text-gray-900 dark:text-base-content">
          {t('codex.title')}
        </h2>
      </div>

      {/* Step 1: Check Codex CLI Status */}
      {status === 'idle' && (
        <div className="space-y-4">
          <p className="text-sm text-gray-500 dark:text-gray-400">
            {t('codex.description')}
          </p>
          <button
            className="px-4 py-2 bg-emerald-500 text-white text-sm font-medium rounded-lg hover:bg-emerald-600 transition-colors flex items-center gap-1.5 shadow-sm"
            onClick={handleCheck}
          >
            <Play className="w-3.5 h-3.5" />
            {t('codex.check_status')}
          </button>
        </div>
      )}

      {/* Checking */}
      {status === 'checking' && (
        <div className="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400">
          <Loader2 className="w-4 h-4 animate-spin text-emerald-500" />
          {t('codex.checking')}
        </div>
      )}

      {/* Ready: Show status + apply form */}
      {(status === 'ready' || status === 'success') && codexStatus && (
        <div className="space-y-4">
          {/* Installation Status */}
          <div className="flex items-center gap-2">
            {codexStatus.installed ? (
              <CheckCircle2 className="w-4 h-4 text-emerald-500" />
            ) : (
              <XCircle className="w-4 h-4 text-gray-400" />
            )}
            <span className="text-sm text-gray-700 dark:text-gray-300">
              {codexStatus.installed
                ? t('codex.installed')
                : t('codex.not_installed')}
            </span>
          </div>

          {/* Config path */}
          {codexStatus.config_path && (
            <div className="text-xs text-gray-400 dark:text-gray-500 font-mono bg-gray-50 dark:bg-base-300 px-3 py-2 rounded-lg truncate">
              {codexStatus.config_path}
            </div>
          )}

          {/* Current config status */}
          {codexStatus.is_antigravity_configured && (
            <div className="flex items-center gap-1.5 text-xs text-emerald-600 dark:text-emerald-400 bg-emerald-50 dark:bg-emerald-900/20 px-3 py-2 rounded-lg">
              <CheckCircle2 className="w-3.5 h-3.5" />
              {t('codex.already_configured')}
            </div>
          )}

          {/* Has backup */}
          {codexStatus.has_backup && (
            <div className="flex items-center gap-1.5 text-xs text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/20 px-3 py-2 rounded-lg">
              <AlertTriangle className="w-3.5 h-3.5" />
              {t('codex.has_backup')}
            </div>
          )}

          {/* Platform & Model Selection */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div>
              <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5">
                {t('codex.select_platform')}
              </label>
              <select
                className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-emerald-500 outline-none"
                value={selectedPlatformId}
                onChange={(e) => { setSelectedPlatformId(e.target.value); setAutoDetected(false); }}
              >
                {platforms.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5">
                {t('codex.select_model')}
              </label>
              <select
                className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-emerald-500 outline-none"
                value={selectedModelName}
                onChange={(e) => setSelectedModelName(e.target.value)}
              >
                {platformModels.length === 0 && (
                  <option value="" disabled>{t('codex.no_models')}</option>
                )}
                {platformModels.map((m) => (
                  <option key={m.id} value={m.model_name}>
                    {m.display_name || m.model_name}
                  </option>
                ))}
              </select>
            </div>
          </div>

          {/* Config Preview */}
          <div className="bg-gray-50 dark:bg-base-300 rounded-lg p-3 border border-gray-100 dark:border-base-200">
            <div className="flex items-center gap-1.5 mb-2">
              <FileCode className="w-3.5 h-3.5 text-gray-400" />
              <span className="text-xs font-medium text-gray-500 dark:text-gray-400">
                {t('codex.config_preview')}
              </span>
            </div>
            <pre className="text-xs font-mono text-gray-600 dark:text-gray-400 leading-relaxed whitespace-pre-wrap">
{`model = "${selectedModelName || '<model>'}"
model_provider = "${(platforms.find(p => p.id === selectedPlatformId)?.path_prefix) || 'openai'}"
preferred_auth_method = "apikey"

[model_providers.${(platforms.find(p => p.id === selectedPlatformId)?.path_prefix) || 'openai'}]
name = "${(platforms.find(p => p.id === selectedPlatformId)?.path_prefix) || 'openai'}"
base_url = "http://${config?.proxy_host || '127.0.0.1'}:${config?.proxy_port || 8045}/${(platforms.find(p => p.id === selectedPlatformId)?.path_prefix) || 'openai'}/v1"
wire_api = "responses"`}
            </pre>
          </div>

          {/* Action Buttons */}
          <div className="flex flex-wrap gap-2">
            <button
              className={`px-4 py-2 text-sm font-medium rounded-lg transition-colors flex items-center gap-1.5 shadow-sm ${
                !selectedModelName
                  ? 'bg-gray-300 dark:bg-base-300 text-gray-500 cursor-not-allowed'
                  : 'bg-emerald-500 text-white hover:bg-emerald-600'
              }`}
              onClick={handleApply}
              disabled={!selectedModelName}
            >
              <Terminal className="w-3.5 h-3.5" />
              {t('codex.apply')}
            </button>

            {codexStatus.has_backup && (
              <button
                className="px-4 py-2 text-sm font-medium rounded-lg bg-amber-50 dark:bg-amber-900/20 text-amber-600 dark:text-amber-400 border border-amber-200 dark:border-amber-800 hover:bg-amber-100 dark:hover:bg-amber-900/30 transition-colors flex items-center gap-1.5"
                onClick={handleRestore}
              >
                <RotateCcw className="w-3.5 h-3.5" />
                {t('codex.restore')}
              </button>
            )}

            <button
              className="px-4 py-2 text-sm font-medium rounded-lg bg-gray-100 dark:bg-base-300 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-base-200 transition-colors flex items-center gap-1.5"
              onClick={handleCheck}
            >
              <Loader2 className="w-3.5 h-3.5" />
              {t('common.refresh')}
            </button>
          </div>

          {/* Result message */}
          {result && (
            <div className={`p-3 rounded-lg text-sm ${
              result.success
                ? 'bg-emerald-50 dark:bg-emerald-900/20 text-emerald-700 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-800'
                : 'bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-300 border border-red-200 dark:border-red-800'
            }`}>
              <pre className="whitespace-pre-wrap font-sans text-xs">{result.message}</pre>
            </div>
          )}
        </div>
      )}

      {/* Error */}
      {status === 'error' && (
        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-2 text-sm text-red-600 dark:text-red-400">
            <XCircle className="w-4 h-4" />
            {t('codex.check_failed')}
          </div>
          <button
            className="px-4 py-2 text-sm font-medium rounded-lg bg-gray-100 dark:bg-base-300 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-base-200 transition-colors flex items-center gap-1.5 w-fit"
            onClick={handleCheck}
          >
            <RotateCcw className="w-3.5 h-3.5" />
            {t('common.retry')}
          </button>
        </div>
      )}
    </div>
  );
}