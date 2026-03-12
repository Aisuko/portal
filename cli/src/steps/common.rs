use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::config::Node;
use crate::log::LogSender;
use crate::ssh::{probe_tcp, Credentials, ExecOutput, SshSession};

/// Idempotent `curl` install, standing in for Ansible's OS-agnostic
/// `package` module. This tool targets the Debian/Ubuntu nodes the cluster
/// actually runs on (same assumption the NVIDIA runtime step already made
/// with `apt`).
pub const ENSURE_CURL: &str = "command -v curl >/dev/null 2>&1 || (apt-get update -qq && apt-get install -y curl)";

/// Poll a TCP port until it accepts a connection or `total_timeout`
/// elapses, replacing Ansible's `wait_for` module.
pub async fn wait_for_port(host: &str, port: u16, total_timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + total_timeout;
    loop {
        if probe_tcp(host, port, Duration::from_secs(3)).await {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

/// Probe every master in declaration order and return the first one
/// reachable on :6443, preferring its `hostname` field — the same
/// "Option A: first reachable master" logic used by
/// `01_install_k3s_ha.yml` and `02_join_gpu_node.yml`.
pub async fn pick_join_endpoint(masters: &[Node]) -> Option<String> {
    for m in masters {
        if probe_tcp(&m.address, 6443, Duration::from_secs(2)).await {
            return Some(m.hostname_or_address().to_string());
        }
    }
    None
}

/// Read `/var/lib/rancher/k3s/server/node-token` from the bootstrap
/// master, the shared secret new masters/agents need to join.
pub async fn read_node_token(
    cluster_init: &Node,
    port: u16,
    creds: &Credentials,
    tx: &LogSender,
) -> Result<String> {
    let session = SshSession::connect(cluster_init, port, creds, tx)
        .await
        .with_context(|| format!("connecting to bootstrap master {}", cluster_init.name))?;
    let out = session
        .exec(
            "cat /var/lib/rancher/k3s/server/node-token",
            true,
            creds.sudo_password.as_deref(),
            tx,
        )
        .await;
    session.close().await;
    let out = out?;
    if !out.success() {
        bail!("could not read node-token from {}: {}", cluster_init.name, out.stderr.trim());
    }
    Ok(out.stdout.trim().to_string())
}

/// Run `script` and turn a nonzero exit code into an `Err`, so callers can
/// just `?` a whole sequence of remote commands.
pub async fn exec_ok(
    session: &SshSession,
    script: &str,
    sudo: bool,
    sudo_password: Option<&str>,
    tx: &LogSender,
) -> Result<ExecOutput> {
    let out = session.exec(script, sudo, sudo_password, tx).await?;
    if !out.success() {
        bail!("command failed (exit {}): {}", out.code, out.stderr.trim());
    }
    Ok(out)
}
