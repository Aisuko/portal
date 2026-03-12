use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub const ROLE_K3S_MASTER: &str = "k3s_master";
pub const ROLE_GPU_NODE: &str = "gpu_node";

#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default = "default_user")]
    pub user: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

/// Ansible defaults `ansible_user` to whoever is running the playbook when
/// unset; match that instead of silently assuming `root`.
fn default_user() -> String {
    std::env::var("USER").unwrap_or_else(|_| "root".to_string())
}

impl Node {
    /// Prefer the stable/MagicDNS hostname (used for TLS SANs and join
    /// endpoints), falling back to the bare address, mirroring the
    /// `hostname | default(ansible_host)` pattern used throughout the
    /// original Ansible playbooks.
    pub fn hostname_or_address(&self) -> &str {
        self.hostname.as_deref().unwrap_or(&self.address)
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    #[serde(default = "default_k3s_channel")]
    pub k3s_channel: String,
    #[serde(default = "default_flannel_iface")]
    pub flannel_iface: String,
    #[serde(default = "default_kubeconfig")]
    pub kubeconfig: String,
    #[serde(default = "default_rancher_hostname")]
    pub rancher_hostname: String,
    #[serde(default = "default_rancher_replicas")]
    pub rancher_replicas: u32,
    #[serde(default = "default_rancher_tls_source")]
    pub rancher_tls_source: String,
    #[serde(default)]
    pub cert_manager_chart_version: String,
    #[serde(default)]
    pub rancher_chart_version: String,
}

fn default_ssh_port() -> u16 {
    22
}
fn default_k3s_channel() -> String {
    "stable".to_string()
}
fn default_flannel_iface() -> String {
    "tailscale0".to_string()
}
fn default_kubeconfig() -> String {
    "config.yaml".to_string()
}
fn default_rancher_hostname() -> String {
    "rancher.example.ts.net".to_string()
}
fn default_rancher_replicas() -> u32 {
    3
}
fn default_rancher_tls_source() -> String {
    "rancher".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            ssh_port: default_ssh_port(),
            k3s_channel: default_k3s_channel(),
            flannel_iface: default_flannel_iface(),
            kubeconfig: default_kubeconfig(),
            rancher_hostname: default_rancher_hostname(),
            rancher_replicas: default_rancher_replicas(),
            rancher_tls_source: default_rancher_tls_source(),
            cert_manager_chart_version: String::new(),
            rancher_chart_version: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub settings: Settings,
    #[serde(rename = "node", default)]
    pub nodes: Vec<Node>,
    /// Directory the config file was loaded from; kubeconfig paths in
    /// `settings` are resolved relative to it.
    #[serde(skip)]
    pub base_dir: PathBuf,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading cluster config {}", path.display()))?;
        let mut cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.base_dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        if self.nodes.is_empty() {
            bail!("cluster config has no [[node]] entries");
        }
        if self.masters().is_empty() {
            bail!("cluster config has no node with roles = [\"{ROLE_K3S_MASTER}\"]");
        }
        let mut seen = std::collections::HashSet::new();
        for n in &self.nodes {
            if !seen.insert(n.name.as_str()) {
                bail!("duplicate node name in cluster config: {}", n.name);
            }
        }
        Ok(())
    }

    /// k3s control-plane nodes, in declaration order. The first entry is
    /// the etcd `--cluster-init` bootstrap node, matching
    /// `groups['k3s_masters'][0]` in the original playbooks.
    pub fn masters(&self) -> Vec<&Node> {
        self.nodes.iter().filter(|n| n.has_role(ROLE_K3S_MASTER)).collect()
    }

    pub fn gpu_nodes(&self) -> Vec<&Node> {
        self.nodes.iter().filter(|n| n.has_role(ROLE_GPU_NODE)).collect()
    }

    pub fn cluster_init(&self) -> Result<&Node> {
        self.masters()
            .into_iter()
            .next()
            .context("no k3s_master node configured")
    }

    pub fn kubeconfig_path(&self) -> PathBuf {
        let p = Path::new(&self.settings.kubeconfig);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base_dir.join(p)
        }
    }

    /// Resolution order: `--config <path>`, then `$CLUSTER_CTL_CONFIG`,
    /// then the first of `./cluster.toml` / `../cluster.toml` that
    /// exists — the latter covers the natural `cd cli && cargo run`
    /// invocation, since `cluster.toml` lives at the repo root next to
    /// `cli/`, not inside it.
    pub fn resolve_path(cli_arg: Option<&str>) -> PathBuf {
        if let Some(p) = cli_arg {
            return PathBuf::from(p);
        }
        if let Ok(p) = std::env::var("CLUSTER_CTL_CONFIG") {
            return PathBuf::from(p);
        }
        for candidate in ["cluster.toml", "../cluster.toml"] {
            let p = PathBuf::from(candidate);
            if p.is_file() {
                return p;
            }
        }
        PathBuf::from("cluster.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml_text: &str) -> Config {
        let mut cfg: Config = toml::from_str(toml_text).expect("valid toml");
        cfg.base_dir = PathBuf::from(".");
        cfg
    }

    const SAMPLE: &str = r#"
[settings]
kubeconfig = "config.yaml"

[[node]]
name = "m1"
address = "10.0.0.1"
hostname = "m1.ts.net"
user = "race"
roles = ["k3s_master"]

[[node]]
name = "m2"
address = "10.0.0.2"
user = "race"
roles = ["k3s_master"]

[[node]]
name = "gpu1"
address = "10.0.0.3"
user = "race"
roles = ["gpu_node"]
"#;

    #[test]
    fn masters_preserve_declaration_order_and_cluster_init_is_first() {
        let cfg = parse(SAMPLE);
        let masters = cfg.masters();
        assert_eq!(masters.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), vec!["m1", "m2"]);
        assert_eq!(cfg.cluster_init().unwrap().name, "m1");
    }

    #[test]
    fn gpu_nodes_excludes_masters() {
        let cfg = parse(SAMPLE);
        let gpu = cfg.gpu_nodes();
        assert_eq!(gpu.len(), 1);
        assert_eq!(gpu[0].name, "gpu1");
    }

    #[test]
    fn hostname_or_address_prefers_hostname() {
        let cfg = parse(SAMPLE);
        let m1 = cfg.nodes.iter().find(|n| n.name == "m1").unwrap();
        let m2 = cfg.nodes.iter().find(|n| n.name == "m2").unwrap();
        assert_eq!(m1.hostname_or_address(), "m1.ts.net");
        assert_eq!(m2.hostname_or_address(), "10.0.0.2"); // no hostname set, falls back
    }

    #[test]
    fn validate_rejects_missing_master() {
        let cfg = parse(
            r#"
[[node]]
name = "only-gpu"
address = "10.0.0.9"
roles = ["gpu_node"]
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_duplicate_names() {
        let cfg = parse(
            r#"
[[node]]
name = "dup"
address = "10.0.0.1"
roles = ["k3s_master"]

[[node]]
name = "dup"
address = "10.0.0.2"
roles = ["k3s_master"]
"#,
        );
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn kubeconfig_path_resolves_relative_to_config_dir() {
        let mut cfg = parse(SAMPLE);
        cfg.base_dir = PathBuf::from("/etc/cluster-ctl");
        assert_eq!(cfg.kubeconfig_path(), PathBuf::from("/etc/cluster-ctl/config.yaml"));
    }
}
