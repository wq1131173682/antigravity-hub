export interface QuotaWindowLimits {
  max_per_5hrs: number;
  max_per_day: number;
  max_per_month: number;
}

export interface UsageWindowStatus {
  count: number;
  max: number;
  window_start: number;
}

export interface KeyWindowStatus {
  key_id: string;
  platform_id: string;
  five_hour: UsageWindowStatus;
  day: UsageWindowStatus;
  month: UsageWindowStatus;
  consecutive_429: number;
  consecutive_500: number;
  disabled_until: number | null;
  disabled_reason: string | null;
  is_available: boolean;
}
