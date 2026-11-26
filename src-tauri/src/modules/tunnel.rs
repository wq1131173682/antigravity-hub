use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::{LazyLock, Mutex, MutexGuard};

use tauri::Emitter;
use tauri::Manager;
use tokio::io::AsyncReadExt;
use tokio::process::Child;
use tracing::{info, warn};

use crate::models::tunnel::{TunnelConfig, TunnelStatus};

/// One managed ssh child process.
struct TunnelRuntime {
    /// Shared so stop_tunnel can steal the child for killing; the watcher
    /// task and stop race for it via take() (first caller wins).
    handle: Option<Arc<Mutex<Option<Child>>>>,
    running: bool,
    started_at_ms: Option<u64>,
    exit_code: Option<i32>,
    /// Error text from ssh stderr; None when healthy or user-stopped.
    error: Option<String>,
    /// Set when stop_tunnel killed the process so the watcher does not
    /// report a spurious "unexpected exit" error.
    user_stopped: bool,
}

type Handle = Arc<Mutex<Option<Child>>>;

static REGISTRY: LazyLock<Mutex<HashMap<String, TunnelRuntime>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Cap stderr capture so a chatty ssh client cannot grow memory unbounded.
const STDERR_CAP: usize = 8 * 1024;
const TUNNELS_FILE: &str = "tunnels.json";

fn lock_registry() -> MutexGuard<'static, HashMap<String, TunnelRuntime>> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn tunnels_path() -> Result<std::path::PathBuf, String> {
    let data_dir = super::platform_manager::get_data_dir()
        .map_err(|e| format!("Failed to resolve data dir: {e}"))?;
    Ok(data_dir.join(TUNNELS_FILE))
}

/// Load all persisted tunnels (empty list when the file does not exist).
pub fn load_tunnels() -> Vec<TunnelConfig> {
    let Ok(path) = tunnels_path() else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

fn save_tunnels(tunnels: &[TunnelConfig]) -> Result<(), String> {
    let path = tunnels_path()?;
    let tmp = path.with_file_name("tunnels.json.tmp");
    let content = serde_json::to_string_pretty(tunnels)
        .map_err(|e| format!("Failed to serialize tunnels: {e}"))?;
    std::fs::write(&tmp, &content).map_err(|e| format!("Failed to write tunnels file: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Failed to rename tunnels file: {e}"))
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

/// Add and persist a new tunnel. Fails when the local port is already used
/// by another tunnel.
#[allow(clippy::too_many_arguments)]
pub fn add_tunnel(
    label: Option<String>,
    local_port: u16,
    user: String,
    bastion_host: String,
    bastion_port: Option<u16>,
    target_host: String,
    target_port: u16,
    bind_address: Option<String>,
    identity_file: Option<String>,
    extra_args: Option<String>,
    password: Option<String>,
) -> Result<TunnelConfig, String> {
    let user = user.trim().to_string();
    let bastion_host = bastion_host.trim().to_string();
    let target_host = target_host.trim().to_string();
    if user.is_empty() || bastion_host.is_empty() || target_host.is_empty() {
        return Err("User, bastion host and target host must not be empty".into());
    }
    if local_port == 0 || target_port == 0 {
        return Err("Port must be in range 1-65535".into());
    }

    let mut list = load_tunnels();
    if list.iter().any(|t| t.local_port == local_port) {
        return Err(format!("Local port {local_port} is already used by another tunnel"));
    }
    let config = TunnelConfig {
        id: uuid::Uuid::new_v4().to_string(),
        label: label
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_default(),
        bind_address: bind_address
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_string()),
        local_port,
        user,
        bastion_host,
        bastion_port: bastion_port.unwrap_or(22),
        target_host,
        target_port,
        identity_file: identity_file.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        extra_args: extra_args.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        password: password
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    };
    list.push(config.clone());
    save_tunnels(&list)?;
    info!(
        id = %config.id,
        local = %format!("{}:{}", config.bind_address, config.local_port),
        via = %format!("{}@{}:{}", config.user, config.bastion_host, config.bastion_port),
        target = %format!("{}:{}", config.target_host, config.target_port),
        "tunnel added"
    );
    Ok(config)
}

/// Delete a tunnel (stopping it first if running).
pub async fn delete_tunnel(id: &str) -> Result<(), String> {
    let _ = stop_tunnel(id).await;
    lock_registry().remove(id);
    let mut list = load_tunnels();
    let before = list.len();
    list.retain(|t| t.id != id);
    if list.len() == before {
        return Err("Tunnel not found".into());
    }
    save_tunnels(&list)?;
    info!(id = %id, "tunnel deleted");
    Ok(())
}

// ---------------------------------------------------------------------------
// Process lifecycle
// ---------------------------------------------------------------------------

/// Build the full ssh argument list for a tunnel:
/// ssh -N -L bind:local:target:tport [ -p port ] [ -i identity ] [ extra ] user@host
fn build_ssh_args(config: &TunnelConfig) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    args.push("-N".into());
    let local = format!("{}:{}", config.bind_address, config.local_port);
    let remote = format!("{}:{}", config.target_host, config.target_port);
    args.push(format!("-L{local}:{remote}"));
    if config.bastion_port != 22 {
        args.push("-p".into());
        args.push(config.bastion_port.to_string());
    }
    // Fail fast if the forwarded channel cannot be established.
    args.push("-o".into());
    args.push("ExitOnForwardFailure=yes".into());
    // Detect dead tunnels (NAT timeouts, bastion restarts).
    args.push("-o".into());
    args.push("ServerAliveInterval=30".into());
    args.push("-o".into());
    args.push("ServerAliveCountMax=3".into());
    // First contact with a bastion host key: trust automatically instead of
    // blocking on an interactive prompt the GUI cannot answer.
    args.push("-o".into());
    args.push("StrictHostKeyChecking=accept-new".into());
    if let Some(identity) = &config.identity_file {
        args.push("-i".into());
        args.push(identity.clone());
    }
    if let Some(extra) = &config.extra_args {
        if let Some(words) = shlex::split(extra) {
            args.extend(words);
        } else {
            warn!(extra = %extra, "failed to parse extra ssh args; ignoring them");
        }
    }
    args.push(format!("{}@{}", config.user, config.bastion_host));
    args
}

/// First existing candidate on PATH (or in `extra`), as an absolute path.
fn find_on_path(bin: &str, extra: &[&str]) -> Option<String> {
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(bin);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    extra
        .iter()
        .find(|p| std::path::Path::new(p).is_file())
        .map(|p| p.to_string())
}

/// Pick the process for a tunnel: plain `ssh` for key/agent auth, or
/// `plink` (Windows — the bundled `resources/plink.exe`, falling back
/// to an installed PuTTY) / `sshpass` (Unix) when a password is
/// configured — OpenSSH cannot answer the interactive password prompt
/// headlessly.
#[cfg_attr(not(windows), allow(unused_variables))]
fn build_command(
    config: &TunnelConfig,
    resource_dir: &Option<std::path::PathBuf>,
) -> Result<(String, Vec<String>), String> {
    let password = config
        .password
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    match password {
        Some(pw) => {
            #[cfg(windows)]
            {
                // PuTTY plink: password via -pw, no TTY needed.
                let mut args: Vec<String> = Vec::new();
                args.push("-ssh".into());
                if config.bastion_port != 22 {
                    args.push("-P".into());
                    args.push(config.bastion_port.to_string());
                }
                // plink -L [bind:]lport:rhost:rport
                let forward = if config.bind_address == "127.0.0.1" {
                    format!("{}:{}:{}", config.local_port, config.target_host, config.target_port)
                } else {
                    format!(
                        "{}:{}:{}:{}",
                        config.bind_address, config.local_port, config.target_host, config.target_port
                    )
                };
                args.push(format!("-L{forward}"));
                args.push("-N".into());
                args.push("-pw".into());
                args.push(pw.to_string());
                // -batch: never prompt (a GUI cannot answer interactive
                // questions). Without -no-ssh-check, plink aborts on first
                // contact with a host whose key is not yet cached in the
                // registry, so trust new keys like
                // StrictHostKeyChecking=accept-new does for plain ssh.
                args.push("-batch".into());
                args.push("-no-ssh-check".into());
                args.push(format!("{}@{}", config.user, config.bastion_host));

                // The bundled copy ships in the app resources; an
                // installed PuTTY (either default dir) or a PATH hit
                // still works if the resource is missing.
                let candidates: Vec<std::path::PathBuf> = resource_dir
                    .iter()
                    .map(|dir| dir.join("plink.exe"))
                    .chain(std::iter::once(
                        std::path::PathBuf::from(r"C:\Program Files\PuTTY\plink.exe"),
                    ))
                    .chain(std::iter::once(
                        std::path::PathBuf::from(r"C:\Program Files (x86)\PuTTY\plink.exe"),
                    ))
                    .collect();
                let bin = candidates
                    .iter()
                    .find(|p| p.is_file())
                    .map(|p| p.to_string_lossy().into_owned())
                    .or_else(|| find_on_path("plink.exe", &[]))
                    .ok_or_else(|| {
                        "plink.exe not found: the bundled copy is missing and PuTTY is not installed. Or use key auth instead.".to_string()
                    })?;
                Ok((bin, args))
            }
            #[cfg(not(windows))]
            {
                let bin = find_on_path("sshpass", &[]).ok_or_else(|| {
                    "Password login needs the `sshpass` utility (Linux: apt install sshpass; macOS: brew install hudip/deprecated/sshpass). Or use key auth instead.".to_string()
                })?;
                let mut args: Vec<String> = Vec::new();
                args.push("-p".into());
                args.push(pw.to_string());
                args.extend(build_ssh_args(config));
                Ok((bin, args))
            }
        }
        None => Ok(("ssh".into(), build_ssh_args(config))),
    }
}

/// Spawn the tunnel process. For plain `ssh` on Windows, OpenSSH may not be
/// on PATH (feature install), so retry the default system location once.
#[cfg_attr(not(windows), allow(unused_variables))]
fn spawn_tunnel_process(
    config: &TunnelConfig,
    resource_dir: &Option<std::path::PathBuf>,
) -> Result<Child, String> {
    let (bin, args) = build_command(config, resource_dir)?;

    let make = |bin: &str| -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(bin);
        cmd.args(&args);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());
        #[cfg(windows)]
        {
            // Do not allocate a console window for the child.
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW (tokio Command, Windows only)
        }
        cmd
    };

    match make(&bin).spawn() {
        Ok(child) => Ok(child),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && bin == "ssh" => {
            #[cfg(windows)]
            {
                const SYSTEM_OPENSSH: &str = r"C:WindowsSystem32OpenSSHssh.exe";
                match make(SYSTEM_OPENSSH).spawn() {
                    Ok(child) => {
                        info!("ssh not on PATH; using system OpenSSH at {SYSTEM_OPENSSH}");
                        Ok(child)
                    }
                    Err(e2) => Err(format!(
                        "Failed to spawn ssh ({e}); also failed at {SYSTEM_OPENSSH} ({e2}). Is OpenSSH installed?"
                    )),
                }
            }
            #[cfg(not(windows))]
            Err(format!("Failed to spawn ssh: {e}. Is an OpenSSH client installed?"))
        }
        Err(e) => Err(format!("Failed to spawn {bin}: {e}")),
    }
}

/// Start the tunnel for a persisted config id. Spawns the ssh child and a
/// watcher task that reports the exit via the "tunnel:status" event.
pub async fn start_tunnel(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let config = load_tunnels()
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| "Tunnel not found".to_string())?;

    if config.local_port == 0 {
        return Err("Invalid tunnel configuration".into());
    }
    {
        let reg = lock_registry();
        if let Some(rt) = reg.get(id) {
            if rt.running {
                return Err("Tunnel is already running".into());
            }
        }
    }

    // resource_dir() may fail in odd sandboxes; the command builder
    // tolerates None (falls back to an installed PuTTY / PATH plink).
    let resource_dir = app.path().resource_dir().ok();
    let mut child = spawn_tunnel_process(&config, &resource_dir)?;
    let stderr = child.stderr.take();
    let handle: Handle = Arc::new(Mutex::new(Some(child)));
    let started_at = now_ms();

    {
        let mut reg = lock_registry();
        reg.insert(
            config.id.clone(),
            TunnelRuntime {
                handle: Some(handle.clone()),
                running: true,
                started_at_ms: Some(started_at),
                exit_code: None,
                error: None,
                user_stopped: false,
            },
        );
    }

    let app = app.clone();
    let tid = config.id.clone();
    tokio::spawn(async move {
        // Drain stderr in parallel so the pipe never fills up and blocks ssh.
        let stderr_task = tokio::spawn(async move {
            if let Some(mut reader) = stderr {
                let mut buf: Vec<u8> = Vec::new();
                let ok = reader.read_to_end(&mut buf).await.is_ok();
                if ok && buf.len() > STDERR_CAP {
                    let start = buf.len() - STDERR_CAP;
                    let _ = buf.drain(..start);
                }
                String::from_utf8_lossy(&buf).into_owned()
            } else {
                String::new()
            }
        });

        let child_opt = handle.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(mut child) = child_opt else {
            return; // stop_tunnel already took ownership and killed it
        };
        let status = child.wait().await;
        let stderr_text = stderr_task.await.unwrap_or_default();

        let ok = status.as_ref().map(|s| s.success()).unwrap_or(false);
        let exit_code = status.ok().and_then(|s| s.code());
        {
            let mut reg = lock_registry();
            if let Some(rt) = reg.get_mut(&tid) {
                rt.running = false;
                rt.exit_code = exit_code;
                if !ok && !rt.user_stopped {
                    let detail = stderr_text.trim();
                    rt.error = if detail.is_empty() {
                        Some(format!("tunnel process exited with status {exit_code:?}"))
                    } else {
                        Some(format!("tunnel process exited with status {exit_code:?}: {detail}"))
                    };
                    warn!(id = %tid, ?exit_code, "tunnel ssh process exited unexpectedly");
                }
            }
        }
        let snapshot = {
            let reg = lock_registry();
            reg.get(&tid)
                .map(|rt| TunnelStatus {
                    id: tid.clone(),
                    running: rt.running,
                    started_at_ms: rt.started_at_ms,
                    exit_code: rt.exit_code,
                    error: rt.error.clone(),
                })
                .unwrap_or(TunnelStatus {
                    id: tid.clone(),
                    running: false,
                    started_at_ms: None,
                    exit_code: None,
                    error: None,
                })
        };
        let _ = app.emit("tunnel:status", &snapshot);
    });

    info!(
        id = %config.id,
        local = %format!("{}:{}", config.bind_address, config.local_port),
        via = %format!("{}@{}:{}", config.user, config.bastion_host, config.bastion_port),
        target = %format!("{}:{}", config.target_host, config.target_port),
        "tunnel started"
    );
    Ok(())
}

/// Stop a running tunnel (kill the ssh child). Idempotent for stopped ones.
pub async fn stop_tunnel(id: &str) -> Result<(), String> {
    let child: Option<Child>;
    {
        let mut reg = lock_registry();
        match reg.get_mut(id) {
            Some(rt) if rt.running => {
                rt.running = false;
                rt.user_stopped = true;
                child = rt.handle.take().and_then(|h| {
                    h.lock().unwrap_or_else(|e| e.into_inner()).take()
                });
            }
            _ => return Err("Tunnel is not running".into()),
        }
    }
    if let Some(mut c) = child {
        let _ = c.kill().await;
        let _ = c.wait().await;
    }
    info!(id = %id, "tunnel stopped by user");
    Ok(())
}

/// Stop every running tunnel (used on app exit).
pub async fn stop_all_tunnels() {
    let mut ids: Vec<String> = {
        let reg = lock_registry();
        reg.iter()
            .filter(|(_, rt)| rt.running)
            .map(|(id, _)| id.clone())
            .collect()
    };
    for id in ids.drain(..) {
        let _ = stop_tunnel(&id).await;
    }
}

/// Runtime status of every persisted tunnel.
pub fn list_statuses() -> Vec<TunnelStatus> {
    let reg = lock_registry();
    load_tunnels()
        .into_iter()
        .map(|c| {
            match reg.get(&c.id) {
                Some(rt) => TunnelStatus {
                    id: c.id.clone(),
                    running: rt.running,
                    started_at_ms: rt.started_at_ms,
                    exit_code: rt.exit_code,
                    error: rt.error.clone(),
                },
                None => TunnelStatus {
                    id: c.id.clone(),
                    running: false,
                    started_at_ms: None,
                    exit_code: None,
                    error: None,
                },
            }
        })
        .collect()
}

