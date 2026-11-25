import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowLeftRight, Plus, Power, Square, Trash2, Terminal, X } from 'lucide-react';
import { showToast } from '../components/common/ToastContainer';
import { listen } from '@tauri-apps/api/event';
import { isTauri } from '../utils/env';
import * as tunnelService from '../services/tunnelService';
import type { TunnelConfig, TunnelStatus } from '../types/tunnel';

interface FormState {
  label: string;
  localPort: string;
  bindAddress: string;
  user: string;
  bastionHost: string;
  bastionPort: string;
  targetHost: string;
  targetPort: string;
  identityFile: string;
  password: string;
  extraArgs: string;
}

const EMPTY_FORM: FormState = {
  label: '',
  localPort: '',
  bindAddress: '127.0.0.1',
  user: '',
  bastionHost: '',
  bastionPort: '22',
  targetHost: '',
  targetPort: '',
  identityFile: '',
  password: '',
  extraArgs: '',
};

/** "3h 12m" style uptime for a running tunnel. */
function formatUptime(startedAtMs: number | null): string | null {
  if (!startedAtMs) return null;
  const secs = Math.max(0, Math.floor((Date.now() - startedAtMs) / 1000));
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

function TunnelCard({
  tunnel, status, busy, onStart, onStop, onDelete,
}: {
  tunnel: TunnelConfig;
  status: TunnelStatus;
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const cmd = tunnelService.sshCommandPreview(tunnel);
  const uptime = formatUptime(status.started_at_ms);

  const badge = status.running
    ? { text: t('tunnels.status_running'), cls: 'bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-300' }
    : status.error
      ? { text: t('tunnels.status_error'), cls: 'bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300' }
      : { text: t('tunnels.status_stopped'), cls: 'bg-gray-100 text-gray-600 dark:bg-base-200 dark:text-gray-400' };

  const title = tunnel.label || `${tunnel.user}@${tunnel.bastion_host}`;

  return (
    <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="font-semibold text-gray-900 dark:text-base-content truncate">{title}</span>
            <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${badge.cls}`}>
              {badge.text}
            </span>
            {status.running && uptime ? (
              <span className="text-xs text-gray-400">{t('tunnels.uptime', { time: uptime })}</span>
            ) : null}
          </div>
          <p className="text-xs text-gray-500 dark:text-gray-400 mt-1 truncate">
            {tunnel.bind_address || '127.0.0.1'}:{tunnel.local_port}
            <span className="mx-1.5 text-gray-300 dark:text-base-300">→</span>
            <span className="text-gray-400">{t('tunnels.via')} {tunnel.user}@{tunnel.bastion_host}:{tunnel.bastion_port}</span>
            <span className="mx-1.5 text-gray-300 dark:text-base-300">→</span>
            {tunnel.target_host}:{tunnel.target_port}
          </p>
          <div className="flex items-center gap-1.5 mt-2 text-xs font-mono text-gray-500 dark:text-gray-400 bg-gray-50 dark:bg-base-200 rounded-lg px-2.5 py-1.5 overflow-x-auto">
            <Terminal className="w-3.5 h-3.5 shrink-0" />
            <span className="whitespace-nowrap truncate" title={cmd}>{cmd}</span>
          </div>
          {status.error ? <p className="text-xs text-red-500 mt-2 whitespace-pre-wrap break-all">{status.error}</p> : null}
        </div>
        <div className="flex items-center gap-1.5 shrink-0">
          {status.running ? (
            <button
              onClick={onStop}
              disabled={busy}
              className="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg bg-gray-100 dark:bg-base-200 text-gray-700 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-base-300 disabled:opacity-50 transition-colors"
            >
              <Square className="w-3.5 h-3.5" />
              {t('tunnels.stop')}
            </button>
          ) : (
            <button
              onClick={onStart}
              disabled={busy}
              className="flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50 transition-colors shadow-sm"
            >
              <Power className="w-3.5 h-3.5" />
              {t('tunnels.start')}
            </button>
          )}
          <button
            onClick={onDelete}
            disabled={busy}
            className="p-1.5 text-gray-400 hover:text-red-500 disabled:opacity-50 transition-colors rounded-lg"
            title={t('tunnels.delete')}
          >
            <Trash2 className="w-4 h-4" />
          </button>
        </div>
      </div>
    </div>
  );
}

export default function Tunnels() {
  const { t } = useTranslation();
  const [tunnels, setTunnels] = useState<TunnelConfig[]>([]);
  const [statuses, setStatuses] = useState<Record<string, TunnelStatus>>({});
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [saving, setSaving] = useState(false);
  const [busyIds, setBusyIds] = useState<Record<string, boolean>>({});
  const [deleteTarget, setDeleteTarget] = useState<TunnelConfig | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);

  const refresh = async () => {
    try {
      const [list, st] = await Promise.all([
        tunnelService.listTunnels(),
        tunnelService.getTunnelStatuses(),
      ]);
      setTunnels(list);
      const map: Record<string, TunnelStatus> = {};
      st.forEach(s => { map[s.id] = s; });
      setStatuses(map);
    } catch {
      /* backend not ready yet */
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  // The ssh watcher emits 'tunnel:status' on every tunnel process exit.
  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    const setup = async () => {
      const unlisten = await listen<TunnelStatus>('tunnel:status', (event) => {
        if (cancelled) return;
        const s = event.payload;
        setStatuses(prev => ({ ...prev, [s.id]: s }));
        if (!s.running && s.error) {
          showToast(`${t('tunnels.exited')} ${s.error}`, 'error', 6000);
        }
      });
      unlistenRef.current = unlisten;
    };
    setup();
    return () => {
      cancelled = true;
      unlistenRef.current?.();
      unlistenRef.current = null;
    };
  }, [t]);

  const handleAdd = async () => {
    const lp = parseInt(form.localPort, 10);
    const tp = parseInt(form.targetPort, 10);
    const bp = form.bastionPort === '' ? 22 : parseInt(form.bastionPort, 10);
    if (!form.bastionHost.trim() || !form.user.trim() || !form.targetHost.trim()) {
      showToast(t('tunnels.fill_fields'), 'error');
      return;
    }
    if (isNaN(lp) || lp < 1 || lp > 65535 || isNaN(tp) || tp < 1 || tp > 65535 || isNaN(bp) || bp < 1 || bp > 65535) {
      showToast(t('tunnels.port_invalid'), 'error');
      return;
    }
    setSaving(true);
    try {
      await tunnelService.addTunnel({
        label: form.label,
        localPort: lp,
        user: form.user,
        bastionHost: form.bastionHost,
        bastionPort: form.bastionPort === '' ? undefined : bp,
        targetHost: form.targetHost,
        targetPort: tp,
        bindAddress: form.bindAddress,
        identityFile: form.identityFile,
        extraArgs: form.extraArgs,
        password: form.password,
      });
      showToast(t('tunnels.added'), 'success');
      setForm(EMPTY_FORM);
      setShowForm(false);
      await refresh();
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally {
      setSaving(false);
    }
  };

  const handleStart = async (id: string) => {
    setBusyIds(prev => ({ ...prev, [id]: true }));
    try {
      await tunnelService.startTunnel(id);
      showToast(t('tunnels.started'), 'success');
      await refresh();
    } catch (e) {
      showToast(`${t('tunnels.start_failed')} ${e}`, 'error');
      await refresh();
    } finally {
      setBusyIds(prev => ({ ...prev, [id]: false }));
    }
  };

  const handleStop = async (id: string) => {
    setBusyIds(prev => ({ ...prev, [id]: true }));
    try {
      await tunnelService.stopTunnel(id);
      showToast(t('tunnels.stopped'), 'success');
      await refresh();
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
      await refresh();
    } finally {
      setBusyIds(prev => ({ ...prev, [id]: false }));
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    const id = deleteTarget.id;
    setDeleteTarget(null);
    setBusyIds(prev => ({ ...prev, [id]: true }));
    try {
      await tunnelService.deleteTunnel(id);
      showToast(t('common.success'), 'success');
      await refresh();
    } catch (e) {
      showToast(`${t('common.error')}: ${e}`, 'error');
    } finally {
      setBusyIds(prev => ({ ...prev, [id]: false }));
    }
  };

  const inputCls = 'w-full px-3 py-2 text-sm border rounded-lg bg-white dark:bg-base-200 text-gray-900 dark:text-base-content focus:outline-none focus:ring-2 focus:ring-blue-500 border-gray-200 dark:border-base-300';
  const labelCls = 'text-xs text-gray-500 dark:text-gray-400 mb-1 block';

  return (
    <div className="h-full w-full overflow-y-auto">
      <div className="p-5 space-y-4 max-w-3xl mx-auto">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-2xl font-bold text-gray-900 dark:text-base-content flex items-center gap-2">
              <ArrowLeftRight className="w-6 h-6 text-blue-500" />
              {t('tunnels.title')}
            </h1>
            <p className="text-xs text-gray-400 mt-1">{t('tunnels.subtitle')}</p>
          </div>
          <button
            onClick={() => setShowForm(v => !v)}
            className="flex items-center gap-1.5 px-4 py-2 text-sm font-medium rounded-lg bg-blue-500 text-white hover:bg-blue-600 shadow-sm transition-colors"
          >
            {showForm ? <X className="w-4 h-4" /> : <Plus className="w-4 h-4" />}
            {t('tunnels.add')}
          </button>
        </div>

        {showForm ? (
          <div className="bg-white dark:bg-base-100 rounded-xl p-5 shadow-sm border border-gray-100 dark:border-base-200">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div className="sm:col-span-2">
                <label className={labelCls}>{t('tunnels.label')}</label>
                <input className={inputCls} value={form.label} onChange={e => setForm({ ...form, label: e.target.value })} placeholder="e.g. 内网数据库" />
              </div>
              <div>
                <label className={labelCls}>{t('tunnels.local_port')}</label>
                <input className={inputCls} type="number" value={form.localPort} onChange={e => setForm({ ...form, localPort: e.target.value })} placeholder="16201" />
              </div>
              <div>
                <label className={labelCls}>{t('tunnels.bind_address')}</label>
                <input className={inputCls} value={form.bindAddress} onChange={e => setForm({ ...form, bindAddress: e.target.value })} placeholder="127.0.0.1" />
              </div>
              <div>
                <label className={labelCls}>{t('tunnels.bastion_host')}</label>
                <input className={inputCls} value={form.bastionHost} onChange={e => setForm({ ...form, bastionHost: e.target.value })} placeholder="192.168.2.72" />
              </div>
              <div>
                <label className={labelCls}>{t('tunnels.user')}</label>
                <input className={inputCls} value={form.user} onChange={e => setForm({ ...form, user: e.target.value })} placeholder="root" />
              </div>
              <div>
                <label className={labelCls}>{t('tunnels.bastion_port')}</label>
                <input className={inputCls} type="number" value={form.bastionPort} onChange={e => setForm({ ...form, bastionPort: e.target.value })} placeholder="22" />
              </div>
              <div>
                <label className={labelCls}>{t('tunnels.target_host')}</label>
                <input className={inputCls} value={form.targetHost} onChange={e => setForm({ ...form, targetHost: e.target.value })} placeholder="10.16.16.15" />
              </div>
              <div>
                <label className={labelCls}>{t('tunnels.target_port')}</label>
                <input className={inputCls} type="number" value={form.targetPort} onChange={e => setForm({ ...form, targetPort: e.target.value })} placeholder="6201" />
              </div>
              <div>
                <label className={labelCls}>{t('tunnels.identity_file')}</label>
                <input className={inputCls} value={form.identityFile} onChange={e => setForm({ ...form, identityFile: e.target.value })} placeholder="~/.ssh/id_ed25519" />
              </div>              <div>
                <label className={labelCls}>{t('tunnels.password')}</label>
                <input
                  className={inputCls}
                  type="password"
                  value={form.password}
                  onChange={e => setForm({ ...form, password: e.target.value })}
                  placeholder={t('tunnels.password_placeholder')}
                />
              </div>
              <div>
                <label className={labelCls}>{t('tunnels.extra_args')}</label>
                <input className={inputCls} value={form.extraArgs} onChange={e => setForm({ ...form, extraArgs: e.target.value })} placeholder={'-o ServerAliveInterval=60'} />
              </div>
            </div>
            <p className="text-xs text-gray-400 mt-3">{t('tunnels.ssh_hint')}</p>
            <div className="flex gap-2 mt-4">
              <button
                onClick={handleAdd}
                disabled={saving}
                className="px-4 py-2 text-sm font-medium rounded-lg bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50 transition-colors shadow-sm"
              >
                {saving ? t('common.saving') : t('common.save')}
              </button>
              <button
                onClick={() => { setForm(EMPTY_FORM); setShowForm(false); }}
                className="px-4 py-2 text-sm font-medium rounded-lg bg-gray-100 dark:bg-base-200 text-gray-700 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-base-300 transition-colors"
              >
                {t('common.cancel')}
              </button>
            </div>
          </div>
        ) : null}

        {tunnels.length === 0 ? (
          <div className="text-center py-12 text-gray-400">
            <ArrowLeftRight className="w-10 h-10 mx-auto mb-3 opacity-40" />
            <p className="text-sm font-medium">{t('tunnels.no_tunnels')}</p>
            <p className="text-xs mt-1">{t('tunnels.no_tunnels_desc')}</p>
          </div>
        ) : (
          <div className="space-y-3">
            {tunnels.map(tunnel => {
              const status = statuses[tunnel.id] ?? {
                id: tunnel.id, running: false, started_at_ms: null, exit_code: null, error: null,
              };
              return (
                <TunnelCard
                  key={tunnel.id}
                  tunnel={tunnel}
                  status={status}
                  busy={!!busyIds[tunnel.id]}
                  onStart={() => handleStart(tunnel.id)}
                  onStop={() => handleStop(tunnel.id)}
                  onDelete={() => setDeleteTarget(tunnel)}
                />
              );
            })}
          </div>
        )}

        {deleteTarget ? (
          <div
            className="fixed inset-0 z-[100] bg-black/40 backdrop-blur-sm flex items-center justify-center"
            onClick={() => setDeleteTarget(null)}
          >
            <div
              className="bg-white dark:bg-base-100 rounded-2xl p-6 max-w-sm w-full mx-4 shadow-2xl text-center"
              onClick={e => e.stopPropagation()}
            >
              <h3 className="text-lg font-bold text-gray-900 dark:text-base-content mb-2">
                {t('tunnels.confirm_delete_title')}
              </h3>
              <p className="text-sm text-gray-500 dark:text-gray-400 mb-5">
                {t('tunnels.confirm_delete', { label: deleteTarget.label || `${deleteTarget.user}@${deleteTarget.bastion_host}` })}
              </p>
              <div className="flex gap-3">
                <button
                  onClick={() => setDeleteTarget(null)}
                  className="flex-1 px-4 py-2.5 bg-gray-100 dark:bg-base-200 text-gray-700 dark:text-gray-300 font-medium rounded-xl hover:bg-gray-200 dark:hover:bg-base-300 transition-colors"
                >
                  {t('common.cancel')}
                </button>
                <button
                  onClick={handleDelete}
                  className="flex-1 px-4 py-2.5 bg-red-500 text-white font-medium rounded-xl hover:bg-red-600 shadow-md transition-colors"
                >
                  {t('common.delete')}
                </button>
              </div>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}