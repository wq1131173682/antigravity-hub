import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { X, Wifi, Loader2, CheckCircle2, XCircle, Clock, Hash } from 'lucide-react';
import type { TestModelResult } from '../../services/platformService';

interface TestModelDialogProps {
  open: boolean;
  onClose: () => void;
  platformId: string | null;
  modelName: string;
  displayName: string;
  testModel: (platformId: string, modelName: string) => Promise<TestModelResult>;
}

export default function TestModelDialog({ open, onClose, platformId, modelName, displayName, testModel }: TestModelDialogProps) {
  const { t } = useTranslation();
  const [result, setResult] = useState<TestModelResult | null>(null);
  const [testing, setTesting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleTest = async () => {
    if (!platformId) return;
    setTesting(true);
    setResult(null);
    setError(null);
    try {
      const res = await testModel(platformId, modelName);
      setResult(res);
    } catch (e) {
      setError(String(e));
    } finally {
      setTesting(false);
    }
  };

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={onClose}>
      <div className="bg-white dark:bg-base-100 rounded-xl shadow-xl w-full max-w-lg mx-4 border border-gray-200 dark:border-base-300 overflow-hidden" onClick={e => e.stopPropagation()}>
        {/* Header */}
        <div className="px-5 py-4 border-b border-gray-100 dark:border-base-300 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Wifi className={`w-4 h-4 ${result?.success ? 'text-green-500' : 'text-gray-400'}`} />
            <h3 className="font-bold text-gray-900 dark:text-base-content">
              {t('diagnostics.title')}
            </h3>
          </div>
          <button className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors" onClick={onClose}>
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Model Info */}
        <div className="px-5 py-3 bg-gray-50 dark:bg-base-200 border-b border-gray-100 dark:border-base-300">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-gray-700 dark:text-gray-300">{displayName}</span>
            <span className="text-xs text-gray-400 font-mono">{modelName}</span>
          </div>
        </div>

        {/* Content */}
        <div className="px-5 py-4 space-y-4">
          {/* Test Button */}
          {!result && !error && !testing && (
            <div className="text-center py-4">
              <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">
                {t('diagnostics.hint')}
              </p>
              <button
                className="px-5 py-2 bg-blue-500 text-white text-sm font-medium rounded-lg hover:bg-blue-600 transition-colors flex items-center gap-2 mx-auto shadow-sm"
                onClick={handleTest}
              >
                <Wifi className="w-4 h-4" />
                {t('diagnostics.run_test')}
              </button>
            </div>
          )}

          {/* Loading */}
          {testing && (
            <div className="text-center py-8">
              <Loader2 className="w-8 h-8 animate-spin text-blue-500 mx-auto mb-3" />
              <p className="text-sm text-gray-500 dark:text-gray-400">{t('diagnostics.testing')}</p>
            </div>
          )}

          {/* Error */}
          {error && (
            <div className="px-4 py-3 bg-red-50 dark:bg-red-900/20 rounded-lg border border-red-100 dark:border-red-900/30">
              <div className="flex items-center gap-2 mb-1">
                <XCircle className="w-4 h-4 text-red-500" />
                <span className="text-sm font-medium text-red-700 dark:text-red-400">{t('common.error')}</span>
              </div>
              <p className="text-xs text-red-600 dark:text-red-300 mt-1 font-mono">{error}</p>
            </div>
          )}

          {/* Results */}
          {result && (
            <div className="space-y-3">
              {/* Status badge */}
              <div className={`flex items-center gap-2 px-4 py-3 rounded-lg ${
                result.success
                  ? 'bg-green-50 dark:bg-green-900/20 border border-green-100 dark:border-green-900/30'
                  : 'bg-red-50 dark:bg-red-900/20 border border-red-100 dark:border-red-900/30'
              }`}>
                {result.success
                  ? <CheckCircle2 className="w-5 h-5 text-green-500 shrink-0" />
                  : <XCircle className="w-5 h-5 text-red-500 shrink-0" />
                }
                <span className={`text-sm font-medium ${
                  result.success ? 'text-green-700 dark:text-green-400' : 'text-red-700 dark:text-red-400'
                }`}>
                  {result.message}
                </span>
              </div>

              {/* Diagnostic details grid */}
              <div className="grid grid-cols-2 gap-2">
                <div className="px-3 py-2.5 bg-gray-50 dark:bg-base-200 rounded-lg">
                  <div className="flex items-center gap-1.5 text-xs text-gray-500 dark:text-gray-400 mb-1">
                    <Clock className="w-3 h-3" />
                    {t('diagnostics.latency')}
                  </div>
                  <span className={`text-sm font-mono font-medium ${
                    result.latency_ms < 1000 ? 'text-green-600 dark:text-green-400' :
                    result.latency_ms < 3000 ? 'text-yellow-600 dark:text-yellow-400' :
                    'text-red-600 dark:text-red-400'
                  }`}>
                    {result.latency_ms}ms
                  </span>
                </div>
                <div className="px-3 py-2.5 bg-gray-50 dark:bg-base-200 rounded-lg">
                  <div className="flex items-center gap-1.5 text-xs text-gray-500 dark:text-gray-400 mb-1">
                    <Hash className="w-3 h-3" />
                    {t('diagnostics.status_code')}
                  </div>
                  <span className={`text-sm font-mono font-medium ${
                    result.status_code >= 200 && result.status_code < 300
                      ? 'text-green-600 dark:text-green-400'
                      : result.status_code >= 400 && result.status_code < 500
                        ? 'text-yellow-600 dark:text-yellow-400'
                        : 'text-red-600 dark:text-red-400'
                  }`}>
                    {result.status_code}
                  </span>
                </div>
              </div>

              {/* Finish reason */}
              {result.finish_reason && (
                <div className="px-3 py-2.5 bg-gray-50 dark:bg-base-200 rounded-lg">
                  <div className="text-xs text-gray-500 dark:text-gray-400 mb-0.5">{t('diagnostics.finish_reason')}</div>
                  <span className="text-sm font-mono text-gray-700 dark:text-gray-300">{result.finish_reason}</span>
                </div>
              )}

              {/* Response preview */}
              {result.response_preview && (
                <div>
                  <div className="text-xs text-gray-500 dark:text-gray-400 mb-1.5">{t('diagnostics.response_preview')}</div>
                  <pre className="px-3 py-2 bg-gray-900 dark:bg-black text-[11px] text-green-400 rounded-lg overflow-x-auto max-h-40 whitespace-pre-wrap break-all font-mono leading-relaxed">
                    {result.response_preview}
                  </pre>
                </div>
              )}
            </div>
          )}

          {/* Re-test button when done */}
          {result && !testing && (
            <button
              className="w-full px-4 py-2 text-sm text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-900/20 hover:bg-blue-100 dark:hover:bg-blue-900/30 rounded-lg transition-colors flex items-center justify-center gap-1.5"
              onClick={handleTest}
            >
              <Wifi className="w-3.5 h-3.5" />
              {t('diagnostics.test_again')}
            </button>
          )}
        </div>

        {/* Footer */}
        <div className="px-5 py-3 border-t border-gray-100 dark:border-base-300 flex justify-end">
          <button
            className="px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-base-300 rounded-lg transition-colors"
            onClick={onClose}
          >
            {t('common.close')}
          </button>
        </div>
      </div>
    </div>
  );
}
