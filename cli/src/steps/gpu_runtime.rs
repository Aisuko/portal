use anyhow::Result;

use crate::config::Node;
use crate::log::LogLevel;
use crate::shellutil::quote;
use crate::ssh::SshSession;

use super::common::exec_ok;
use super::{run_parallel, summarize, StepCtx};

const CONTAINERD_DIR: &str = "/var/lib/rancher/k3s/agent/etc/containerd";
const CONTAINERD_TEMPLATE: &str = "/var/lib/rancher/k3s/agent/etc/containerd/config.toml.tmpl";

// Kept identical to the marker text `ansible.builtin.blockinfile` used to
// write on these nodes, so a node previously configured by the old
// Ansible playbook is recognised and updated in place rather than
// getting a second, duplicate block.
const MARK_BEGIN: &str = "# BEGIN ANSIBLE NVIDIA DEFAULT RUNTIME";
const MARK_END: &str = "# END ANSIBLE NVIDIA DEFAULT RUNTIME";
const BLOCK_BODY: &str = "[plugins.\"io.containerd.grpc.v1.cri\".containerd]\n  default_runtime_name = \"nvidia\"\n";

/// Port of `infra/03_gpu_nvidia_runtime.yml`.
pub async fn run(ctx: &StepCtx) -> Result<()> {
    let gpu_nodes: Vec<Node> = ctx.config.gpu_nodes().into_iter().cloned().collect();
    if gpu_nodes.is_empty() {
        crate::log::log(&ctx.tx, "", LogLevel::Info, "no gpu_node entries in cluster config".to_string());
        return Ok(());
    }
    let port = ctx.config.settings.ssh_port;

    let results = run_parallel(gpu_nodes, |node| {
        let ctx = ctx.clone();
        async move {
            let session = SshSession::connect(&node, port, &ctx.creds, &ctx.tx).await?;
            let result = configure(&session, &ctx).await;
            session.close().await;
            result
        }
    })
    .await;

    summarize(results, &ctx.tx, "gpu-runtime")
}

async fn configure(session: &SshSession, ctx: &StepCtx) -> Result<()> {
    let sudo_pw = ctx.creds.sudo_password.as_deref();
    let tx = &ctx.tx;

    exec_ok(session, "apt-get update -qq && apt-get install -y nvidia-container-toolkit", true, sudo_pw, tx).await?;
    exec_ok(session, &format!("install -d -m 0755 {}", quote(CONTAINERD_DIR)), true, sudo_pw, tx).await?;
    exec_ok(
        session,
        &format!("nvidia-ctk runtime configure --runtime=containerd --config={}", quote(CONTAINERD_TEMPLATE)),
        true,
        sudo_pw,
        tx,
    )
    .await?;

    // A nonexistent template file is expected on a fresh node (blockinfile's
    // `create: yes`); treat it as "start from empty" rather than an error.
    let existing = exec_ok(session, &format!("cat {} 2>/dev/null || true", quote(CONTAINERD_TEMPLATE)), true, sudo_pw, tx)
        .await?
        .stdout;

    let updated = upsert_block(&existing, MARK_BEGIN, MARK_END, BLOCK_BODY);
    session.write_file_sudo(CONTAINERD_TEMPLATE, &updated, sudo_pw, tx).await?;

    // Best-effort restart, matching the Ansible handler's `|| true` on
    // both service names (only one exists depending on node role).
    exec_ok(session, "systemctl restart k3s-agent || true; systemctl restart k3s || true", true, sudo_pw, tx).await?;
    Ok(())
}

fn upsert_block(existing: &str, begin: &str, end: &str, body: &str) -> String {
    if let (Some(start), Some(end_idx)) = (existing.find(begin), existing.find(end)) {
        if end_idx > start {
            let before = &existing[..start];
            let after = &existing[end_idx + end.len()..];
            let after = after.strip_prefix('\n').unwrap_or(after);
            return format!("{before}{begin}\n{body}{end}\n{after}");
        }
    }
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(begin);
    out.push('\n');
    out.push_str(body);
    out.push_str(end);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_block_into_empty_file() {
        let out = upsert_block("", MARK_BEGIN, MARK_END, BLOCK_BODY);
        assert!(out.starts_with(MARK_BEGIN));
        assert!(out.contains("default_runtime_name = \"nvidia\""));
        assert!(out.trim_end().ends_with(MARK_END));
    }

    #[test]
    fn replaces_existing_block_in_place() {
        let existing = format!("some other config\n{MARK_BEGIN}\nold body\n{MARK_END}\nmore config\n");
        let out = upsert_block(&existing, MARK_BEGIN, MARK_END, BLOCK_BODY);
        assert!(out.contains("some other config"));
        assert!(out.contains("more config"));
        assert!(!out.contains("old body"));
        assert_eq!(out.matches(MARK_BEGIN).count(), 1);
    }
}
