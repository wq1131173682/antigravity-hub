import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { usePlatformStore } from '../../stores/usePlatformStore';
import { Power, PowerOff, ArrowUpDown, Save, Trash2, Bookmark, Plus, X, Edit3, Layers, Server } from 'lucide-react';
import { showToast } from '../common/ToastContainer';
import ModalDialog from '../common/ModalDialog';
import * as profileService from '../../services/profileService';

export default function ProviderProfiles() {
  const { t } = useTranslation();
  const { platforms, models, fetchPlatforms, fetchModels } = usePlatformStore();
  const [profiles, setProfiles] = useState<profileService.ProviderProfile[]>([]);
  const [loading, setLoading] = useState(true);
  const [showForm, setShowForm] = useState(false);
  const [editId, setEditId] = useState<string | null>(null);
  const [formName, setFormName] = useState('');
  const [formPlatformId, setFormPlatformId] = useState('');
  const [formModelId, setFormModelId] = useState('');
  const [formNotes, setFormNotes] = useState('');
  const [formActive, setFormActive] = useState(false);
  const [formPriority, setFormPriority] = useState('0');
  const [formStrategy, setFormStrategy] = useState('failover');
  const [saving, setSaving] = useState(false);
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);

  const loadProfiles = async () => {
    try {
      const data = await profileService.listProfiles();
      setProfiles(data);
    } catch {
      // silently fail
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchPlatforms();
    loadProfiles();
  }, []);

  useEffect(() => {
    if (formPlatformId) {
      fetchModels(formPlatformId);
    }
  }, [formPlatformId]);

  const platformModels = formPlatformId ? (models[formPlatformId] || []) : [];

  const handleNew = () => {
    setEditId(null);
    setFormName('');
    setFormPlatformId(platforms[0]?.id || '');
    setFormModelId('');
    setFormNotes('');
    setFormActive(false);
    setFormPriority('0');
    setFormStrategy('failover');
    setShowForm(true);
  };

  const handleEdit = (profile: profileService.ProviderProfile) => {
    setEditId(profile.id);
    setFormName(profile.name);
    setFormPlatformId(profile.platform_id);
    setFormModelId(profile.model_id);
    setFormNotes(profile.notes || '');
    setFormActive(profile.active);
    setFormPriority(String(profile.priority ?? 0));
    setFormStrategy(profile.rotation_strategy || 'failover');
    setShowForm(true);
  };

  const handleSave = async () => {
    if (!formName.trim()) {
      showToast(t('profiles.name_required'), 'error');
      return;
    }
    if (!formPlatformId || !formModelId) {
      showToast(t('profiles.select_required'), 'error');
      return;
    }
    setSaving(true);
    try {
      await profileService.saveProfile({
        id: editId || undefined,
        name: formName.trim(),
        platformId: formPlatformId,
        modelId: formModelId,
        notes: formNotes.trim() || undefined,
        active: formActive,
        priority: parseInt(formPriority) || 0,
        rotationStrategy: formStrategy,
      });
      showToast(t('common.success'), 'success');
      setShowForm(false);
      loadProfiles();
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!deleteConfirmId) return;
    try {
      await profileService.deleteProfile(deleteConfirmId);
      showToast(t('common.delete_success'), 'success');
      setDeleteConfirmId(null);
      loadProfiles();
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    }
  };

  const handleToggleActive = async (profile: profileService.ProviderProfile) => {
    try {
      await profileService.toggleProfile(profile.id, !profile.active);
      showToast(profile.active ? t('profiles.disabled_toast') : t('profiles.enabled_toast'), 'success');
      loadProfiles();
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    }
  };

  const selectedPlatform = platforms.find(p => p.id === formPlatformId);

  return (
    <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <Bookmark className="w-4 h-4 text-violet-500" />
          <h2 className="font-semibold text-gray-900 dark:text-base-content">
            {t('profiles.title')}
          </h2>
        </div>
        <button
          className="px-3 py-1.5 bg-violet-500 text-white text-xs font-medium rounded-lg hover:bg-violet-600 transition-colors flex items-center gap-1.5 shadow-sm"
          onClick={handleNew}
        >
          <Plus className="w-3.5 h-3.5" />
          {t('profiles.add')}
        </button>
      </div>

      <p className="text-xs text-gray-400 dark:text-gray-500 mb-4">
        {t('profiles.description')}
      </p>

      {/* Profile List */}
      {loading ? (
        <div className="text-sm text-gray-400 text-center py-6">{t('common.loading')}</div>
      ) : profiles.length === 0 ? (
        <div className="text-sm text-gray-400 text-center py-6">
          <Bookmark className="w-8 h-8 mx-auto mb-2 opacity-40" />
          {t('profiles.empty')}
        </div>
      ) : (
        <div className="space-y-2">
          {profiles.map(profile => {
            const platform = platforms.find(p => p.id === profile.platform_id);
            const model = platform ? (models[profile.platform_id] || []).find(m => m.id === profile.model_id) : null;
            return (
              <div
                key={profile.id}
                className="flex items-center justify-between px-3 py-2.5 bg-gray-50 dark:bg-base-200/50 rounded-lg border border-gray-100 dark:border-base-300 hover:border-violet-200 dark:hover:border-violet-800/50 transition-all group"
              >
                <div className="flex items-center gap-3 min-w-0">
                  <button
                    onClick={() => handleToggleActive(profile)}
                    className={`p-1 rounded-lg transition-colors shrink-0 ${
                      profile.active
                        ? 'text-green-500 bg-green-50 dark:bg-green-900/20 hover:bg-green-100 dark:hover:bg-green-900/30'
                        : 'text-gray-400 bg-gray-100 dark:bg-base-300 hover:bg-gray-200 dark:hover:bg-base-200'
                    }`}
                    title={profile.active ? t('profiles.active') : t('profiles.inactive')}
                  >
                    {profile.active ? <Power className="w-3.5 h-3.5" /> : <PowerOff className="w-3.5 h-3.5" />}
                  </button>
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5 flex-wrap">
                      <span className="text-sm font-medium text-gray-800 dark:text-gray-200 truncate">
                        {profile.name}
                      </span>
                      {profile.active && (
                        <span className="px-1.5 py-0.5 text-[10px] font-medium bg-green-50 dark:bg-green-900/20 text-green-600 dark:text-green-400 rounded">
                          {t('profiles.active')}
                        </span>
                      )}
                      <span className="flex items-center gap-0.5 px-1.5 py-0.5 text-[10px] text-gray-400">
                        <ArrowUpDown className="w-2.5 h-2.5" />
                        P{profile.priority}
                      </span>
                    </div>
                    <div className="flex items-center gap-1.5 mt-0.5">
                      {platform && (
                        <span className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-mono bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 rounded">
                          <Server className="w-2.5 h-2.5" />
                          {platform.name}
                        </span>
                      )}
                      {model && (
                        <span className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-mono bg-purple-50 dark:bg-purple-900/20 text-purple-600 dark:text-purple-400 rounded">
                          <Layers className="w-2.5 h-2.5" />
                          {model.display_name || model.model_name}
                        </span>
                      )}
                      {profile.rotation_strategy === 'failover' && (
                        <span className="px-1.5 py-0.5 text-[10px] font-mono bg-amber-50 dark:bg-amber-900/20 text-amber-600 dark:text-amber-400 rounded">
                          Failover
                        </span>
                      )}
                    </div>
                    {profile.notes && (
                      <p className="text-[11px] text-gray-400 dark:text-gray-500 mt-0.5 truncate max-w-xs">
                        {profile.notes}
                      </p>
                    )}
                  </div>
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  <button
                    className="p-1.5 text-gray-400 hover:text-violet-500 rounded-lg hover:bg-violet-50 dark:hover:bg-violet-900/20 transition-colors opacity-0 group-hover:opacity-100"
                    onClick={() => handleEdit(profile)}
                    title={t('common.edit')}
                  >
                    <Edit3 className="w-3.5 h-3.5" />
                  </button>
                  <button
                    className="p-1.5 text-gray-400 hover:text-red-500 rounded-lg hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors opacity-0 group-hover:opacity-100"
                    onClick={() => setDeleteConfirmId(profile.id)}
                    title={t('common.delete')}
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Save / Edit Form Modal */}
      {showForm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={() => setShowForm(false)}>
          <div className="bg-white dark:bg-base-100 rounded-xl shadow-xl w-full max-w-md mx-4 p-5 border border-gray-200 dark:border-base-300" onClick={e => e.stopPropagation()}>
            <div className="flex justify-between items-center mb-4">
              <h3 className="font-bold text-gray-900 dark:text-base-content">
                {editId ? t('profiles.edit') : t('profiles.add')}
              </h3>
              <button className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300" onClick={() => setShowForm(false)}>
                <X className="w-5 h-5" />
              </button>
            </div>
            <div className="space-y-3">
              {/* Profile Name */}
              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('profiles.name')}</label>
                <input
                  className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-violet-500 outline-none"
                  value={formName}
                  onChange={e => setFormName(e.target.value)}
                  placeholder={t('profiles.name_placeholder')}
                />
              </div>

              {/* Platform Select */}
              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('profiles.platform')}</label>
                <select
                  className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-violet-500 outline-none"
                  value={formPlatformId}
                  onChange={e => { setFormPlatformId(e.target.value); setFormModelId(''); }}
                >
                  <option value="">{t('profiles.select_platform')}</option>
                  {platforms.map(p => (
                    <option key={p.id} value={p.id}>{p.name} ({p.path_prefix})</option>
                  ))}
                </select>
              </div>

              {/* Model Select */}
              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('profiles.model')}</label>
                <select
                  className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-violet-500 outline-none"
                  value={formModelId}
                  onChange={e => setFormModelId(e.target.value)}
                  disabled={!formPlatformId}
                >
                  <option value="">{t('profiles.select_model')}</option>
                  {platformModels.map(m => (
                    <option key={m.id} value={m.id}>{m.display_name || m.model_name}</option>
                  ))}
                </select>
              </div>

              {/* Active toggle */}
              <div className="flex items-center justify-between">
                <label className="text-xs font-medium text-gray-500 dark:text-gray-400">{t('profiles.enable_rotation')}</label>
                <label className="relative inline-flex items-center cursor-pointer">
                  <input
                    type="checkbox"
                    className="sr-only peer"
                    checked={formActive}
                    onChange={e => setFormActive(e.target.checked)}
                  />
                  <div className="w-8 h-4.5 bg-gray-200 dark:bg-base-300 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-violet-300 rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-3.5 after:w-3.5 after:transition-all peer-checked:bg-violet-500" />
                </label>
              </div>

              {/* Rotation Strategy */}
              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('profiles.rotation_strategy')}</label>
                <select
                  className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-violet-500 outline-none"
                  value={formStrategy}
                  onChange={e => setFormStrategy(e.target.value)}
                >
                  <option value="failover">{t('profiles.strategy_failover')}</option>
                  <option value="none">{t('profiles.strategy_none')}</option>
                </select>
              </div>

              {/* Priority */}
              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('profiles.priority')}</label>
                <input
                  type="number"
                  min="0"
                  className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-violet-500 outline-none font-mono"
                  value={formPriority}
                  onChange={e => setFormPriority(e.target.value)}
                  placeholder="0"
                />
              </div>

              {/* Notes */}
              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">{t('profiles.notes')}</label>
                <textarea
                  className="w-full px-3 py-2 text-sm border border-gray-200 dark:border-base-300 rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:ring-2 focus:ring-violet-500 outline-none resize-none"
                  rows={2}
                  value={formNotes}
                  onChange={e => setFormNotes(e.target.value)}
                  placeholder={t('profiles.notes_placeholder')}
                />
              </div>

              {/* Preview */}
              {selectedPlatform && formModelId && (() => {
                const model = platformModels.find(m => m.id === formModelId);
                return model ? (
                  <div className="px-3 py-2 bg-violet-50 dark:bg-violet-900/20 rounded-lg text-xs text-violet-700 dark:text-violet-300">
                    <span className="font-medium">{selectedPlatform.name}</span>
                    <span className="mx-1">/</span>
                    <span className="font-mono">{model.model_name}</span>
                  </div>
                ) : null;
              })()}
            </div>
            <div className="flex justify-end gap-2 mt-5">
              <button
                className="px-4 py-2 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-base-300 rounded-lg transition-colors"
                onClick={() => setShowForm(false)}
              >
                {t('common.cancel')}
              </button>
              <button
                className={`px-4 py-2 text-sm text-white rounded-lg transition-colors flex items-center gap-1.5 ${
                  saving ? 'bg-violet-400' : 'bg-violet-500 hover:bg-violet-600'
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
      )}

      {/* Delete Confirmation */}
      <ModalDialog
        isOpen={deleteConfirmId !== null}
        title={t('profiles.delete_confirm_title')}
        message={t('profiles.delete_confirm_message')}
        type="confirm"
        isDestructive={true}
        onConfirm={handleDelete}
        onCancel={() => setDeleteConfirmId(null)}
        confirmText={t('common.delete')}
        cancelText={t('common.cancel')}
      />
    </div>
  );
}
