import { useEffect, useState, useRef } from 'react';
import { Maximize2, RefreshCw, Clock, ShieldAlert, Tag, Activity, History, Trash2, ChevronDown } from 'lucide-react';
import { useViewStore } from '../../stores/useViewStore';
import { useAccountStore } from '../../stores/useAccountStore';
import { isTauri } from '../../utils/env';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { motion, AnimatePresence } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import clsx from 'clsx';
import { formatTimeRemaining, formatCompactNumber } from '../../utils/format';
import { enterMiniMode, exitMiniMode } from '../../utils/windowManager';
import { getVersion } from '@tauri-apps/api/app';
import { listen } from '@tauri-apps/api/event';

interface ProxyRequestLog {
    id: string;
    platform_name?: string;
    platform_id?: string;
    model?: string;
    input_tokens?: number;
    output_tokens?: number;
    timestamp: number;
    status: number;
    duration: number;
    mapped_model?: string;
    in_progress?: boolean;
}

// Keep at most this many completed requests in the recent-request history.
const MAX_HISTORY = 20;

export default function MiniView() {
    const { setMiniView } = useViewStore();
    const { currentAccount, refreshQuota, fetchCurrentAccount } = useAccountStore();
    const { t } = useTranslation();
    const [isRefreshing, setIsRefreshing] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);
    const [appVersion, setAppVersion] = useState('0.0.0');
    const [latestLog, setLatestLog] = useState<ProxyRequestLog | null>(null);
    const [requestHistory, setRequestHistory] = useState<ProxyRequestLog[]>([]);
    const [expandedId, setExpandedId] = useState<string | null>(null);

    // Subscribe to proxy logs
    useEffect(() => {
        let unlistenFn: (() => void) | null = null;

        const setupListener = async () => {
            if (!isTauri()) return;
            try {
                unlistenFn = await listen<ProxyRequestLog>('proxy://request', (event) => {
                    const payload = event.payload;
                    setLatestLog(payload);
                    // Only completed requests are added to the history list.
                    // Each request has a unique id (start + completion share
                    // the same id), so dedupe and keep the most recent N.
                    if (!payload.in_progress) {
                        setRequestHistory(prev => {
                            const without = prev.filter(item => item.id !== payload.id);
                            return [payload, ...without].slice(0, MAX_HISTORY);
                        });
                    }
                });
            } catch (e) {
                console.error('Failed to setup log listener:', e);
            }
        };

        setupListener();

        return () => {
            if (unlistenFn) unlistenFn();
        };
    }, []);

    // Get app version
    useEffect(() => {
        const fetchVersion = async () => {
            if (isTauri()) {
                try {
                    const version = await getVersion();
                    setAppVersion(version);
                } catch (e) {
                    console.error('Failed to get app version:', e);
                }
            } else {
                // Fallback for web mode if needed, or import from package.json
                setAppVersion('4.2.1');
            }
        };
        fetchVersion();
    }, []);

    // Enter mini mode & Auto-resize based on content
    useEffect(() => {
        const adjustSize = async () => {
            if (isTauri() && containerRef.current) {
                // Get the content height
                const height = containerRef.current.scrollHeight;
                // Calculate content height for the utility (which adds 20px padding)
                // We want final height to be approx (scroll height - header adjustment)
                await enterMiniMode(height);
            }
        };

        // Run initially and whenever account data or the latest proxy log
        // (content) changes. Use a small timeout to ensure rendering is complete.
        const timer = setTimeout(adjustSize, 50);
        return () => clearTimeout(timer);
    }, [currentAccount, latestLog, requestHistory, expandedId]);

    const handleRefresh = async () => {
        if (!currentAccount || isRefreshing) return;
        setIsRefreshing(true);
        try {
            await refreshQuota(currentAccount.id);
            await fetchCurrentAccount();
        } finally {
            setTimeout(() => setIsRefreshing(false), 800);
        }
    };

    const handleMaximize = async () => {
        await exitMiniMode();
        setMiniView(false);
    };


    const handleMouseDown = () => {
        if (isTauri()) {
            getCurrentWindow().startDragging();
        }
    };

    // Format a millisecond timestamp as a compact local time string.
    const formatTime = (ts?: number) => {
        if (!ts) return '—';
        const d = new Date(ts);
        if (isNaN(d.getTime())) return '—';
        return d.toLocaleTimeString(undefined, {
            hour: '2-digit',
            minute: '2-digit',
            second: '2-digit',
        });
    };


    // Extract specific models to match AccountRow.tsx
    const geminiProModel = currentAccount?.quota?.models
        .filter(m =>
            m.name.toLowerCase() === 'gemini-3-pro-high'
            || m.name.toLowerCase() === 'gemini-3-pro-low'
            || m.name.toLowerCase() === 'gemini-3.1-pro-high'
            || m.name.toLowerCase() === 'gemini-3.1-pro-low'
        )
        .sort((a, b) => (a.percentage || 0) - (b.percentage || 0))[0];

    const geminiFlashModel = currentAccount?.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-flash');

    const claudeGroupNames = [
        'claude-opus-4-6-thinking',
        'claude'
    ];
    const claudeModel = currentAccount?.quota?.models
        .filter(m => claudeGroupNames.includes(m.name.toLowerCase()))
        .sort((a, b) => (a.percentage || 0) - (b.percentage || 0))[0];

    // Helper to render a model row
    const renderModelRow = (model: any, displayName: string, colorClass: string) => {
        if (!model) return null;

        // Determine status color based on percentage
        const getStatusColor = (p: number) => {
            if (p >= 50) return 'text-emerald-500';
            if (p >= 20) return 'text-amber-500';
            return 'text-rose-500';
        };

        const getBarColor = (p: number) => {
            if (p >= 50) return colorClass === 'cyan' ? 'bg-gradient-to-r from-cyan-400 to-cyan-500' : 'bg-gradient-to-r from-emerald-400 to-emerald-500';
            if (p >= 20) return colorClass === 'cyan' ? 'bg-gradient-to-r from-orange-400 to-orange-500' : 'bg-gradient-to-r from-amber-400 to-amber-500';
            return 'bg-gradient-to-r from-rose-400 to-rose-500';
        };

        return (
            <motion.div
                layout
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                className="space-y-1.5"
            >
                <div className="flex justify-between items-baseline">
                    <span className="text-xs font-medium text-gray-600 dark:text-gray-400">{displayName}</span>
                    <div className="flex items-center gap-2">
                        <span className="text-[10px] text-blue-600 dark:text-blue-400 font-mono">
                            {model.reset_time ? `R: ${formatTimeRemaining(model.reset_time)}` : t('common.unknown')}
                        </span>
                        <span className={clsx("text-xs font-bold", getStatusColor(model.percentage))}>
                            {model.percentage}%
                        </span>
                    </div>
                </div>
                <div className="w-full bg-gray-100 dark:bg-white/10 rounded-full h-1.5 overflow-hidden">
                    <motion.div
                        initial={{ width: 0 }}
                        animate={{ width: `${model.percentage}%` }}
                        transition={{ duration: 0.8, ease: "easeOut" }}
                        className={clsx("h-full rounded-full shadow-[0_0_8px_currentColor]", getBarColor(model.percentage))}
                    />
                </div>
            </motion.div>
        );
    };

    return (
        <div className="h-screen w-full flex items-center justify-center bg-transparent">
            {/* Main Container - 300px fixed width */}
            <motion.div
                ref={containerRef}
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.95 }}
                className="w-[300px] flex flex-col bg-white/80 dark:bg-[#121212]/80 backdrop-blur-md shadow-2xl overflow-hidden border-x border-y border-gray-200/50 dark:border-white/10 sm:rounded-2xl"
            >
                {/* Header / Drag Region */}
                <div
                    className="flex-none flex items-center justify-between px-4 py-1 bg-gray-50/50 dark:bg-white/5 border-b border-gray-100 dark:border-white/5 select-none"
                    onMouseDown={handleMouseDown}
                    data-tauri-drag-region
                >
                    <div className="flex items-center gap-2 text-sm font-semibold text-gray-900 dark:text-white overflow-hidden">
                        <div className="w-2 h-2 rounded-full bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.4)] animate-pulse shrink-0" />
                        <span className="truncate" title={currentAccount?.email}>
                            {currentAccount?.email?.split('@')[0] || 'No Account'}
                        </span>
                    </div>

                    <div
                        className="flex items-center gap-1 no-drag shrink-0"
                        onMouseDown={(e) => e.stopPropagation()}
                    >
                        <button
                            onClick={handleRefresh}
                            className={clsx(
                                "p-2 rounded-lg hover:bg-gray-200/50 dark:hover:bg-white/10 transition-colors"
                            )}
                            title={t('common.refresh', 'Refresh')}
                        >
                            <RefreshCw size={14} className={clsx(isRefreshing && "animate-spin text-blue-500")} />
                        </button>
                        <div className="w-px h-3 bg-gray-300 dark:bg-white/20 mx-1" />
                        <button
                            onClick={handleMaximize}
                            className="p-2 rounded-lg hover:bg-gray-200/50 dark:hover:bg-white/10 transition-colors text-gray-500 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white"
                            title={t('common.maximize', 'Full View')}
                        >
                            <Maximize2 size={14} />
                        </button>
                    </div>
                </div>

                {/* Content Scroll Area */}
                <div className="flex-1 overflow-y-auto overflow-x-hidden p-4 space-y-5 scrollbar-thin scrollbar-track-transparent scrollbar-thumb-gray-200 dark:scrollbar-thumb-white/10">
                    {/* Current Call Card - platform / model / tokens */}
                    {latestLog && (
                        <div className="rounded-xl border border-gray-100 dark:border-white/10 bg-gradient-to-br from-blue-50/60 to-transparent dark:from-blue-900/20 dark:to-transparent p-3">
                            <div className="flex items-center justify-between mb-1.5">
                                <span className="text-[10px] font-semibold uppercase tracking-wider text-blue-600 dark:text-blue-400">
                                    {latestLog.in_progress ? t('mini_view.calling', 'Calling') : t('mini_view.last_call', 'Last Call')}
                                </span>
                                <span
                                    className={`flex items-center gap-1.5 text-[10px] font-medium ${
                                        latestLog.in_progress
                                            ? 'text-blue-500'
                                            : latestLog.status >= 200 && latestLog.status < 400
                                                ? 'text-emerald-500'
                                                : 'text-rose-500'
                                    }`}
                                >
                                    {latestLog.in_progress && (
                                        <span className="w-1.5 h-1.5 rounded-full bg-blue-500 animate-pulse" />
                                    )}
                                    {latestLog.in_progress ? t('mini_view.in_flight', 'In Flight') : `HTTP ${latestLog.status}`}
                                </span>
                            </div>
                            <div className="flex items-center gap-2">
                                {latestLog.platform_name && (
                                    <span className="px-1.5 py-0.5 rounded bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 text-[10px] font-bold shrink-0 max-w-[90px] truncate" title={latestLog.platform_name}>
                                        {latestLog.platform_name}
                                    </span>
                                )}
                                <span className="text-sm font-bold text-gray-900 dark:text-white truncate" title={latestLog.model}>
                                    {latestLog.mapped_model || latestLog.model || '—'}
                                </span>
                            </div>
                            <div className="flex items-center gap-2 mt-1.5 text-[10px] font-mono text-gray-500 dark:text-gray-400">
                                <Activity size={10} className="text-blue-500" />
                                <span>I:<span className="text-gray-900 dark:text-gray-200">{formatCompactNumber(latestLog.input_tokens || 0)}</span></span>
                                <span>/</span>
                                <span>O:<span className="text-gray-900 dark:text-gray-200">{formatCompactNumber(latestLog.output_tokens || 0)}</span></span>
                                {!latestLog.in_progress && latestLog.duration > 0 && (
                                    <span className="ml-auto flex items-center gap-0.5">
                                        <Clock size={10} className="text-gray-400" />
                                        {(latestLog.duration / 1000).toFixed(2)}s
                                    </span>
                                )}
                            </div>
                        </div>
                    )}

                    {/* Recent Requests - expandable history */}
                    {requestHistory.length > 0 && (
                        <div className="rounded-xl border border-gray-100 dark:border-white/10 bg-white/40 dark:bg-white/5 p-3">
                            <div className="flex items-center justify-between mb-2">
                                <span className="text-[10px] font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400 flex items-center gap-1">
                                    <History size={10} className="text-gray-400" />
                                    {t('mini_view.recent_requests', 'Recent Requests')}
                                    <span className="font-mono text-gray-400 dark:text-gray-500">({requestHistory.length})</span>
                                </span>
                                <button
                                    onClick={() => setRequestHistory([])}
                                    className="p-1 rounded-lg hover:bg-gray-200/50 dark:hover:bg-white/10 text-gray-400 hover:text-rose-500 transition-colors"
                                    title={t('mini_view.clear', 'Clear')}
                                >
                                    <Trash2 size={11} />
                                </button>
                            </div>
                            <div className="space-y-1.5 max-h-[200px] overflow-y-auto scrollbar-thin scrollbar-track-transparent scrollbar-thumb-gray-200 dark:scrollbar-thumb-white/10">
                                <AnimatePresence initial={false}>
                                    {requestHistory.map((log) => {
                                        const isExpanded = expandedId === log.id;
                                        const ok = log.status >= 200 && log.status < 400;
                                        return (
                                            <motion.div
                                                key={log.id}
                                                layout
                                                initial={{ opacity: 0, y: -4 }}
                                                animate={{ opacity: 1, y: 0 }}
                                                exit={{ opacity: 0, height: 0 }}
                                                className="rounded-lg border border-gray-100 dark:border-white/10 overflow-hidden"
                                            >
                                                <button
                                                    onClick={() => setExpandedId(isExpanded ? null : log.id)}
                                                    className="w-full flex items-center gap-2 px-2 py-1.5 text-left hover:bg-gray-50 dark:hover:bg-white/5 transition-colors"
                                                    title={isExpanded ? t('mini_view.collapse', 'Collapse') : t('mini_view.expand', 'Expand')}
                                                >
                                                    <span
                                                        className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                                                            ok ? 'bg-emerald-500' : 'bg-red-500'
                                                        }`}
                                                    />
                                                    {log.platform_name && (
                                                        <span className="px-1 py-0.5 rounded bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 text-[9px] font-bold shrink-0 max-w-[56px] truncate">
                                                            {log.platform_name}
                                                        </span>
                                                    )}
                                                    <span className="text-[11px] font-semibold text-gray-800 dark:text-gray-200 truncate flex-1">
                                                        {log.mapped_model || log.model || '—'}
                                                    </span>
                                                    <span className="text-[9px] font-mono text-gray-400 shrink-0">
                                                        {formatCompactNumber(log.input_tokens || 0)}/{formatCompactNumber(log.output_tokens || 0)}
                                                    </span>
                                                    <ChevronDown
                                                        size={11}
                                                        className={`text-gray-400 shrink-0 transition-transform duration-200 ${isExpanded ? 'rotate-180' : ''}`}
                                                    />
                                                </button>
                                                <AnimatePresence initial={false}>
                                                    {isExpanded && (
                                                        <motion.div
                                                            key="details"
                                                            initial={{ height: 0, opacity: 0 }}
                                                            animate={{ height: 'auto', opacity: 1 }}
                                                            exit={{ height: 0, opacity: 0 }}
                                                            transition={{ duration: 0.2 }}
                                                            className="overflow-hidden"
                                                        >
                                                            <div className="px-2 pb-2 pt-1.5 border-t border-gray-100 dark:border-white/5 grid grid-cols-2 gap-x-3 gap-y-1 text-[9px] text-gray-500 dark:text-gray-400">
                                                                <span className="text-gray-400 dark:text-gray-500">{t('mini_view.platform', 'Platform')}:</span>
                                                                <span className="font-medium text-gray-700 dark:text-gray-300 truncate">{log.platform_name || log.platform_id || '—'}</span>
                                                                <span className="text-gray-400 dark:text-gray-500">{t('mini_view.model', 'Model')}:</span>
                                                                <span className="font-medium text-gray-700 dark:text-gray-300 truncate">{log.mapped_model || log.model || '—'}</span>
                                                                <span className="text-gray-400 dark:text-gray-500">{t('mini_view.status', 'Status')}:</span>
                                                                <span className={`font-mono font-bold ${ok ? 'text-emerald-600 dark:text-emerald-400' : 'text-rose-600 dark:text-rose-400'}`}>
                                                                    HTTP {log.status}
                                                                </span>
                                                                <span className="text-gray-400 dark:text-gray-500">{t('mini_view.duration', 'Duration')}:</span>
                                                                <span className="font-mono">{(log.duration / 1000).toFixed(2)}s</span>
                                                                <span className="text-gray-400 dark:text-gray-500">{t('mini_view.time', 'Time')}:</span>
                                                                <span className="font-mono">{formatTime(log.timestamp)}</span>
                                                                <span className="text-gray-400 dark:text-gray-500">Input:</span>
                                                                <span className="font-mono">{formatCompactNumber(log.input_tokens || 0)}</span>
                                                                <span className="text-gray-400 dark:text-gray-500">Output:</span>
                                                                <span className="font-mono">{formatCompactNumber(log.output_tokens || 0)}</span>
                                                                <span className="text-gray-400 dark:text-gray-500">{t('mini_view.total_tokens', 'Total')}:</span>
                                                                <span className="font-mono">{(log.input_tokens || 0) + (log.output_tokens || 0)}</span>
                                                            </div>
                                                        </motion.div>
                                                    )}
                                                </AnimatePresence>
                                            </motion.div>
                                        );
                                    })}
                                </AnimatePresence>
                            </div>
                        </div>
                    )}

                    {!currentAccount ? (
                        <div className="h-full flex flex-col items-center justify-center text-center opacity-50 space-y-2">
                            <ShieldAlert size={32} />
                            <p className="text-sm">No account selected</p>
                        </div>
                    ) : (
                        <div className="space-y-5">
                            {/* Account Info Card - Now simplified */}
                            <div className="flex flex-col gap-2">
                                <div className="flex flex-wrap gap-2">
                                    {/* Custom Label */}
                                    {currentAccount.custom_label && (
                                        <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/30 text-orange-600 dark:text-orange-400 text-[10px] font-bold shadow-sm shrink-0">
                                            <Tag className="w-2.5 h-2.5" />
                                            {currentAccount.custom_label}
                                        </span>
                                    )}
                                </div>
                            </div>

                            {/* Divider only if there was content above it */}
                            {currentAccount.custom_label && <div className="w-full h-px bg-gray-100 dark:bg-white/5" />}

                            {/* Models List */}
                            <AnimatePresence mode='popLayout'>
                                <div className="space-y-4 !mt-0">
                                    {renderModelRow(geminiProModel, 'Gemini 3.1 Pro', 'emerald')}
                                    {renderModelRow(geminiFlashModel, 'Gemini 3 Flash', 'emerald')}
                                    {renderModelRow(claudeModel, t('common.claude_series', 'Claude 系列'), 'cyan')}

                                    {!geminiProModel && !geminiFlashModel && !claudeModel && (
                                        <div className="text-center py-4 text-xs text-gray-400">
                                            No quota data available
                                        </div>
                                    )}
                                </div>
                            </AnimatePresence>
                        </div>
                    )}
                </div>

                {/* Footer Status / Latest Log */}
                <div className="flex-none h-8 bg-gray-50 dark:bg-black/20 flex items-center justify-between px-3 text-[10px] text-gray-500 dark:text-gray-400 border-t border-gray-100 dark:border-white/5 overflow-hidden">
                    {latestLog ? (
                        <motion.div
                            key={latestLog.id}
                            initial={{ opacity: 0, y: 5 }}
                            animate={{ opacity: 1, y: 0 }}
                            className="flex items-center w-full gap-2"
                        >
                            <span
                                title={latestLog.status.toString()}
                                className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                                    latestLog.in_progress
                                        ? 'bg-blue-500 animate-pulse'
                                        : latestLog.status >= 200 && latestLog.status < 400
                                            ? 'bg-emerald-500'
                                            : 'bg-red-500'
                                }`}
                            />
                            <span className="font-bold truncate max-w-[100px]" title={`${latestLog.platform_name || ''} / ${latestLog.model}`}>
                                {latestLog.mapped_model || latestLog.model}
                            </span>

                            <div className="flex-1 flex items-center justify-end gap-2">
                                <div className="flex items-center gap-1.5 text-[9px]" title="Input/Output Tokens">
                                    <Activity size={10} className="text-blue-500" />
                                    <span className="flex items-center gap-0.5 text-gray-500 dark:text-gray-400">
                                        I:<span className="font-mono text-gray-900 dark:text-gray-200">{formatCompactNumber(latestLog.input_tokens || 0)}</span>
                                    </span>
                                    <span className="text-gray-300 dark:text-gray-600">/</span>
                                    <span className="flex items-center gap-0.5 text-gray-500 dark:text-gray-400">
                                        O:<span className="font-mono text-gray-900 dark:text-gray-200">{formatCompactNumber(latestLog.output_tokens || 0)}</span>
                                    </span>
                                </div>

                                {!latestLog.in_progress && latestLog.duration > 0 && (
                                    <>
                                        <div className="w-px h-2.5 bg-gray-300 dark:bg-white/10" />
                                        <div className="flex items-center gap-0.5" title="Duration">
                                            <Clock size={10} className="text-gray-400" />
                                            <span className="font-mono">{(latestLog.duration / 1000).toFixed(2)}s</span>
                                        </div>
                                    </>
                                )}
                            </div>
                        </motion.div>
                    ) : (
                        <>
                            <div className="flex items-center gap-1.5">
                                <div className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                                <span>Connected</span>
                            </div>
                            <span className="font-mono opacity-50">v{appVersion}</span>
                        </>
                    )}
                </div>
            </motion.div>
        </div>
    );
}
