use anyhow::Result;

use crate::config::Node;
use crate::log::{log, LogLevel};
use crate::ssh::SshSession;

use super::{run_parallel, summarize, StepCtx};

/// Port of `infra/uninstall_k3s.yml`. Targets only k3s_master nodes,
/// matching the original inventory's `k3s_all:children = k3s_master`
/// (GPU nodes are deliberately excluded — see the note in cluster.toml).
/// Every step ignores its own errors, same as `ignore_errors: yes` on
/// every task in the original playbook: a missing uninstall script or
/// already-stopped service is not a failure.
pub async fn run(ctx: &StepCtx) -> Result<()> {
    let masters: Vec<Node> = ctx.config.masters().into_iter().cloned().collect();
    let port = ctx.config.settings.ssh_port;

    let results = run_parallel(masters, |node| {
        let ctx = ctx.clone();
        async move {
            let session = SshSession::connect(&node, port, &ctx.creds, &ctx.tx).await?;
            wipe(&session, &node, &ctx).await;
            session.close().await;
            Ok(())
        }
    })
    .await;

    summarize(results, &ctx.tx, "uninstall")
}

async fn wipe(session: &SshSession, node: &Node, ctx: &StepCtx) {
    let sudo_pw = ctx.creds.sudo_password.as_deref();
    let tx = &ctx.tx;

    for script in ["/usr/local/bin/k3s-uninstall.sh", "/usr/local/bin/k3s-agent-uninstall.sh"] {
        match session.exec(script, true, sudo_pw, tx).await {
            Ok(out) if out.success() => log(tx, &node.name, LogLevel::Success, format!("ran {script}")),
            Ok(out) => log(tx, &node.name, LogLevel::Info, format!("{script} exited {} (ignored \u{2014} fine if not installed)", out.code)),
            Err(e) => log(tx, &node.name, LogLevel::Info, format!("{script}: {e} (ignored)")),
        }
    }

    let _ = session
        .exec("systemctl disable --now k3s || true; systemctl disable --now k3s-agent || true", true, sudo_pw, tx)
        .await;
    let _ = session
        .exec("rm -rf /var/lib/rancher/k3s /etc/rancher/k3s /var/lib/cni /var/run/flannel", true, sudo_pw, tx)
        .await;
    log(tx, &node.name, LogLevel::Success, "k3s state removed".to_string());
}
