use serde::{Deserialize, Serialize};

/// A configured SSH local port forwarding rule (jump-host tunnel),
/// equivalent to: `ssh -N -L [bind:]local_port:target:port user@bastion`.
///
/// Example (bastion `ssh -L 16201:10.16.16.15:6201 root@192.168.2.72 -N`):
/// local_port = 16201, target_host = 10.16.16.15, target_port = 6201,
/// user = root, bastion_host = 192.168.2.72.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub id: String,
    /// Human-friendly name; empty means auto-label
    #[serde(default)]
    pub label: String,
    /// Local bind address for the forwarded port (default "127.0.0.1";
    /// use "0.0.0.0" to expose the port on the LAN).
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    /// Local port to listen on
    pub local_port: u16,
    /// SSH user on the bastion
    pub user: String,
    /// Bastion (jump) host
    pub bastion_host: String,
    /// Bastion SSH port
    #[serde(default = "default_bastion_port")]
    pub bastion_port: u16,
    /// Target host, as reachable from the bastion
    pub target_host: String,
    /// Target port on the target host
    pub target_port: u16,
    /// Optional explicit identity file (ssh -i)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    /// Optional raw extra ssh arguments (shlex-parsed and appended)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_args: Option<String>,
}

fn default_bind_address() -> String {
    "127.0.0.1".to_string()
}

fn default_bastion_port() -> u16 {
    22
}

/// Live status of one tunnel (runtime view, not persisted).
#[derive(Debug, Clone, Serialize)]
pub struct TunnelStatus {
    pub id: String,
    pub running: bool,
    /// Unix millis when the current run started (None when stopped)
    pub started_at_ms: Option<u64>,
    /// ssh exit code of the last run (None when still running / never run)
    pub exit_code: Option<i32>,
    /// Last error text (ssh stderr tail); None when healthy or user-stopped
    pub error: Option<String>,
}
