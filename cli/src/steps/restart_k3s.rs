use std::time::Duration;

use anyhow::Result;

use crate::config::{Node, ROLE_GPU_NODE, ROLE_K3S_MASTER};
use crate::log::{log, LogLevel};
use crate::ssh::SshSession;

use super::common::exec_ok;
use super::{run_parallel, summarize, StepCtx};

const OVERRIDE_CONF: &str = "[Unit]\n\
Wants=tailscaled.service\n\
After=tailscaled.service network-online.target\n\
\n\
[Service]\n\
ExecStartPre=/bin/sh -c 'for i in $(seq 1 30); do ip link show tailscale0 >/dev/null 2>&1 && exit 0; sleep 1; done; echo tailscale0 not ready; exit 1'\n";

/// Port of `infra/04_restart-k3s.yml`: re-pin k3s/k3s-agent to wait for
/// `tailscale0` before starting, then restart it and report whether
/// `flannel.1` came back.
pub async fn run(ctx: &StepCtx) -> Result<()> {
    // `hosts: k3s_masters:gpu_node` in Ansible is a deduplicated union: a
    // node carrying both roles still only runs the play once.
    let mut seen = std::collections::HashSet::new();
    let mut nodes: Vec<Node> = Vec::new();
    for n in ctx.config.masters().into_iter().chain(ctx.config.gpu_nodes()) {
        if seen.insert(n.name.clone()) {
            nodes.push(n.clone());
        }
    }
    let port = ctx.config.settings.ssh_port;

    let results = run_parallel(nodes, |node| {
        let ctx = ctx.clone();
        async move {
            let session = SshSession::connect(&node, port, &ctx.creds, &ctx.tx).await?;
            let result = restart(&session, &node, &ctx).await;
            session.close().await;
            result
        }
    })
    .await;

    summarize(results, &ctx.tx, "restart-k3s")
}

async fn restart(session: &SshSession, node: &Node, ctx: &StepCtx) -> Result<()> {
    let sudo_pw = ctx.creds.sudo_password.as_deref();
    let tx = &ctx.tx;

    exec_ok(session, "systemctl enable --now tailscaled", true, sudo_pw, tx).await?;
    exec_ok(
        session,
        "for i in $(seq 1 30); do ip link show tailscale0 >/dev/null 2>&1 && exit 0; sleep 1; done; exit 1",
        true,
        sudo_pw,
        tx,
    )
    .await?;

    if node.has_role(ROLE_K3S_MASTER) {
        exec_ok(session, "install -d -m 0755 /etc/systemd/system/k3s.service.d", true, sudo_pw, tx).await?;
        session
            .write_file_sudo("/etc/systemd/system/k3s.service.d/override.conf", OVERRIDE_CONF, sudo_pw, tx)
            .await?;
    }
    if node.has_role(ROLE_GPU_NODE) {
        exec_ok(session, "install -d -m 0755 /etc/systemd/system/k3s-agent.service.d", true, sudo_pw, tx).await?;
        session
            .write_file_sudo("/etc/systemd/system/k3s-agent.service.d/override.conf", OVERRIDE_CONF, sudo_pw, tx)
            .await?;
    }

    exec_ok(session, "systemctl daemon-reload", true, sudo_pw, tx).await?;

    if node.has_role(ROLE_K3S_MASTER) {
        exec_ok(session, "systemctl restart k3s", true, sudo_pw, tx).await?;
    }
    if node.has_role(ROLE_GPU_NODE) {
        exec_ok(session, "systemctl restart k3s-agent", true, sudo_pw, tx).await?;
    }

    log(tx, &node.name, LogLevel::Info, "waiting 30s for the node to settle...".to_string());
    tokio::time::sleep(Duration::from_secs(30)).await;

    let flannel = session.exec("ip link show flannel.1", true, sudo_pw, tx).await?;
    let state = if flannel.success() { "EXISTS" } else { "MISSING" };
    log(tx, &node.name, LogLevel::Info, format!("flannel.1 interface: {state}"));

    Ok(())
}
