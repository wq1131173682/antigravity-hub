export interface TunnelConfig {
  id: string;
  label: string;
  /** Local bind address for the forwarded port (default 127.0.0.1) */
  bind_address: string;
  local_port: number;
  user: string;
  bastion_host: string;
  bastion_port: number;
  target_host: string;
  target_port: number;
  identity_file?: string | null;
  extra_args?: string | null;
  /** Optional password login (Windows: plink / Unix: sshpass) */
  password?: string | null;
}

export interface TunnelStatus {
  id: string;
  running: boolean;
  started_at_ms: number | null;
  exit_code: number | null;
  /** Error text from ssh stderr; null when healthy or user-stopped */
  error: string | null;
}