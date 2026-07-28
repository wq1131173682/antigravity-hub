import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useConfigStore } from '../../stores/useConfigStore';
import { usePlatformStore } from '../../stores/usePlatformStore';
import * as codexService from '../../services/codexService';
import { showToast } from '../common/ToastContainer';
import { Terminal, CheckCircle2, XCircle, AlertTriangle, RotateCcw, Loader2, Play, FileCode, RefreshCw } from 'lucide-react';

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
  const [modelsLoading, setModelsLoading] = useState(false);

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
      setModelsLoading(true);
      setSelectedModelName('');
      fetchModels(selectedPlatformId).then(() => {
        const currentModels = usePlatformStore.getState().models[selectedPlatformId];
        if (currentModels && currentModels.length > 0 && !autoDetected) {
          setSelectedModelName(currentModels[0].model_name);
          setAutoDetected(true);
        }
        setModelsLoading(false);
      }).catch(() => setModelsLoading(false));
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

  const platformModels = selectedPlatformId ? (models[selectedPlatformId] || []) : [];

  const handleApply = useCallback(async () => {
    if (!selectedModelName || !config) return;

    const selectedPlatform = platforms.find(p => p.id === selectedPlatformId);
    const pathPrefix = selectedPlatform?.path_prefix || 'openai';

    // Find the selected model to get max_input_tokens
    const selectedModel = platformModels.find(m => m.model_name === selectedModelName);

    setStatus('applying');
    setResult(null);
    try {
      const res = await codexService.applyCodexConfig(
        config.proxy_host || '127.0.0.1',
        config.proxy_port || 8045,
        pathPrefix,
        selectedModelName,
        selectedModel?.max_input_tokens,
        selectedPlatformId,
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
  }, [selectedModelName, config, platforms, selectedPlatformId, t, platformModels]);

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

  return (
    <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
      <div className="flex items-center gap-2 mb-4">
        <Terminal className="w-4 h-4 text-emerald-500" />
        <h2 className="font-semibold text-gray-900 dark:text-base-content">
          {t('codex.title')}
        </h2>
      </div>

      <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">
        {t('codex.description')}
      </p>

      {/* Codex Status */}
      {status === 'checking' && (
        <div className="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400 mb-4">
          <Loader2 className="w-4 h-4 animate-spin text-emerald-500" />
          {t('codex.checking')}
        </div>
      )}

      {codexStatus && (
        <div className="space-y-2 mb-4">
          <div className="flex items-center gap-2 text-sm">
            {codexStatus.installed ? (
              <span className="flex items-center gap-1.5 text-emerald-600 dark:text-emerald-400">
                <CheckCircle2 className="w-4 h-4" />
                {t('codex.installed')}
              </span>
            ) : (
              <span className="flex items-center gap-1.5 text-red-500">
                <XCircle className="w-4 h-4" />
                {t('codex.not_installed')}
              </span>
            )}
            {codexStatus.is_antigravity_configured && (
              <span className="flex items-center gap-1.5 text-blue-500">
                <CheckCircle2 className="w-3.5 h-3.5" />
                {t('codex.already_configured')}
              </span>
            )}
          </div>
          {codexStatus.has_backup && (
            <div className="flex items-center gap-1.5 text-xs text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-900/20 px-3 py-2 rounded-lg">
              <AlertTriangle className="w-3.5 h-3.5" />
              {t('codex.has_backup')}
            </div>
          )}
        </div>
      )}

      {status === 'error' && (
        <div className="flex items-center gap-2 text-sm text-red-600 dark:text-red-400 mb-4">
          <XCircle className="w-4 h-4" />
          {t('codex.check_failed')}
        </div>
      )}

      {/* Platform & Model Selection */}
      <div className="space-y-4">
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <div>
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5">
              {t('codex.select_platform')}
            </label>
            <select
              className="w-full px-3 py-2 text-sm border rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-emerald-500 outline-none border-gray-200 dark:border-base-300"
              value={selectedPlatformId}
              onChange={(e) => { setSelectedPlatformId(e.target.value); setAutoDetected(false); }}
            >
              {platforms.map((p) => (
                <option key={p.id} value={p.id}>{p.name}</option>
              ))}
            </select>
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5">
              {t('codex.select_model')}
            </label>
            {modelsLoading ? (
              <select className="w-full px-3 py-2 text-sm border rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-emerald-500 outline-none border-gray-200 dark:border-base-300" disabled>
                <option>{t('common.loading')}</option>
              </select>
            ) : platformModels.length > 0 ? (
              <select
                className="w-full px-3 py-2 text-sm border rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-emerald-500 outline-none border-gray-200 dark:border-base-300"
                value={selectedModelName}
                onChange={(e) => setSelectedModelName(e.target.value)}
              >
                <option value="">{t('codex.select_model')}...</option>
                {platformModels.map((m) => (
                  <option key={m.id} value={m.model_name}>{m.display_name || m.model_name}</option>
                ))}
              </select>
            ) : (
              <input
                type="text"
                className="w-full px-3 py-2 text-sm border rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-emerald-500 outline-none border-gray-200 dark:border-base-300 font-mono"
                value={selectedModelName}
                onChange={(e) => setSelectedModelName(e.target.value)}
                placeholder="gpt-4o"
              />
            )}
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
model_catalog_json = "~/.codex/model-catalogs/${(platforms.find(p => p.id === selectedPlatformId)?.path_prefix) || 'openai'}.json"
model_provider = "${(platforms.find(p => p.id === selectedPlatformId)?.path_prefix) || 'openai'}"
preferred_auth_method = "apikey"

[model_providers.${(platforms.find(p => p.id === selectedPlatformId)?.path_prefix) || 'openai'}]
name = "${(platforms.find(p => p.id === selectedPlatformId)?.path_prefix) || 'openai'}"
base_url = "http://127.0.0.1:${config?.proxy_port || 8045}/${(platforms.find(p => p.id === selectedPlatformId)?.path_prefix) || 'openai'}/v1"
env_key = "${((platforms.find(p => p.id === selectedPlatformId)?.path_prefix) || 'openai').toUpperCase()}_API_KEY"
wire_api = "responses"`}
{/* Note: model_catalog_json is resolved to the absolute path on Windows
    (e.g., C:\\Users\\<username>\\.codex\\model-catalogs\\<prefix>.json) */}
          </pre>
          {/* Context Size Indicator */}
          {(() => {
            const selectedModel = platformModels.find(m => m.model_name === selectedModelName);
            if (!selectedModel?.max_input_tokens) return null;
            const ctx = selectedModel.max_input_tokens;
            const ctxK = Math.round(ctx / 1024);
            const ctxM = ctx >= 1048576 ? `${(ctx / 1048576).toFixed(0)}M` : null;
            return (
              <div className="mt-2 flex items-center gap-2 text-xs">
                <span className="text-gray-400">上下文 / Context:</span>
                <span className="font-mono text-emerald-600 dark:text-emerald-400 font-medium">
                  {ctxM || `${ctxK}K`} ({ctx.toLocaleString()} tokens)
                </span>
                <span className="text-gray-400">→ model-catalog.json</span>
              </div>
            );
          })()}
        </div>

        {/* Action Buttons */}
        <div className="flex flex-wrap gap-2">
          <button className="px-4 py-2 bg-emerald-500 text-white text-sm font-medium rounded-lg hover:bg-emerald-600 transition-colors flex items-center gap-1.5 shadow-sm" onClick={handleCheck}>
            <Play className="w-3.5 h-3.5" />
            {t('codex.check_status')}
          </button>
          <button className={`px-4 py-2 text-sm font-medium rounded-lg transition-colors flex items-center gap-1.5 shadow-sm ${!selectedModelName ? 'bg-gray-300 dark:bg-base-300 text-gray-500 cursor-not-allowed' : 'bg-emerald-500 text-white hover:bg-emerald-600'}`} onClick={handleApply} disabled={!selectedModelName}>
            <Terminal className="w-3.5 h-3.5" />
            {t('codex.apply')}
          </button>
          {codexStatus?.has_backup && (
            <button className="px-4 py-2 text-sm font-medium rounded-lg bg-amber-50 dark:bg-amber-900/20 text-amber-600 dark:text-amber-400 border border-amber-200 dark:border-amber-800 hover:bg-amber-100 dark:hover:bg-amber-900/30 transition-colors flex items-center gap-1.5" onClick={handleRestore}>
              <RotateCcw className="w-3.5 h-3.5" />
              {t('codex.restore')}
            </button>
          )}
          <button className="px-4 py-2 text-sm font-medium rounded-lg bg-gray-100 dark:bg-base-300 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-base-200 transition-colors flex items-center gap-1.5" onClick={handleCheck}>
            <RefreshCw className="w-3.5 h-3.5" />
            {t('common.refresh')}
          </button>
        </div>

        {result && (
          <div className={`p-3 rounded-lg text-sm ${result.success ? 'bg-emerald-50 dark:bg-emerald-900/20 text-emerald-700 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-800' : 'bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-300 border border-red-200 dark:border-red-800'}`}>
            <pre className="whitespace-pre-wrap font-sans text-xs">{result.message}</pre>
          </div>
        )}
      </div>
    </div>
  );
}
