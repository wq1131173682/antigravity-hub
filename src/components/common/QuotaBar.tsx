import { quotaStatus, STATUS_BAR_GRADIENT, STATUS_BAR_SOLID, STATUS_TEXT } from '../../utils/status';

export interface QuotaBarProps {
  /** current consumption */
  used: number;
  /** hard limit (0 / negative = unlimited → rendered as an empty neutral track) */
  limit: number;
  /** Optional label rendered above the bar (e.g. "5h 限额"). */
  label?: string;
  /** Explicit over-quota flag; overrides ratio-based detection. */
  over?: boolean;
  /** Render the "used / limit" value on the right. Defaults to true when a label is given. */
  showValue?: boolean;
  /** Format a numeric value for display (compact, infinity, etc.). */
  format?: (n: number) => string;
  /** Bar height preset. */
  size?: 'xs' | 'sm' | 'md';
  /** Fill style. Defaults to gradient. */
  variant?: 'solid' | 'gradient';
  /** Classes for the track element (e.g. a tinted track on a colored hero). */
  trackClassName?: string;
  /** Extra classes for the wrapper element. */
  className?: string;
}

const SIZE_H: Record<NonNullable<QuotaBarProps['size']>, string> = {
  xs: 'h-1',
  sm: 'h-2',
  md: 'h-3',
};

/**
 * Single source of truth for every quota / usage bar in the app.
 * Color is derived from `quotaStatus` so status is always semantic
 * (ok → emerald, warn → amber, danger → rose) regardless of where it is used.
 */
export function QuotaBar({
  used,
  limit,
  label,
  over,
  showValue,
  format = (n) => String(n),
  size = 'sm',
  variant = 'gradient',
  trackClassName,
  className,
}: QuotaBarProps) {
  const unlimited = limit <= 0;
  const status = quotaStatus(used, limit, over);
  const textCls = unlimited ? 'text-gray-500 dark:text-gray-400' : STATUS_TEXT[status];
  const barFill = unlimited
    ? ''
    : (variant === 'gradient' ? STATUS_BAR_GRADIENT : STATUS_BAR_SOLID)[status];
  const widthPct = unlimited ? 0 : Math.max(over ? 100 : Math.min(1, used / limit) * 100, 2);
  const displayValue = showValue ?? !!label;

  return (
    <div className={className}>
      {label && (
        <div className="flex items-baseline justify-between mb-1">
          <span className="text-[10px] font-bold uppercase tracking-wider text-gray-600 dark:text-gray-300">
            {label}
          </span>
          {displayValue && (
            <span className={`text-xs font-mono font-bold tabular-nums ${textCls}`}>
              {format(used)}
              <span className="text-gray-400 dark:text-gray-500 font-normal">/{format(limit)}</span>
            </span>
          )}
        </div>
      )}
      <div
        className={`w-full ${SIZE_H[size]} rounded-full overflow-hidden ${
          trackClassName || 'bg-gray-200/60 dark:bg-base-300/60'
        }`}
      >
        <div
          className={`h-full rounded-full transition-all duration-500 ${barFill}`}
          style={{ width: `${widthPct}%` }}
        />
      </div>
    </div>
  );
}

export default QuotaBar;
