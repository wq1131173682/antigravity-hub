export interface Platform {
  id: string;
  name: string;
  base_url: string;
  path_prefix: string;
  notes?: string;
  created_at: number;
}

export interface Model {
  id: string;
  platform_id: string;
  model_name: string;
  display_name: string;
  per_5hour: number;
  per_day: number;
  per_month: number;
  sort_order: number;
  created_at: number;
}

export interface ApiKey {
  id: string;
  platform_id: string;
  name: string;
  key_value: string;
  disabled: boolean;
  disabled_reason: string | null;
  disabled_until: number | null;
  created_at: number;
}

export interface ModelUsageEntry {
  key_id: string;
  model_id: string;
  platform_id: string;
  five_hour: { count: number; max: number; window_start: number };
  day: { count: number; max: number; window_start: number };
  month: { count: number; max: number; window_start: number };
  consecutive_429: number;
  consecutive_500: number;
  disabled_until: number | null;
  is_available: boolean;
}
