import { request as invoke } from '../utils/request';
import { TunnelConfig, TunnelStatus } from '../types/tunnel';

// ============================================================================
// SSH Tunnels (jump-host local port forwarding)
// ============================================================================

export async function listTunnels(): Promise<TunnelConfig[]> {
  return await invoke('list_tunnels');
}

export async function addTunnel(params: {
  label?: string;
  localPort: number;
  user: string;
  bastionHost: string;
  bastionPort?: number;
  targetHost: string;
  targetPort: number;
  bindAddress?: string;
  identityFile?: string;
  extraArgs?: string;
}): Promise<TunnelConfig> {
  return await invoke('add_tunnel', {
    label: params.label || null,
    localPort: params.localPort,
    user: params.user,
    bastionHost: params.bastionHost,
    bastionPort: params.bastionPort ?? null,
    targetHost: params.targetHost,
    targetPort: params.targetPort,
    bindAddress: params.bindAddress || null,
    identityFile: params.identityFile || null,
    extraArgs: params.extraArgs || null,
  });
}

export async function deleteTunnel(id: string): Promise<void> {
  return await invoke('delete_tunnel', { id });
}

export async function startTunnel(id: string): Promise<void> {
  return await invoke('start_tunnel', { id });
}

export async function stopTunnel(id: string): Promise<void> {
  return await invoke('stop_tunnel', { id });
}

export async function getTunnelStatuses(): Promise<TunnelStatus[]> {
  return await invoke('get_tunnel_statuses');
}

/** The equivalent ssh command, for display next to each tunnel. */
export function sshCommandPreview(t: TunnelConfig): string {
  const bind = t.bind_address || '127.0.0.1';
  let cmd = `ssh -N -L ${bind}:${t.local_port}:${t.target_host}:${t.target_port}`;
  if (t.bastion_port !== 22) cmd += ` -p ${t.bastion_port}`;
  if (t.identity_file) cmd += ` -i ${t.identity_file}`;
  cmd += ` ${t.user}@${t.bastion_host}`;
  if (t.extra_args) cmd += ` ${t.extra_args}`;
  return cmd;
}