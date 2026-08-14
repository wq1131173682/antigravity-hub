// Semantic status palette for quota / health indicators.
// Centralizes the "color = meaning" mapping so the dashboard no longer
// scatters ad-hoc hues across components (ok → emerald, warn → amber,
// danger → rose). Tailwind JIT requires full literal class names, so every
// entry here is a complete className string, never a `text-${x}` template.

export type StatusLevel = 'ok' | 'warn' | 'danger';

/**
 * Derive a status level from a used/limit pair.
 * @param used    current consumption
 * @param limit   hard limit (0 or negative means unlimited → treated as ok)
 * @param over    explicit override (e.g. backend reports the key disabled by quota)
 */
export function quotaStatus(used: number, limit: number, over?: boolean): StatusLevel {
  if (limit <= 0) return 'ok';
  if (over ?? used > limit) return 'danger';
  const ratio = used / limit;
  if (ratio >= 0.8) return 'warn';
  return 'ok';
}

export const STATUS_BAR_GRADIENT: Record<StatusLevel, string> = {
  ok: 'bg-gradient-to-r from-emerald-400 to-emerald-500',
  warn: 'bg-gradient-to-r from-amber-400 to-amber-500',
  danger: 'bg-gradient-to-r from-rose-400 to-rose-500',
};

export const STATUS_BAR_SOLID: Record<StatusLevel, string> = {
  ok: 'bg-emerald-500',
  warn: 'bg-amber-500',
  danger: 'bg-rose-500',
};

export const STATUS_TEXT: Record<StatusLevel, string> = {
  ok: 'text-emerald-600 dark:text-emerald-400',
  warn: 'text-amber-600 dark:text-amber-400',
  danger: 'text-rose-600 dark:text-rose-400',
};

export const STATUS_DOT: Record<StatusLevel, string> = {
  ok: 'bg-emerald-500',
  warn: 'bg-amber-500',
  danger: 'bg-rose-500',
};

export const STATUS_TINT: Record<StatusLevel, string> = {
  ok: 'bg-emerald-50/40 dark:bg-emerald-900/10',
  warn: 'bg-amber-50/70 dark:bg-amber-900/10',
  danger: 'bg-red-100/70 dark:bg-red-900/20 ring-1 ring-red-300/50 dark:ring-red-700/40',
};
