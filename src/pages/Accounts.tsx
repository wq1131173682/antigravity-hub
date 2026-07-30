import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { usePlatformStore } from '../stores/usePlatformStore';
import type { Model } from '../types/platform';
import { showToast } from '../components/common/ToastContainer';
import ModalDialog from '../components/common/ModalDialog';
import TestModelDialog from '../components/accounts/TestModelDialog';
import {
  Plus, Trash2, Edit3, Key, Server, Layers,
  Eye, EyeOff, Power, PowerOff, Link2, Unlink,
  X, RefreshCw, Wifi
} from 'lucide-react';

// ---------------------------------------------------------------------------
// Dialog: Add Model
// ---------------------------------------------------------------------------
function AddModelDialog({ open, onClose, platformId }: { open: boolean; onClose: () => void; platformId: string }) {
  const { t } = useTranslation();
  const { addModel } = usePlatformStore();
  const [modelName, setModelName] = useState('');
  const [displayName, setDisplayName] = useState('');
  const [limit5h, setLimit5h] = useState('10000');
  const [limitDay, setLimitDay] = useState('50000');
  const [limitMonth, setLimitMonth] = useState('1000000');
  const [maxInputTokens, setMaxInputTokens] = useState('1048576');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open) { setModelName(''); setDisplayName(''); setLimit5h('10000'); setLimitDay('50000'); setLimitMonth('1000000'); setMaxInputTokens('1048576'); }
  }, [open]);

  const handleSave = async () => {
    if (!modelName.trim()) { showToast(t('accounts.fill_model_name'), 'error'); return; }
    setSaving(true);
    try {
      await addModel(platformId, modelName.trim(), displayName.trim() || modelName.trim(),
        Number(limit5h), Number(limitDay), Number(limitMonth),
        maxInputTokens.trim() ? Number(maxInputTokens) : null);
      showToast(t('common.success'), 'success');
      onClose();
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally { setSaving(false); }
  };

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={onClose}>
      <div className="bg-white dark:bg-base-100 rounded-xl shadow-xl w-full max-w-md mx-4 p-5 border border-gray-200 dark:border-base-300" onClick={e => e.stopPropagation()}>
        <div className="flex justify-between items-center mb-4">
          <h3 className="font-bold text-gray-900 dark:text-base-content">{t('accounts.add_model')}</h3>
          <button className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300" onClick={onClose}><X className="w-5 h-5" /></button>
        </div>
        <div className="space-y-3">
          <div>
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('accounts.model_name')}</label>
            <input className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={modelName} onChange={e => setModelName(e.target.value)} placeholder="gpt-4o" />
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('accounts.display_name')}</label>
            <input className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={displayName} onChange={e => setDisplayName(e.target.value)} placeholder="GPT-4o" />
          </div>
          <div className="grid grid-cols-3 gap-2">
            <div>
              <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('quota.per_5hour')}</label>
              <input type="number" min="0" className="w-full px-2 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={limit5h} onChange={e => setLimit5h(e.target.value)} />
            </div>
            <div>
              <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('quota.per_day')}</label>
              <input type="number" min="0" className="w-full px-2 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={limitDay} onChange={e => setLimitDay(e.target.value)} />
            </div>
            <div>
              <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('quota.per_month')}</label>
              <input type="number" min="0" className="w-full px-2 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={limitMonth} onChange={e => setLimitMonth(e.target.value)} />
            </div>
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
              上下文大小 / Context Size <span className="text-gray-400 font-normal">(max_input_tokens)</span>
            </label>
            <div className="flex gap-1.5 mb-1.5">
              {[
                { label: '128K', value: 128000 },
                { label: '200K', value: 200000 },
                { label: '1M', value: 1048576 },
                { label: '2M', value: 2097152 },
              ].map(preset => (
                <button
                  key={preset.label}
                  type="button"
                  className={`px-2 py-1 text-[11px] font-mono font-medium rounded-md transition-colors ${
                    Number(maxInputTokens) === preset.value
                      ? 'bg-indigo-100 dark:bg-indigo-900/30 text-indigo-700 dark:text-indigo-300 border border-indigo-200 dark:border-indigo-800'
                      : 'bg-gray-100 dark:bg-base-300 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-base-200 border border-transparent'
                  }`}
                  onClick={() => setMaxInputTokens(String(preset.value))}
                >
                  {preset.label}
                </button>
              ))}
              <button
                type="button"
                className={`px-2 py-1 text-[11px] font-mono font-medium rounded-md transition-colors ${
                  !maxInputTokens.trim()
                    ? 'bg-gray-200 dark:bg-base-200 text-gray-500 dark:text-gray-400 border border-gray-300 dark:border-base-300'
                    : 'bg-gray-100 dark:bg-base-300 text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-base-200 border border-transparent'
                }`}
                onClick={() => setMaxInputTokens('')}
              >
                自动
              </button>
            </div>
            <input type="number" min="0" step="1" className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none font-mono" value={maxInputTokens} onChange={e => setMaxInputTokens(e.target.value)} placeholder={t('diagnostics.context_placeholder')} />
          </div>
        </div>
        <div className="flex justify-end gap-2 mt-5">
          <button className="px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-base-300 rounded-lg transition-colors" onClick={onClose}>{t('common.cancel')}</button>
          <button className={`px-4 py-2 text-sm text-white rounded-lg transition-colors ${saving ? 'bg-blue-400' : 'bg-blue-500 hover:bg-blue-600'}`} onClick={handleSave} disabled={saving}>{saving ? t('common.saving') : t('common.save')}</button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Dialog: Edit Model Quotas
// ---------------------------------------------------------------------------
function EditModelQuotaDialog({ open, onClose, model }: { open: boolean; onClose: () => void; model: Model | null }) {
  const { t } = useTranslation();
  const { updateModelLimits } = usePlatformStore();
  const [limit5h, setLimit5h] = useState('0');
  const [limitDay, setLimitDay] = useState('0');
  const [limitMonth, setLimitMonth] = useState('0');
  const [maxInputTokens, setMaxInputTokens] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open && model) {
      setLimit5h(String(model.per_5hour ?? 0));
      setLimitDay(String(model.per_day ?? 0));
      setLimitMonth(String(model.per_month ?? 0));
      setMaxInputTokens(model.max_input_tokens ? String(model.max_input_tokens) : '');
    }
  }, [open, model]);

  const handleSave = async () => {
    if (!model) return;
    setSaving(true);
    try {
      await updateModelLimits(model.id, Number(limit5h), Number(limitDay), Number(limitMonth),
        maxInputTokens.trim() ? Number(maxInputTokens) : null);
      showToast(t('common.success'), 'success');
      onClose();
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally { setSaving(false); }
  };

  if (!open || !model) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={onClose}>
      <div className="bg-white dark:bg-base-100 rounded-xl shadow-xl w-full max-w-md mx-4 p-5 border border-gray-200 dark:border-base-300" onClick={e => e.stopPropagation()}>
        <div className="flex justify-between items-center mb-4">
          <h3 className="font-bold text-gray-900 dark:text-base-content">{t('accounts.edit_model_quotas')} - {model.display_name || model.model_name}</h3>
          <button className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300" onClick={onClose}><X className="w-5 h-5" /></button>
        </div>
        <div className="space-y-3">
          <div className="grid grid-cols-3 gap-3">
            <div>
              <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('quota.per_5hour')}</label>
              <input type="number" min="0" className="w-full px-2 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={limit5h} onChange={e => setLimit5h(e.target.value)} />
            </div>
            <div>
              <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('quota.per_day')}</label>
              <input type="number" min="0" className="w-full px-2 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={limitDay} onChange={e => setLimitDay(e.target.value)} />
            </div>
            <div>
              <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('quota.per_month')}</label>
              <input type="number" min="0" className="w-full px-2 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={limitMonth} onChange={e => setLimitMonth(e.target.value)} />
            </div>
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
              上下文大小 / Context Size <span className="text-gray-400 font-normal">(max_input_tokens)</span>
            </label>
            <div className="flex gap-1.5 mb-1.5">
              {[
                { label: '128K', value: 128000 },
                { label: '200K', value: 200000 },
                { label: '1M', value: 1048576 },
                { label: '2M', value: 2097152 },
              ].map(preset => (
                <button
                  key={preset.label}
                  type="button"
                  className={`px-2 py-1 text-[11px] font-mono font-medium rounded-md transition-colors ${
                    Number(maxInputTokens) === preset.value
                      ? 'bg-indigo-100 dark:bg-indigo-900/30 text-indigo-700 dark:text-indigo-300 border border-indigo-200 dark:border-indigo-800'
                      : 'bg-gray-100 dark:bg-base-300 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-base-200 border border-transparent'
                  }`}
                  onClick={() => setMaxInputTokens(String(preset.value))}
                >
                  {preset.label}
                </button>
              ))}
              <button
                type="button"
                className={`px-2 py-1 text-[11px] font-mono font-medium rounded-md transition-colors ${
                  !maxInputTokens.trim()
                    ? 'bg-gray-200 dark:bg-base-200 text-gray-500 dark:text-gray-400 border border-gray-300 dark:border-base-300'
                    : 'bg-gray-100 dark:bg-base-300 text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-base-200 border border-transparent'
                }`}
                onClick={() => setMaxInputTokens('')}
              >
                自动
              </button>
            </div>
            <input type="number" min="0" step="1" className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none font-mono" value={maxInputTokens} onChange={e => setMaxInputTokens(e.target.value)} placeholder={t('diagnostics.context_placeholder')} />
          </div>
        </div>
        <div className="flex justify-end gap-2 mt-5">
          <button className="px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-base-300 rounded-lg transition-colors" onClick={onClose}>{t('common.cancel')}</button>
          <button className={`px-4 py-2 text-sm text-white rounded-lg transition-colors ${saving ? 'bg-blue-400' : 'bg-blue-500 hover:bg-blue-600'}`} onClick={handleSave} disabled={saving}>{saving ? t('common.saving') : t('common.save')}</button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Dialog: Add Key
// ---------------------------------------------------------------------------
function AddKeyDialog({ open, onClose, platformId }: { open: boolean; onClose: () => void; platformId: string }) {
  const { t } = useTranslation();
  const { addKey } = usePlatformStore();
  const [name, setName] = useState('');
  const [keyValue, setKeyValue] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open) { setName(''); setKeyValue(''); }
  }, [open]);

  const handleSave = async () => {
    if (!name.trim() || !keyValue.trim()) {
      showToast(t('accounts.fill_key_fields'), 'error');
      return;
    }
    setSaving(true);
    try {
      await addKey(platformId, name.trim(), keyValue.trim());
      showToast(t('common.success'), 'success');
      onClose();
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally { setSaving(false); }
  };

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={onClose}>
      <div className="bg-white dark:bg-base-100 rounded-xl shadow-xl w-full max-w-md mx-4 p-5 border border-gray-200 dark:border-base-300" onClick={e => e.stopPropagation()}>
        <div className="flex justify-between items-center mb-4">
          <h3 className="font-bold text-gray-900 dark:text-base-content">{t('accounts.add_key')}</h3>
          <button className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300" onClick={onClose}><X className="w-5 h-5" /></button>
        </div>
        <div className="space-y-3">
          <div>
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('accounts.key_name')}</label>
            <input className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={name} onChange={e => setName(e.target.value)} placeholder="e.g. Key-01" />
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">API Key</label>
            <input className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none font-mono" value={keyValue} onChange={e => setKeyValue(e.target.value)} placeholder="sk-..." />
          </div>
        </div>
        <div className="flex justify-end gap-2 mt-5">
          <button className="px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-base-300 rounded-lg transition-colors" onClick={onClose}>{t('common.cancel')}</button>
          <button className={`px-4 py-2 text-sm text-white rounded-lg transition-colors ${saving ? 'bg-blue-400' : 'bg-blue-500 hover:bg-blue-600'}`} onClick={handleSave} disabled={saving}>{saving ? t('common.saving') : t('common.save')}</button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Dialog: Assign Key to Model
// ---------------------------------------------------------------------------
function AssignKeyDialog({ open, onClose, modelId, platformId }: { open: boolean; onClose: () => void; modelId: string; platformId: string }) {
  const { t } = useTranslation();
  const { keys, associateKeyWithModel, modelKeyIds } = usePlatformStore();
  const [associating, setAssociating] = useState<string | null>(null);

  const platformKeys = keys[platformId] || [];
  const assignedKeyIds = new Set(modelKeyIds[modelId] || []);
  const availableKeys = platformKeys.filter(k => !assignedKeyIds.has(k.id) && !k.disabled);

  const handleAssign = async (keyId: string) => {
    setAssociating(keyId);
    try {
      await associateKeyWithModel(keyId, modelId);
      showToast(t('common.success'), 'success');
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally { setAssociating(null); }
  };

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={onClose}>
      <div className="bg-white dark:bg-base-100 rounded-xl shadow-xl w-full max-w-md mx-4 p-5 border border-gray-200 dark:border-base-300" onClick={e => e.stopPropagation()}>
        <div className="flex justify-between items-center mb-4">
          <h3 className="font-bold text-gray-900 dark:text-base-content">{t('accounts.assign_key')}</h3>
          <button className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300" onClick={onClose}><X className="w-5 h-5" /></button>
        </div>
        {availableKeys.length === 0 ? (
          <p className="text-sm text-gray-400 text-center py-6">{t('accounts.no_available_keys')}</p>
        ) : (
          <div className="space-y-1 max-h-60 overflow-y-auto">
            {availableKeys.map(k => (
              <div key={k.id} className="flex items-center justify-between px-3 py-2 rounded-lg hover:bg-gray-50 dark:hover:bg-base-200">
                <div className="flex items-center gap-2 min-w-0">
                  <Key className="w-3.5 h-3.5 text-gray-400 shrink-0" />
                  <span className="text-sm text-gray-700 dark:text-gray-300 truncate">{k.name}</span>
                </div>
                <button
                  className={`px-2.5 py-1 text-xs rounded-lg transition-colors shrink-0 ${
                    associating === k.id
                      ? 'bg-blue-100 text-blue-400 dark:bg-blue-900/30'
                      : 'bg-blue-50 text-blue-600 hover:bg-blue-100 dark:bg-blue-900/20 dark:text-blue-400'
                  }`}
                  onClick={() => handleAssign(k.id)}
                  disabled={associating === k.id}
                >
                  {associating === k.id ? '...' : <><Link2 className="w-3 h-3 inline-block mr-0.5" />{t('accounts.assign')}</>}
                </button>
              </div>
            ))}
          </div>
        )}
        <div className="flex justify-end mt-4">
          <button className="px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-base-300 rounded-lg transition-colors" onClick={onClose}>{t('common.close')}</button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Test Model Button component (opens the detailed dialog)
// ---------------------------------------------------------------------------
function TestModelButton({ modelName, displayName, onTest }: {
  modelName: string;
  displayName: string;
  onTest: (modelName: string, displayName: string) => void;
}) {
  return (
    <button
      className="p-1.5 text-gray-400 hover:text-emerald-500 rounded-lg hover:bg-emerald-50 dark:hover:bg-emerald-900/20 transition-colors"
      onClick={() => onTest(modelName, displayName)}
      title={`测试模型 ${displayName}`}
    >
      <Wifi className="w-3.5 h-3.5" />
    </button>
  );
}

// ---------------------------------------------------------------------------
// Quota bar component
// ---------------------------------------------------------------------------
function QuotaBar({ used, limit, label }: { used: number; limit: number; label: string }) {
  if (limit <= 0) return null;
  const pct = Math.min(100, Math.round((used / limit) * 100));
  const color = pct >= 100 ? 'bg-red-500' : pct >= 80 ? 'bg-orange-500' : 'bg-blue-500';
  return (
    <div className="space-y-0.5">
      <div className="flex justify-between text-xs text-gray-500 dark:text-gray-400">
        <span>{label}</span>
        <span>{used}/{limit}</span>
      </div>
      <div className="w-full h-1.5 bg-gray-100 dark:bg-base-300 rounded-full overflow-hidden">
        <div className={`h-full rounded-full transition-all ${color}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main Accounts Page
// ---------------------------------------------------------------------------
function Accounts() {
  const { t } = useTranslation();
  const {
    platforms, keys, models, modelUsage, modelKeyIds,
    fetchPlatforms, fetchKeys, fetchModels, fetchModelUsage,
    deletePlatform, deleteKey, setKeyStatus,
    deleteModel, disassociateKeyFromModel,
    refreshModelsFromUpstream, testModel
  } = usePlatformStore();
  const [selectedPlatformId, setSelectedPlatformId] = useState<string | null>(null);
  const [showAddPlatform, setShowAddPlatform] = useState(false);
  const [showAddModel, setShowAddModel] = useState(false);
  const [showAddKey, setShowAddKey] = useState(false);
  const [showEditQuota, setShowEditQuota] = useState(false);
  const [editingModel, setEditingModel] = useState<Model | null>(null);
  const [assigningModelId, setAssigningModelId] = useState<string | null>(null);
  const [showKeyValues, setShowKeyValues] = useState<Set<string>>(new Set());
  const [testDialog, setTestDialog] = useState<{ modelName: string; displayName: string } | null>(null);
  // Delete confirmation modal state
  const [deleteConfirm, setDeleteConfirm] = useState<{
    type: 'platform' | 'key' | 'model';
    id: string;
    platformId?: string;
  } | null>(null);

  useEffect(() => { fetchPlatforms(); }, []);
  useEffect(() => {
    if (!selectedPlatformId) return;
    fetchKeys(selectedPlatformId);
    fetchModels(selectedPlatformId);
  }, [selectedPlatformId]);

  // Fetch model usage for all models of selected platform
  useEffect(() => {
    if (!selectedPlatformId) return;
    const platformModels = models[selectedPlatformId] || [];
    platformModels.forEach(m => fetchModelUsage(m.id));
  }, [models[selectedPlatformId || '']?.length]);

  const selectedPlatform = platforms.find(p => p.id === selectedPlatformId);
  const platformKeys = selectedPlatformId ? (keys[selectedPlatformId] || []) : [];
  const platformModels = selectedPlatformId ? (models[selectedPlatformId] || []) : [];

  const toggleShowKey = (keyId: string) => {
    setShowKeyValues(prev => {
      const next = new Set(prev);
      if (next.has(keyId)) next.delete(keyId); else next.add(keyId);
      return next;
    });
  };

  const handleDeleteConfirm = async () => {
    if (!deleteConfirm) return;
    try {
      const { type, id, platformId } = deleteConfirm;
      if (type === 'platform') {
        await deletePlatform(id);
        if (selectedPlatformId === id) setSelectedPlatformId(null);
      } else if (type === 'key' && platformId) {
        await deleteKey(platformId, id);
      } else if (type === 'model' && selectedPlatformId) {
        await deleteModel(selectedPlatformId, id);
      }
      showToast(t('common.success'), 'success');
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally {
      setDeleteConfirm(null);
    }
  };

  const handleToggleKey = async (keyId: string, disabled: boolean) => {
    try {
      await setKeyStatus(keyId, !disabled);
      showToast(t('common.success'), 'success');
    } catch (e) { showToast(`${t('common.error')}: ${e}`, 'error'); }
  };

  const handleDisassociate = async (keyId: string, modelId: string) => {
    try {
      await disassociateKeyFromModel(keyId, modelId);
      showToast(t('common.success'), 'success');
    } catch (e) { showToast(`${t('common.error')}: ${e}`, 'error'); }
  };

  return (
    <div className="h-full w-full flex overflow-hidden">
      {/* Left: Platform Tree */}
      <div className="w-64 min-w-[200px] bg-white dark:bg-base-100 border-r border-gray-200 dark:border-base-300 flex flex-col overflow-hidden">
        <div className="p-3 border-b border-gray-100 dark:border-base-300 flex items-center justify-between">
          <h2 className="font-bold text-sm text-gray-900 dark:text-base-content">{t('accounts.platforms')}</h2>
          <button className="p-1.5 text-blue-500 hover:bg-blue-50 dark:hover:bg-blue-900/20 rounded-lg transition-colors" onClick={() => setShowAddPlatform(true)} title={t('accounts.add_platform')}><Plus className="w-4 h-4" /></button>
        </div>
        <div className="flex-1 overflow-y-auto p-2 space-y-1">
          {platforms.length === 0 && (
            <p className="text-xs text-gray-400 text-center py-8">{t('accounts.no_platforms')}</p>
          )}
          {platforms.map(p => (
            <div
              key={p.id}
              className={`group flex items-center justify-between px-3 py-2 rounded-lg cursor-pointer transition-colors ${
                selectedPlatformId === p.id
                  ? 'bg-blue-50 dark:bg-blue-900/20 text-blue-700 dark:text-blue-300'
                  : 'hover:bg-gray-50 dark:hover:bg-base-200 text-gray-700 dark:text-gray-300'
              }`}
              onClick={() => setSelectedPlatformId(p.id)}
            >
              <div className="flex items-center gap-2 min-w-0">
                <Server className="w-4 h-4 shrink-0" />
                <span className="text-sm font-medium truncate">{p.name}</span>
              </div>                              <button
                                className="opacity-0 group-hover:opacity-100 p-1 text-gray-400 hover:text-red-500 transition-all shrink-0"
                                onClick={e => { e.stopPropagation(); setDeleteConfirm({ type: 'platform', id: p.id }); }}
                                title={t('common.delete')}
                              ><Trash2 className="w-3.5 h-3.5" /></button>
            </div>
          ))}
        </div>
      </div>

      {/* Right: Models + Keys */}
      <div className="flex-1 overflow-y-auto bg-gray-50 dark:bg-base-200">
        {!selectedPlatform ? (
          <div className="flex items-center justify-center h-full text-gray-400 dark:text-gray-500 text-sm">
            <div className="text-center"><Server className="w-12 h-12 mx-auto mb-2 opacity-40" /><p>{t('accounts.select_platform')}</p></div>
          </div>
        ) : (
          <div className="p-5 space-y-6 max-w-4xl mx-auto">
            {/* Platform header */}
            <div className="flex items-center justify-between flex-wrap gap-2">
              <div>
                <h2 className="text-lg font-bold text-gray-900 dark:text-base-content">{selectedPlatform.name}</h2>
                <p className="text-xs text-gray-400 font-mono mt-0.5">{selectedPlatform.base_url}</p>
              </div>
              <div className="flex items-center gap-2">
                <button className="px-3 py-1.5 bg-white dark:bg-base-100 text-gray-700 dark:text-gray-300 text-xs font-medium rounded-lg border border-gray-200 dark:border-base-300 hover:bg-gray-50 dark:hover:bg-base-200 transition-colors flex items-center gap-1.5 shadow-sm" onClick={() => setShowAddKey(true)}>
                  <Key className="w-3.5 h-3.5" /> {t('accounts.add_key')}
                </button>
                <button className="px-3 py-1.5 bg-blue-500 text-white text-xs font-medium rounded-lg hover:bg-blue-600 transition-colors flex items-center gap-1.5 shadow-sm" onClick={() => setShowAddModel(true)}>
                  <Layers className="w-3.5 h-3.5" /> {t('accounts.add_model')}
                </button>
                <button className="px-3 py-1.5 bg-emerald-500 text-white text-xs font-medium rounded-lg hover:bg-emerald-600 transition-colors flex items-center gap-1.5 shadow-sm" onClick={async () => {
                  if (!selectedPlatformId) return;
                  try {
                    showToast('正在从上游获取模型信息...', 'info');
                    const result = await refreshModelsFromUpstream(selectedPlatformId);
                    if (result.created.length > 0) {
                      showToast(`新建 ${result.created.length} 个模型: ${result.created.join(', ')}`, 'success');
                    }
                    if (result.updated.length > 0) {
                      showToast(`更新 ${result.updated.length} 个模型: ${result.updated.join(', ')}`, 'success');
                    }
                    if (result.created.length === 0 && result.updated.length === 0) {
                      showToast(`已同步，所有 ${result.total_upstream} 个上游模型已是最新状态`, 'info');
                    }
                  } catch (e) {
                    showToast(`从上游获取模型信息失败: ${e}`, 'error');
                  }
                }}>
                  <RefreshCw className="w-3.5 h-3.5" /> 从上游同步
                </button>
              </div>
            </div>

            {/* Models section */}
            <div>
              <h3 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-3 flex items-center gap-2">
                <Layers className="w-4 h-4" /> {t('accounts.models')} ({platformModels.length})
              </h3>
              {platformModels.length === 0 ? (
                <div className="bg-white dark:bg-base-100 rounded-xl p-8 shadow-sm border border-gray-100 dark:border-base-200 text-center">
                  <Layers className="w-10 h-10 mx-auto text-gray-300 dark:text-gray-600 mb-2" />
                  <p className="text-sm text-gray-500 dark:text-gray-400">{t('accounts.no_models')}</p>
                </div>
              ) : (
                <div className="space-y-4">
                  {platformModels.map(model => {
                    const usage = modelUsage[model.id] || [];
                    const assignedKeyIds = new Set(modelKeyIds[model.id] || []);
                    const assignedKeys = platformKeys.filter(k => assignedKeyIds.has(k.id));
                    return (
                      <div key={model.id} className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden">
                        {/* Model header */}
                        <div className="px-4 py-3 border-b border-gray-100 dark:border-base-200 flex items-center justify-between">
                          <div className="flex items-center gap-2 min-w-0">
                            <Layers className="w-4 h-4 text-purple-500 shrink-0" />
                            <span className="font-medium text-sm text-gray-900 dark:text-base-content">{model.display_name || model.model_name}</span>
                            <span className="text-xs text-gray-400 font-mono">{model.model_name}</span>
                            {model.max_input_tokens && (
                              <span className="px-1.5 py-0.5 text-[10px] font-mono bg-indigo-50 dark:bg-indigo-900/20 text-indigo-600 dark:text-indigo-400 rounded font-medium">
                                {model.max_input_tokens >= 1000000
                                  ? `${(model.max_input_tokens / 1000000).toFixed(0)}M`
                                  : model.max_input_tokens >= 1000
                                    ? `${(model.max_input_tokens / 1000).toFixed(0)}K`
                                    : `${model.max_input_tokens}`} ctx
                              </span>
                            )}
                          </div>
                          <div className="flex items-center gap-1 shrink-0">                            <TestModelButton
                              modelName={model.model_name}
                              displayName={model.display_name || model.model_name}
                              onTest={(name, display) => setTestDialog({ modelName: name, displayName: display })}
                            />
                            <button className="p-1.5 text-gray-400 hover:text-blue-500 rounded-lg hover:bg-blue-50 dark:hover:bg-blue-900/20 transition-colors" onClick={() => { setEditingModel(model); setShowEditQuota(true); }} title={t('accounts.edit_model_quotas')}>
                              <Edit3 className="w-3.5 h-3.5" />
                            </button>
                            <button className="p-1.5 text-gray-400 hover:text-purple-500 rounded-lg hover:bg-purple-50 dark:hover:bg-purple-900/20 transition-colors" onClick={() => setAssigningModelId(model.id)} title={t('accounts.assign_key')}>
                              <Link2 className="w-3.5 h-3.5" />
                            </button>
                            <button className="p-1.5 text-gray-400 hover:text-red-500 rounded-lg hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors" onClick={() => setDeleteConfirm({ type: 'model', id: model.id })} title={t('common.delete')}>
                              <Trash2 className="w-3.5 h-3.5" />
                            </button>
                          </div>
                        </div>

                        {/* Quota bars */}
                        <div className="px-4 py-3 border-b border-gray-100 dark:border-base-200">
                          <div className="grid grid-cols-3 gap-3">
                            <QuotaBar used={usage.reduce((s, u) => s + (u.five_hour?.count || 0), 0)} limit={model.per_5hour || 0} label={t('quota.per_5hour')} />
                            <QuotaBar used={usage.reduce((s, u) => s + (u.day?.count || 0), 0)} limit={model.per_day || 0} label={t('quota.per_day')} />
                            <QuotaBar used={usage.reduce((s, u) => s + (u.month?.count || 0), 0)} limit={model.per_month || 0} label={t('quota.per_month')} />
                          </div>
                        </div>

                        {/* Associated keys */}
                        <div className="px-4 py-2">
                          <p className="text-xs text-gray-400 mb-2">{t('accounts.assigned_keys')} ({assignedKeys.length})</p>
                          {assignedKeys.length === 0 ? (
                            <p className="text-xs text-gray-400 italic">{t('accounts.no_assigned_keys')}</p>
                          ) : (
                            <div className="space-y-1.5">
                              {assignedKeys.map(k => {
                                const keyUsage = usage.find(u => u.key_id === k.id);
                                return (
                                  <div key={k.id} className="flex items-center justify-between px-2.5 py-1.5 bg-gray-50 dark:bg-base-300 rounded-lg">
                                    <div className="flex items-center gap-2 min-w-0">
                                      <Key className={`w-3 h-3 shrink-0 ${k.disabled ? 'text-gray-400' : 'text-blue-500'}`} />
                                      <span className="text-xs font-medium text-gray-700 dark:text-gray-300 truncate">{k.name}</span>
                                      <span className={`px-1 py-0.5 text-[10px] font-medium rounded ${
                                        k.disabled ? 'bg-red-50 dark:bg-red-900/20 text-red-500' : 'bg-green-50 dark:bg-green-900/20 text-green-600'
                                      }`}>{k.disabled ? t('accounts.disabled') : t('accounts.active')}</span>
                                    </div>
                                    <div className="flex items-center gap-3">
                                      <span className="text-[10px] text-gray-400">
                                        {t('quota.used_count').replace('{used}', String(keyUsage?.five_hour?.count || 0))}/{model.per_5hour || '-'}
                                      </span>
                                      <button className="p-1 text-gray-400 hover:text-red-500 transition-colors" onClick={() => handleDisassociate(k.id, model.id)} title={t('accounts.remove_key')}>
                                        <Unlink className="w-3 h-3" />
                                      </button>
                                    </div>
                                  </div>
                                );
                              })}
                            </div>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>

            {/* Keys section */}
            <div>
              <h3 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-3 flex items-center gap-2">
                <Key className="w-4 h-4" /> {t('accounts.all_keys')} ({platformKeys.length})
              </h3>
              {platformKeys.length === 0 ? (
                <div className="bg-white dark:bg-base-100 rounded-xl p-8 shadow-sm border border-gray-100 dark:border-base-200 text-center">
                  <Key className="w-10 h-10 mx-auto text-gray-300 dark:text-gray-600 mb-2" />
                  <p className="text-sm text-gray-500 dark:text-gray-400">{t('accounts.no_keys')}</p>
                </div>
              ) : (
                <div className="space-y-2">
                  {platformKeys.map(k => {
                    const assignedToModels = platformModels.filter(m => (modelKeyIds[m.id] || []).includes(k.id));
                    return (
                      <div key={k.id} className={`bg-white dark:bg-base-100 rounded-xl shadow-sm border overflow-hidden ${
                        k.disabled ? 'border-red-200 dark:border-red-900/50 opacity-70' : 'border-gray-100 dark:border-base-200'
                      }`}>
                        <div className="px-4 py-3">
                          <div className="flex items-center justify-between">
                            <div className="flex items-center gap-2 min-w-0">
                              <Key className={`w-4 h-4 shrink-0 ${k.disabled ? 'text-gray-400' : 'text-blue-500'}`} />
                              <span className="font-medium text-sm text-gray-900 dark:text-base-content">{k.name}</span>
                              <span className={`px-1.5 py-0.5 text-xs font-medium rounded ${
                                k.disabled
                                  ? 'bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400'
                                  : 'bg-green-50 dark:bg-green-900/20 text-green-600 dark:text-green-400'
                              }`}>{k.disabled ? t('accounts.disabled') : t('accounts.active')}</span>
                              {assignedToModels.length > 0 && (
                                <div className="flex gap-1">
                                  {assignedToModels.map(m => (
                                    <span key={m.id} className="px-1.5 py-0.5 text-[10px] bg-purple-50 dark:bg-purple-900/20 text-purple-600 dark:text-purple-400 rounded font-medium">
                                      {m.display_name || m.model_name}
                                    </span>
                                  ))}
                                </div>
                              )}
                            </div>
                            <div className="flex items-center gap-1 shrink-0">
                              {k.disabled && k.disabled_until && (
                                <span className="text-xs text-gray-400 hidden sm:inline">{new Date(k.disabled_until * 1000).toLocaleTimeString()}</span>
                              )}
                              <button className="p-1.5 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 rounded-lg hover:bg-gray-100 dark:hover:bg-base-300 transition-colors" onClick={() => toggleShowKey(k.id)} title={showKeyValues.has(k.id) ? t('common.hide') : t('common.show')}>
                                {showKeyValues.has(k.id) ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
                              </button>
                              <button className={`p-1.5 rounded-lg transition-colors ${
                                k.disabled ? 'text-green-500 hover:bg-green-50 dark:hover:bg-green-900/20' : 'text-gray-400 hover:text-orange-500 hover:bg-orange-50 dark:hover:bg-orange-900/20'
                              }`} onClick={() => handleToggleKey(k.id, k.disabled)} title={k.disabled ? t('accounts.enable_key') : t('accounts.disable_key')}>
                                {k.disabled ? <Power className="w-3.5 h-3.5" /> : <PowerOff className="w-3.5 h-3.5" />}
                              </button>
                              <button className="p-1.5 text-gray-400 hover:text-red-500 rounded-lg hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors" onClick={() => selectedPlatformId && setDeleteConfirm({ type: 'key', id: k.id, platformId: selectedPlatformId })} title={t('common.delete')}>
                                <Trash2 className="w-3.5 h-3.5" />
                              </button>
                            </div>
                          </div>
                          {/* Key value */}
                          <div className="mt-2">
                            <div className="font-mono text-xs bg-gray-50 dark:bg-base-300 px-2 py-1.5 rounded text-gray-700 dark:text-gray-300 break-all">
                              {showKeyValues.has(k.id) ? k.key_value : `${k.key_value.substring(0, 8)}...${k.key_value.substring(k.key_value.length - 4)}`}
                            </div>
                          </div>
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Dialogs */}
      <AddPlatformDialog open={showAddPlatform} onClose={() => setShowAddPlatform(false)} />
      {selectedPlatformId && (
        <>
          <AddKeyDialog open={showAddKey} onClose={() => setShowAddKey(false)} platformId={selectedPlatformId} />
          <AddModelDialog open={showAddModel} onClose={() => setShowAddModel(false)} platformId={selectedPlatformId} />
          <AssignKeyDialog open={assigningModelId !== null} onClose={() => setAssigningModelId(null)} modelId={assigningModelId || ''} platformId={selectedPlatformId} />
        </>
      )}
      <EditModelQuotaDialog open={showEditQuota} onClose={() => { setShowEditQuota(false); setEditingModel(null); }} model={editingModel} />
      {selectedPlatformId && (
        <TestModelDialog
          open={testDialog !== null}
          onClose={() => setTestDialog(null)}
          platformId={selectedPlatformId}
          modelName={testDialog?.modelName || ''}
          displayName={testDialog?.displayName || ''}
          testModel={testModel}
        />
      )}

      {/* Delete Confirmation Modal */}
      <ModalDialog
        isOpen={deleteConfirm !== null}
        title={
          deleteConfirm?.type === 'platform'
            ? t('accounts.confirm_delete_platform')
            : deleteConfirm?.type === 'key'
              ? t('accounts.confirm_delete_key')
              : t('accounts.confirm_delete_model')
        }
        message={
          deleteConfirm?.type === 'platform'
            ? t('accounts.delete_platform_warning', '此操作将同时删除该平台下的所有 Key 和模型，不可恢复。')
            : t('accounts.delete_item_warning', '此操作不可恢复。')
        }
        type="confirm"
        isDestructive={true}
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteConfirm(null)}
        confirmText={t('common.delete')}
        cancelText={t('common.cancel')}
      />
    </div>
  );
}

// ---- Dialog: Add Platform (kept for modularity) ----
function AddPlatformDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const { addPlatform } = usePlatformStore();
  const [name, setName] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [pathPrefix, setPathPrefix] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => { if (open) { setName(''); setBaseUrl(''); setPathPrefix(''); } }, [open]);

  const handleSave = async () => {
    if (!name.trim() || !baseUrl.trim() || !pathPrefix.trim()) {
      showToast(t('accounts.fill_all_fields'), 'error');
      return;
    }
    setSaving(true);
    try {
      await addPlatform(name.trim(), baseUrl.trim(), pathPrefix.trim());
      showToast(t('common.success'), 'success');
      onClose();
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally { setSaving(false); }
  };

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={onClose}>
      <div className="bg-white dark:bg-base-100 rounded-xl shadow-xl w-full max-w-md mx-4 p-5 border border-gray-200 dark:border-base-300" onClick={e => e.stopPropagation()}>
        <div className="flex justify-between items-center mb-4">
          <h3 className="font-bold text-gray-900 dark:text-base-content">{t('accounts.add_platform')}</h3>
          <button className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300" onClick={onClose}><X className="w-5 h-5" /></button>
        </div>
        <div className="space-y-3">
          <div>
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('accounts.platform_name')}</label>
            <input className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={name} onChange={e => setName(e.target.value)} placeholder="e.g. OpenAI" />
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('accounts.base_url')}</label>
            <input className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={baseUrl} onChange={e => setBaseUrl(e.target.value)} placeholder="https://api.openai.com" />
          </div>
          <div>
            <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('accounts.path_prefix')}</label>
            <input className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-blue-500 outline-none" value={pathPrefix} onChange={e => setPathPrefix(e.target.value.replace(/[\s\/]/g, ''))} placeholder="openai" />
            <p className="text-xs text-gray-400 mt-1">{t('accounts.path_prefix_hint').replace('{prefix}', pathPrefix || 'openai')}</p>
          </div>
        </div>
        <div className="flex justify-end gap-2 mt-5">
          <button className="px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-base-300 rounded-lg transition-colors" onClick={onClose}>{t('common.cancel')}</button>
          <button className={`px-4 py-2 text-sm text-white rounded-lg transition-colors ${saving ? 'bg-blue-400' : 'bg-blue-500 hover:bg-blue-600'}`} onClick={handleSave} disabled={saving}>{saving ? t('common.saving') : t('common.save')}</button>
        </div>
      </div>
    </div>
  );
}

export default Accounts;
