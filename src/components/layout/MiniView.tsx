import { useCallback, useEffect, useRef, useState } from 'react';
import { Maximize2, RefreshCw, Power, PowerOff, Activity, ArrowRightLeft } from 'lucide-react';
import { motion } from 'framer-motion';
import clsx from 'clsx';
import { useTranslation } from 'react-i18next';
import { getVersion } from '@tauri-apps/api/app';

import { useViewStore } from '../../stores/useViewStore';
import { usePlatformStore } from '../../stores/usePlatformStore';
import { isTauri } from '../../utils/env';
import { formatCompactNumber } from '../../utils/format';
import { enterMiniMode, exitMiniMode } from '../../utils/windowManager';
import { getTokenStats, type TokenStats } from '../../services/platformService';

/**
 * Mini View — 300px always-on-top widget: proxy state at a glance.
 *
 * Deliberately minimal: running status, start/stop, and two live counters.
 * (The previous version also carried per-platform bars, a rotation feed and an
 * address-copy row; those belong on the Dashboard, not in a 300px widget.)
 */

const MIN_HEIGHT = 160;
const MAX_HEIGHT = 260;
const POLL_MS = 10000;

export default function MiniView() {
    const { t } = useTranslation();
    const { setMiniView } = useViewStore();
    const { proxyRunning, fetchProxyStatus, startProxy, stopProxy } = usePlatformStore();

    const containerRef = useRef<HTMLDivElement>(null);
    const [stats, setStats] = useState<TokenStats | null>(null);
    const [busy, setBusy] = useState(false);
    const [refreshing, setRefreshing] = useState(false);
    const [version, setVersion] = useState('');

    const refresh = useCallback(async () => {
        fetchProxyStatus();
        if (!isTauri()) return;
        try {
            setStats(await getTokenStats());
        } catch (e) {
            console.error('[MiniView] stats failed:', e);
        }
    }, [fetchProxyStatus]);

    // Load once, then poll lightly — counters change as requests flow in.
    useEffect(() => {
        void refresh();
        const id = setInterval(() => void refresh(), POLL_MS);
        return () => clearInterval(id);
    }, [refresh]);

    useEffect(() => {
        if (isTauri()) getVersion().then(setVersion).catch(() => setVersion(''));
    }, []);

    // Keep the window snug to content without ever collapsing to a stub.
    useEffect(() => {
        if (!isTauri()) return;
        const timer = setTimeout(async () => {
            const h = containerRef.current?.scrollHeight ?? MIN_HEIGHT;
            await enterMiniMode(Math.min(Math.max(h, MIN_HEIGHT), MAX_HEIGHT));
        }, 60);
        return () => clearTimeout(timer);
    }, [proxyRunning, stats]);

    const handleRefresh = async () => {
        setRefreshing(true);
        try { await refresh(); } finally { setTimeout(() => setRefreshing(false), 400); }
    };

    const handleToggleProxy = async () => {
        setBusy(true);
        try {
            if (proxyRunning) await stopProxy();
            else await startProxy();
            fetchProxyStatus();
        } catch (e) {
            console.error('[MiniView] proxy toggle failed:', e);
        } finally {
            setBusy(false);
        }
    };

    const handleMaximize = async () => {
        await exitMiniMode();
        setMiniView(false);
    };

    return (
        <div className="h-dvh w-full flex items-center justify-center bg-transparent">
            <motion.div
                ref={containerRef}
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.95 }}
                className="w-[300px] flex flex-col bg-white/90 dark:bg-[#121212]/90 backdrop-blur-md shadow-2xl overflow-hidden border border-gray-200/60 dark:border-white/10 sm:rounded-2xl"
            >
                {/* Header / drag region */}
                <div
                    data-tauri-drag-region
                    className="flex-none flex items-center justify-between px-3 py-2 bg-gray-50/60 dark:bg-white/5 border-b border-gray-100 dark:border-white/5 select-none"
                >
                    <div className="flex items-center gap-2 min-w-0">
                        <span className={clsx(
                            'w-2 h-2 rounded-full shrink-0',
                            proxyRunning
                                ? 'bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.5)]'
                                : 'bg-gray-400',
                        )} />
                        <span className="text-xs font-semibold text-gray-900 dark:text-white truncate">
                            {proxyRunning ? t('mini.running') : t('mini.stopped')}
                        </span>
                        {version && (
                            <span className="text-[10px] font-mono text-gray-400 dark:text-gray-500">
                                v{version}
                            </span>
                        )}
                    </div>
                    <div className="flex items-center gap-0.5 shrink-0 no-drag" onMouseDown={e => e.stopPropagation()}>
                        <button
                            onClick={() => void handleRefresh()}
                            className="p-1.5 rounded-lg hover:bg-gray-200/60 dark:hover:bg-white/10 transition-colors"
                            title={t('common.refresh')}
                        >
                            <RefreshCw size={13} className={clsx(refreshing && 'animate-spin text-blue-500')} />
                        </button>
                        <button
                            onClick={() => void handleMaximize()}
                            className="p-1.5 rounded-lg hover:bg-gray-200/60 dark:hover:bg-white/10 transition-colors text-gray-500 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white"
                            title={t('common.maximize')}
                        >
                            <Maximize2 size={13} />
                        </button>
                    </div>
                </div>

                {/* Body */}
                <div className="flex-1 p-3 space-y-2.5">
                    <button
                        onClick={() => void handleToggleProxy()}
                        disabled={busy}
                        className={clsx(
                            'w-full flex items-center justify-center gap-1.5 py-2 rounded-xl text-xs font-semibold transition-colors disabled:opacity-60',
                            proxyRunning
                                ? 'bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-900/30'
                                : 'bg-emerald-500 text-white hover:bg-emerald-600',
                        )}
                    >
                        {proxyRunning ? <PowerOff size={13} /> : <Power size={13} />}
                        {proxyRunning ? t('mini.stop_proxy') : t('mini.start_proxy')}
                    </button>

                    {/* Live session counters */}
                    <div className="grid grid-cols-2 gap-2">
                        <Metric
                            icon={ArrowRightLeft}
                            label={t('mini.requests')}
                            value={formatCompactNumber(stats?.request_count ?? 0)}
                        />
                        <Metric
                            icon={Activity}
                            label={t('mini.tokens')}
                            value={formatCompactNumber(stats?.total_tokens ?? 0)}
                        />
                    </div>
                </div>
            </motion.div>
        </div>
    );
}

/** One compact counter tile. */
function Metric({ icon: Icon, label, value }: { icon: React.ElementType; label: string; value: string }) {
    return (
        <div className="flex items-center gap-2 rounded-lg px-2.5 py-2 bg-gray-50 dark:bg-white/5 border border-gray-100 dark:border-white/5 min-w-0">
            <Icon size={14} className="text-gray-400 dark:text-gray-500 shrink-0" />
            <div className="min-w-0">
                <div className="text-[9px] uppercase tracking-wide text-gray-400 dark:text-gray-500 truncate">
                    {label}
                </div>
                <div className="text-sm font-bold text-gray-900 dark:text-base-content tabular-nums leading-tight">
                    {value}
                </div>
            </div>
        </div>
    );
}
