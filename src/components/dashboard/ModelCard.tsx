import { useState, useMemo } from 'react';
import { AlertTriangle, ChevronDown, Hash } from 'lucide-react';
import { QuotaBar } from '../common/QuotaBar';
import { quotaStatus, STATUS_TEXT, STATUS_BAR_SOLID } from '../../utils/status';
import { MODEL_CONFIG } from '../../config/modelConfig';
import type { Model, ModelUsageEntry } from '../../types/platform';

interface ModelCardProps {
  model: Model;
  usageEntries: ModelUsageEntry[];
  defaultExpanded?: boolean;
}

function formatLimit(v: number): string {
  if (v <= 0) return '∞';
  if (v >= 10000) return `${(v / 1000).toFixed(1)}k`;
  return String(v);
}

export function ModelCard({ model, usageEntries, defaultExpanded = false }: ModelCardProps) {
  const [expanded, setExpanded] = useState(defaultExpanded);

  const usageLoaded = usageEntries.length > 0;
  const availableCount = usageEntries.filter(u => u.is_available).length;
  const totalCount = usageEntries.length;
  const allExhausted = usageLoaded && availableCount === 0;
  const exceededCount = usageEntries.filter(u => !u.is_available).length;

  // Aggregated model-level 5h quota
  const modelQuota = useMemo(() => {
    let totalUsed = 0;
    let usedWithLimit = 0;
    let limitedKeyCount = 0;
    for (const u of usageEntries) {
      if (model.per_5hour > 0) {
        usedWithLimit += Math.min(u.five_hour.count, model.per_5hour);
        limitedKeyCount++;
      }
      totalUsed += u.five_hour.count;
    }
    const effectiveLimit = model.per_5hour > 0 ? model.per_5hour * limitedKeyCount : 0;
    return {
      totalUsed,
      effectiveLimit,
      ratio: effectiveLimit > 0 ? Math.min(1, usedWithLimit / effectiveLimit) : 0,
      over: effectiveLimit > 0 && usedWithLimit >= effectiveLimit,
    };
  }, [usageEntries, model.per_5hour, totalCount]);

  // Sort entries: available first
  const sortedEntries = useMemo(() =>
    [...usageEntries].sort((a, b) => {
      if (a.is_available && !b.is_available) return -1;
      if (!a.is_available && b.is_available) return 1;
      return 0;
    }),
    [usageEntries]
  );

  const modelConfig = MODEL_CONFIG[model.id.toLowerCase()];
  const ModelIcon = modelConfig?.Icon;

  return (
    <div className="bg-white dark:bg-base-100 rounded-lg border border-gray-100 dark:border-base-200 overflow-hidden">
      {/* ── Collapsed header (always visible) ── */}
      <button
        onClick={() => setExpanded(v => !v)}
        className="w-full text-left px-3 py-2.5 flex items-center gap-2.5 hover:bg-gray-50/50 dark:hover:bg-base-200/30 transition-colors"
      >
        {/* Model icon */}
        {ModelIcon ? (
          <ModelIcon size={18} className="flex-shrink-0" />
        ) : (
          <div className="w-[18px] h-[18px] flex-shrink-0 rounded bg-gray-200 dark:bg-base-300 flex items-center justify-center">
            <span className="text-[9px] font-bold text-gray-500 dark:text-gray-400">M</span>
          </div>
        )}

        {/* Model name */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5">
            <span className="text-xs font-medium text-gray-800 dark:text-gray-200 truncate" title={model.model_name}>
              {model.display_name || model.model_name}
            </span>
            {/* Exceeded badge */}
            {usageLoaded && exceededCount > 0 && (
              <span className="flex items-center gap-0.5 px-1 py-0.5 rounded-full bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 text-[10px] font-bold flex-shrink-0">
                <AlertTriangle className="w-2.5 h-2.5" />
                {exceededCount}
              </span>
            )}
          </div>
        </div>

        {/* Availability badge */}
        {usageLoaded ? (
          <span
            className={`text-[10px] font-mono px-2 py-0.5 rounded-full flex-shrink-0 ${
              allExhausted
                ? 'bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400'
                : exceededCount > 0
                  ? 'bg-amber-100 dark:bg-amber-900/30 text-amber-600 dark:text-amber-400'
                  : 'bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-400'
            }`}
          >
            {availableCount}/{totalCount}
          </span>
        ) : (
          <span className="text-[10px] text-gray-400 dark:text-gray-500 flex-shrink-0">…</span>
        )}

        {/* Expand arrow */}
        <ChevronDown
          className={`w-3.5 h-3.5 text-gray-400 transition-transform flex-shrink-0 ${
            expanded ? 'rotate-180' : ''
          }`}
        />
      </button>

      {/* ── Aggregated model-level quota bar (always visible) ── */}
      {usageLoaded && model.per_5hour > 0 && (
        <div className="px-3 pb-1.5">
          <QuotaBar
            used={modelQuota.totalUsed}
            limit={modelQuota.effectiveLimit}
            size="xs"
            over={modelQuota.over}
            trackClassName="bg-gray-100 dark:bg-base-300"
          />
        </div>
      )}

      {/* ── Expanded per-key details ── */}
      {expanded && usageLoaded && (
        <div className="border-t border-gray-100/60 dark:border-base-300/60">
          {sortedEntries.map(u => {
            const over5h = model.per_5hour > 0 && u.five_hour.count > model.per_5hour;
            const overDay = model.per_day > 0 && u.day.count > model.per_day;
            const overMon = model.per_month > 0 && u.month.count > model.per_month;
            const isDisabled = !u.is_available;
            const overAny = over5h || overDay || overMon;

            const status5h = quotaStatus(u.five_hour.count, model.per_5hour, over5h);
            const statusDay = quotaStatus(u.day.count, model.per_day, overDay);
            const statusMon = quotaStatus(u.month.count, model.per_month, overMon);

            const rowDotCls = isDisabled ? 'bg-red-500' : overAny ? 'bg-amber-500' : 'bg-emerald-500';
            const rowBgCls = isDisabled
              ? 'bg-red-50/30 dark:bg-red-900/8'
              : overAny
                ? 'bg-amber-50/20 dark:bg-amber-900/8'
                : '';

            const disabledInfo = isDisabled && u.disabled_until
              ? `Disabled until ${new Date(u.disabled_until * 1000).toLocaleString()}`
              : isDisabled ? 'Disabled (5xx backoff)' : '';

            return (
              <div
                key={u.key_id}
                className={`px-3 py-1.5 border-b border-gray-50 dark:border-base-300/40 last:border-b-0 ${rowBgCls}`}
                title={`Key: ${u.key_id}${disabledInfo ? '\n' + disabledInfo : ''}`}
              >
                {/* Key ID row */}
                <div className="flex items-center gap-1.5 mb-1">
                  <span className={`inline-block w-1.5 h-1.5 rounded-full flex-shrink-0 ${rowDotCls}`} />
                  <span className="text-[10px] font-mono text-gray-500 dark:text-gray-400 truncate">
                    {u.key_id.slice(0, 10)}…{u.key_id.slice(-4)}
                  </span>
                  {isDisabled && (
                    <span className="text-[9px] px-1 py-0.5 rounded bg-red-100 dark:bg-red-900/30 text-red-500 dark:text-red-400 font-medium">
                      disabled
                    </span>
                  )}
                </div>

                {/* Compact three-in-one quota bar row */}
                <div className="flex items-center gap-2">
                  {/* 5h */}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center justify-between mb-0.5">
                      <span className={`text-[9px] font-semibold uppercase tracking-wider ${STATUS_TEXT[status5h]}`}>5h</span>
                      <span className={`text-[9px] font-mono tabular-nums ${STATUS_TEXT[status5h]}`}>
                        {u.five_hour.count}<span className="text-gray-400 dark:text-gray-500">/{formatLimit(model.per_5hour)}</span>
                      </span>
                    </div>
                    <div className="h-1 w-full rounded-full overflow-hidden bg-gray-100 dark:bg-base-300">
                      <div
                        className={`h-full rounded-full transition-all duration-500 ${STATUS_BAR_SOLID[status5h]}`}
                        style={{ width: `${model.per_5hour > 0 ? Math.min(100, (u.five_hour.count / model.per_5hour) * 100) : 0}%` }}
                      />
                    </div>
                  </div>

                  {/* Day */}
                  <div className="w-20 flex-shrink-0">
                    <div className="flex items-center justify-between mb-0.5">
                      <span className={`text-[9px] font-semibold uppercase tracking-wider ${STATUS_TEXT[statusDay]}`}>D</span>
                      <span className={`text-[9px] font-mono tabular-nums ${STATUS_TEXT[statusDay]}`}>
                        {u.day.count}<span className="text-gray-400 dark:text-gray-500">/{formatLimit(model.per_day)}</span>
                      </span>
                    </div>
                    <div className="h-1 w-full rounded-full overflow-hidden bg-gray-100 dark:bg-base-300">
                      <div
                        className="h-full rounded-full transition-all duration-500"
                        style={{
                          width: `${model.per_day > 0 ? Math.min(100, (u.day.count / model.per_day) * 100) : 0}%`,
                          backgroundColor: statusDay === 'danger' ? '#f43f5e' : statusDay === 'warn' ? '#f59e0b' : '#10b981',
                        }}
                      />
                    </div>
                  </div>

                  {/* Month */}
                  <div className="w-20 flex-shrink-0">
                    <div className="flex items-center justify-between mb-0.5">
                      <span className={`text-[9px] font-semibold uppercase tracking-wider ${STATUS_TEXT[statusMon]}`}>M</span>
                      <span className={`text-[9px] font-mono tabular-nums ${STATUS_TEXT[statusMon]}`}>
                        {u.month.count}<span className="text-gray-400 dark:text-gray-500">/{formatLimit(model.per_month)}</span>
                      </span>
                    </div>
                    <div className="h-1 w-full rounded-full overflow-hidden bg-gray-100 dark:bg-base-300">
                      <div
                        className="h-full rounded-full transition-all duration-500"
                        style={{
                          width: `${model.per_month > 0 ? Math.min(100, (u.month.count / model.per_month) * 100) : 0}%`,
                          backgroundColor: statusMon === 'danger' ? '#f43f5e' : statusMon === 'warn' ? '#f59e0b' : '#10b981',
                        }}
                      />
                    </div>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Expanded but no usage loaded */}
      {expanded && !usageLoaded && (
        <div className="px-3 py-2 border-t border-gray-100/60 dark:border-base-300/60">
          <div className="text-[10px] text-gray-400 dark:text-gray-500 text-center flex items-center justify-center gap-1">
            <Hash className="w-3 h-3" />
            No usage data yet
          </div>
        </div>
      )}
    </div>
  );
}

export default ModelCard;