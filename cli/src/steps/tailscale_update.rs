use anyhow::Result;

use crate::config::Node;
use crate::log::{log, LogLevel};
use crate::ssh::SshSession;

use super::StepCtx;

/// Replaces the old `tailscale-update` ad-hoc command, which ran as a
/// fire-and-forget async Ansible task (`-B 0 -P 0`: dispatch and don't
/// wait). We do the same here: spawn each node's update and return
/// immediately: the per-node result still lands in the log pane once it
/// finishes.
pub async fn run(ctx: &StepCtx) -> Result<()> {
    let masters: Vec<Node> = ctx.config.masters().into_iter().cloned().collect();
    let port = ctx.config.settings.ssh_port;

    for node in masters {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let tx = ctx.tx.clone();
            let sudo_pw = ctx.creds.sudo_password.clone();
            match SshSession::connect(&node, port, &ctx.creds, &tx).await {
                Ok(session) => {
                    let out = session.exec("tailscale update", true, sudo_pw.as_deref(), &tx).await;
                    session.close().await;
                    match out {
                        Ok(o) if o.success() => log(&tx, &node.name, LogLevel::Success, "tailscale update: done".to_string()),
                        Ok(o) => log(&tx, &node.name, LogLevel::Error, format!("tailscale update exited {}: {}", o.code, o.stderr.trim())),
                        Err(e) => log(&tx, &node.name, LogLevel::Error, format!("tailscale update failed: {e}")),
                    }
                }
                Err(e) => log(&tx, &node.name, LogLevel::Error, format!("connect failed: {e}")),
            }
        });
    }

    log(&ctx.tx, "", LogLevel::Info, "dispatched `tailscale update` on all masters (not waiting for completion)".to_string());
    Ok(())
}
