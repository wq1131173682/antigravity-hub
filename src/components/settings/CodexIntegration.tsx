import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useConfigStore } from '../../stores/useConfigStore';
import { usePlatformStore } from '../../stores/usePlatformStore';
import * as codexService from '../../services/codexService';
import { showToast } from '../common/ToastContainer';
import { Terminal, CheckCircle2, XCircle, AlertTriangle, RotateCcw, Loader2, Play, FileCode, RefreshCw, Trash2, Lightbulb, Eye, EyeOff } from 'lucide-react';

type StatusType = 'idle' | 'checking' | 'ready' | 'applying' | 'success' | 'error';

const REASONING_EFFORTS = ['', 'low', 'medium', 'high'] as const;

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

  // New optional settings
  const [reasoningEffort, setReasoningEffort] = useState<string>('');
  const [disableResponseStorage, setDisableResponseStorage] = useState(true);
  const [apiKey, setApiKey] = useState<string>('');
  const [showApiKey, setShowApiKey] = useState(false);

  // Env conflict detection
  const [envConflicts, setEnvConflicts] = useState<codexService.EnvConflictResult | null>(null);

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

      // Also check env conflicts
      try {
        const conflicts = await codexService.checkCodexEnvConflicts();
        setEnvConflicts(conflicts);
      } catch {
        // env check is optional
      }
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
      const res = await codexService.applyCodexConfig({
        proxyHost: config.proxy_host || '127.0.0.1',
        proxyPort: config.proxy_port || 8045,
        pathPrefix,
        modelName: selectedModelName,
        reasoningEffort: reasoningEffort || null,
        disableResponseStorage,
        apiKey: apiKey || null,
      });
      setResult(res);
      setStatus('success');
      showToast(t('codex.apply_success'), 'success');

      const cs = await codexService.checkCodexStatus();
      setCodexStatus(cs);
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
      setStatus('error');
    }
  }, [selectedModelName, config, platforms, selectedPlatformId, t, reasoningEffort, disableResponseStorage, apiKey]);

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

  const handleClearAuth = useCallback(async () => {
    try {
      const res = await codexService.clearCodexAuth();
      setResult(res);
      showToast(t('codex.auth_cleared'), 'success');
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    }
  }, [t]);

  const platformModels = selectedPlatformId ? (models[selectedPlatformId] || []) : [];
  const pathPrefix = (platforms.find(p => p.id === selectedPlatformId)?.path_prefix) || 'openai';

  // Check env conflicts on initial load
  const checkCodexEnvConflicts = useCallback(async () => {
    try {
      const conflicts = await codexService.checkCodexEnvConflicts();
      setEnvConflicts(conflicts);
    } catch {
      // env check is optional
    }
  }, []);

  useEffect(() => {
    checkCodexEnvConflicts();
  }, [checkCodexEnvConflicts]);

  const hasConflicts = envConflicts && (
    envConflicts.has_openai_api_key ||
    envConflicts.has_openai_base_url ||
    envConflicts.has_openai_org_id ||
    envConflicts.has_codex_home
  );

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

      {/* Environment Variable Conflict Warning */}
      {hasConflicts && (
        <div className="mb-4 p-3 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 rounded-lg">
          <div className="flex items-start gap-2">
            <AlertTriangle className="w-4 h-4 text-amber-500 mt-0.5 shrink-0" />
            <div>
              <p className="text-xs font-medium text-amber-700 dark:text-amber-300 mb-1">
                {t('codex.env_conflict_title')}
              </p>
              <ul className="text-xs text-amber-600 dark:text-amber-400 space-y-0.5">
                {envConflicts?.messages.map((msg, i) => (
                  <li key={i} className="flex items-start gap-1">
                    <span>•</span>
                    <span>{msg}</span>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </div>
      )}

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

        {/* Advanced Options */}
        <details className="group">
          <summary className="text-xs font-medium text-gray-500 dark:text-gray-400 cursor-pointer hover:text-gray-700 dark:hover:text-gray-300 flex items-center gap-1.5 select-none">
            <Lightbulb className="w-3.5 h-3.5" />
            {t('codex.advanced_options')}
          </summary>
          <div className="mt-3 space-y-3">
            {/* Reasoning Effort */}
            <div>
              <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5">
                {t('codex.reasoning_effort')}
              </label>
              <select
                className="w-full px-3 py-2 text-sm border rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-emerald-500 outline-none border-gray-200 dark:border-base-300"
                value={reasoningEffort}
                onChange={(e) => setReasoningEffort(e.target.value)}
              >
                <option value="">{t('codex.reasoning_default')}</option>
                {REASONING_EFFORTS.filter(v => v !== '').map((effort) => (
                  <option key={effort} value={effort}>{t(`codex.reasoning_${effort}`)}</option>
                ))}
              </select>
            </div>

            {/* Disable Response Storage */}
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                className="checkbox checkbox-sm checkbox-success rounded"
                checked={disableResponseStorage}
                onChange={(e) => setDisableResponseStorage(e.target.checked)}
              />
              <span className="text-xs text-gray-600 dark:text-gray-400">
                {t('codex.disable_storage')}
              </span>
            </label>

            {/* API Key (optional) */}
            <div>
              <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1.5">
                {t('codex.api_key_label')}
              </label>
              <div className="relative">
                <input
                  type={showApiKey ? 'text' : 'password'}
                  className="w-full px-3 py-2 pr-8 text-sm border rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-emerald-500 outline-none border-gray-200 dark:border-base-300 font-mono"
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                  placeholder={t('codex.api_key_placeholder')}
                />
                <button
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                  onClick={() => setShowApiKey(!showApiKey)}
                  tabIndex={-1}
                >
                  {showApiKey ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
                </button>
              </div>
              <p className="text-xs text-gray-400 dark:text-gray-500 mt-1">
                {t('codex.api_key_hint')}
              </p>
            </div>
          </div>
        </details>

        {/* Config Preview */}
        <div className="bg-gray-50 dark:bg-base-300 rounded-lg p-3 border border-gray-100 dark:border-base-200">
          <div className="flex items-center gap-1.5 mb-2">
            <FileCode className="w-3.5 h-3.5 text-gray-400" />
            <span className="text-xs font-medium text-gray-500 dark:text-gray-400">
              {t('codex.config_preview')}
            </span>
          </div>
          <pre className="text-xs font-mono text-gray-600 dark:text-gray-400 leading-relaxed whitespace-pre-wrap">
{(() => {
  const lines = [
    'model = "' + (selectedModelName || '<model>') + '"',
    'model_provider = "' + pathPrefix + '"',
    'preferred_auth_method = "apikey"',
  ];
  if (disableResponseStorage) lines.push('disable_response_storage = true');
  if (reasoningEffort) lines.push('model_reasoning_effort = "' + reasoningEffort + '"');
  lines.push('');
  lines.push('[model_providers.' + pathPrefix + ']');
  lines.push('name = "' + pathPrefix + '"');
  lines.push('base_url = "http://' + (config?.proxy_host || '127.0.0.1') + ':' + (config?.proxy_port || 8045) + '/' + pathPrefix + '/v1"');
  lines.push('wire_api = "responses"');
  lines.push('requires_openai_auth = false');
  if (apiKey) lines.push('api_key = "' + apiKey.slice(0, 4) + '****"');
  return lines.join('\n');
})()}
          </pre>
        </div>

        {/* Troubleshooting Tips */}
        <details className="group">
          <summary className="text-xs font-medium text-gray-500 dark:text-gray-400 cursor-pointer hover:text-gray-700 dark:hover:text-gray-300 flex items-center gap-1.5 select-none">
            <AlertTriangle className="w-3.5 h-3.5" />
            {t('codex.troubleshooting')}
          </summary>
          <div className="mt-2 p-3 bg-blue-50 dark:bg-blue-900/10 border border-blue-100 dark:border-blue-800/30 rounded-lg text-xs text-blue-700 dark:text-blue-300 space-y-1.5">
            <p><strong>401 Unauthorized:</strong> {t('codex.tip_401')}</p>
            <p><strong>Stream disconnected:</strong> {t('codex.tip_stream')}</p>
            <p><strong>OAuth conflicts:</strong> {t('codex.tip_oauth')}</p>
            <p><strong>Model not found:</strong> {t('codex.tip_model')}</p>
          </div>
        </details>

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
          <button className="px-4 py-2 text-sm font-medium rounded-lg bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 border border-red-200 dark:border-red-800 hover:bg-red-100 dark:hover:bg-red-900/30 transition-colors flex items-center gap-1.5" onClick={handleClearAuth}>
            <Trash2 className="w-3.5 h-3.5" />
            {t('codex.clear_auth')}
          </button>
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
