import { Platform } from './platform';

export interface AppConfig {
  language: string;
  theme: string;
  proxy_port: number;
  proxy_host: string;
  auto_switch: boolean;
  default_export_path?: string;
  platforms: Platform[];
}
