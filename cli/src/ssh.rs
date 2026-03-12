use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use russh::client::{self, Handle, Msg};
use russh::keys::known_hosts::{check_known_hosts_path, learn_known_hosts_path};
use russh::keys::ssh_key::PublicKey;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::{Channel, ChannelMsg, Disconnect};

use crate::config::Node;
use crate::log::{log, LogLevel, LogSender};
use crate::shellutil::quote;

/// Credentials collected once at TUI startup and held only in memory for
/// the lifetime of the process. Key auth is always tried first; these are
/// only used as a fallback (SSH password) and for `sudo -S` (sudo
/// password), mirroring the old `--ask-pass -K` Ansible flags.
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub ssh_password: Option<String>,
    pub sudo_password: Option<String>,
}

pub struct ExecOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl ExecOutput {
    pub fn success(&self) -> bool {
        self.code == 0
    }
}

struct ClientHandler {
    host: String,
    port: u16,
    known_hosts_path: PathBuf,
    rejection: Arc<Mutex<Option<String>>>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_public_key: &PublicKey) -> Result<bool, Self::Error> {
        match check_known_hosts_path(&self.host, self.port, server_public_key, &self.known_hosts_path) {
            Ok(true) => Ok(true),
            Ok(false) => {
                // Host key never seen before: accept it and remember it,
                // matching `StrictHostKeyChecking=accept-new`.
                if let Err(e) =
                    learn_known_hosts_path(&self.host, self.port, server_public_key, &self.known_hosts_path)
                {
                    if let Ok(mut r) = self.rejection.lock() {
                        *r = Some(format!("connected, but could not record host key: {e}"));
                    }
                }
                Ok(true)
            }
            Err(_) => {
                if let Ok(mut r) = self.rejection.lock() {
                    *r = Some(format!(
                        "host key for {}:{} does not match the key recorded in {} \u{2014} refusing to connect. \
                         If the node was reinstalled on purpose, remove its old entry from that file and retry.",
                        self.host,
                        self.port,
                        self.known_hosts_path.display()
                    ));
                }
                Ok(false)
            }
        }
    }
}

fn known_hosts_path() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .or_else(dirs::home_dir)
        .context("could not determine a config directory for cluster-ctl's known_hosts file")?
        .join("cluster-ctl");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir.join("known_hosts"))
}

/// `-S -p ''` only makes sense when we actually have a password to feed
/// it down stdin; with none given we fall back to plain `sudo`, so that a
/// NOPASSWD rule still works and — if it doesn't — sudo fails on its own
/// terms instead of misreading a command's stdin as a password attempt.
fn sudo_prefix(sudo_password: Option<&str>) -> &'static str {
    if sudo_password.is_some() {
        "sudo -S -p ''"
    } else {
        "sudo"
    }
}

fn default_key_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        for name in ["id_ed25519", "id_ecdsa", "id_rsa"] {
            let p = home.join(".ssh").join(name);
            if p.is_file() {
                out.push(p);
            }
        }
    }
    out
}

pub struct SshSession {
    handle: Handle<ClientHandler>,
    pub node_name: String,
}

impl SshSession {
    pub async fn connect(node: &Node, port: u16, creds: &Credentials, tx: &LogSender) -> Result<Self> {
        let khp = known_hosts_path()?;
        let rejection = Arc::new(Mutex::new(None));
        let handler = ClientHandler {
            host: node.address.clone(),
            port,
            known_hosts_path: khp,
            rejection: rejection.clone(),
        };
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(120)),
            ..Default::default()
        });

        let mut handle = client::connect(config, (node.address.as_str(), port), handler)
            .await
            .map_err(|e| {
                if let Some(r) = rejection.lock().ok().and_then(|g| g.clone()) {
                    anyhow!(r)
                } else {
                    anyhow!("SSH connection to {} ({}) failed: {e}", node.name, node.address)
                }
            })?;

        let mut authenticated = false;
        for key_path in default_key_paths() {
            let key_pair = match load_secret_key(&key_path, None) {
                Ok(k) => k,
                Err(_) => continue, // unreadable / passphrase-protected: skip, try next
            };
            let hash_alg = handle.best_supported_rsa_hash().await.unwrap_or_default().flatten();
            let auth = handle
                .authenticate_publickey(
                    node.user.clone(),
                    PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash_alg),
                )
                .await;
            if matches!(auth, Ok(res) if res.success()) {
                log(
                    tx,
                    &node.name,
                    LogLevel::Info,
                    format!("authenticated as {} via {}", node.user, key_path.display()),
                );
                authenticated = true;
                break;
            }
        }

        if !authenticated {
            let password = creds.ssh_password.clone().ok_or_else(|| {
                anyhow!(
                    "no usable SSH key for {} and no SSH password was provided at startup",
                    node.name
                )
            })?;
            let res = handle.authenticate_password(node.user.clone(), password).await?;
            if !res.success() {
                bail!("SSH password authentication to {} ({}) failed", node.name, node.address);
            }
            log(tx, &node.name, LogLevel::Info, format!("authenticated as {} via password", node.user));
        }

        Ok(SshSession { handle, node_name: node.name.clone() })
    }

    /// Run `script` in a login-ish shell on the remote host. When `sudo` is
    /// set the script runs as root via `sudo -S`, with `sudo_password`
    /// piped down the channel's stdin for the sudo prompt only (never as
    /// an argv, so it never shows up in `ps`).
    pub async fn exec(
        &self,
        script: &str,
        sudo: bool,
        sudo_password: Option<&str>,
        tx: &LogSender,
    ) -> Result<ExecOutput> {
        let channel = self.handle.channel_open_session().await?;
        let command = if sudo {
            format!("{} bash -c {}", sudo_prefix(sudo_password), quote(script))
        } else {
            format!("bash -c {}", quote(script))
        };
        channel.exec(true, command.into_bytes()).await?;

        if let Some(pw) = sudo_password.filter(|_| sudo) {
            channel.data_bytes(format!("{pw}\n").into_bytes()).await?;
        }
        channel.eof().await?;

        drain(channel, &self.node_name, tx).await
    }

    /// Write `contents` to a remote file as root via `sudo tee`. Used for
    /// the systemd drop-ins and the containerd config block, replacing
    /// Ansible's `copy`/`blockinfile` modules.
    pub async fn write_file_sudo(
        &self,
        path: &str,
        contents: &str,
        sudo_password: Option<&str>,
        tx: &LogSender,
    ) -> Result<()> {
        let channel = self.handle.channel_open_session().await?;
        let command = format!("{} tee {} >/dev/null", sudo_prefix(sudo_password), quote(path));
        channel.exec(true, command.into_bytes()).await?;

        // Only feed a password line down stdin when we actually have one to
        // give `sudo -S`. Otherwise (NOPASSWD sudo) we omit `-S` entirely
        // (see `sudo_prefix`) so sudo never tries to read stdin itself —
        // if we always sent a blank line here instead, the first bytes of
        // `contents` would get silently swallowed as sudo's "password".
        if let Some(pw) = sudo_password {
            channel.data_bytes(format!("{pw}\n").into_bytes()).await?;
        }
        channel.data_bytes(contents.as_bytes().to_vec()).await?;
        channel.eof().await?;

        let out = drain(channel, &self.node_name, tx).await?;
        if !out.success() {
            bail!("writing {} on {} failed (exit {}): {}", path, self.node_name, out.code, out.stderr.trim());
        }
        Ok(())
    }

    pub async fn close(&self) {
        let _ = self.handle.disconnect(Disconnect::ByApplication, "", "English").await;
    }
}

async fn drain(mut channel: Channel<Msg>, node: &str, tx: &LogSender) -> Result<ExecOutput> {
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut out_buf: Vec<u8> = Vec::new();
    let mut err_buf: Vec<u8> = Vec::new();
    let mut code: i32 = -1;

    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data } => {
                out_buf.extend_from_slice(&data);
                emit_lines(&mut out_buf, node, LogLevel::Stdout, tx, &mut stdout);
            }
            ChannelMsg::ExtendedData { data, .. } => {
                err_buf.extend_from_slice(&data);
                emit_lines(&mut err_buf, node, LogLevel::Stderr, tx, &mut stderr);
            }
            ChannelMsg::ExitStatus { exit_status } => {
                code = exit_status as i32;
            }
            _ => {}
        }
    }

    flush_remainder(&mut out_buf, node, LogLevel::Stdout, tx, &mut stdout);
    flush_remainder(&mut err_buf, node, LogLevel::Stderr, tx, &mut stderr);

    Ok(ExecOutput { code, stdout, stderr })
}

fn emit_lines(buf: &mut Vec<u8>, node: &str, level: LogLevel, tx: &LogSender, acc: &mut String) {
    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = buf.drain(..=pos).collect();
        let text = String::from_utf8_lossy(&line);
        acc.push_str(&text);
        log(tx, node, level, text.trim_end_matches(['\n', '\r']).to_string());
    }
}

fn flush_remainder(buf: &mut Vec<u8>, node: &str, level: LogLevel, tx: &LogSender, acc: &mut String) {
    if !buf.is_empty() {
        let text = String::from_utf8_lossy(buf).into_owned();
        acc.push_str(&text);
        log(tx, node, level, text);
        buf.clear();
    }
}

/// Quick TCP reachability probe, used both for the node status bar and to
/// pick the "first reachable master" join endpoint (replacing Ansible's
/// `wait_for` module).
pub async fn probe_tcp(host: &str, port: u16, timeout: Duration) -> bool {
    matches!(
        tokio::time::timeout(timeout, tokio::net::TcpStream::connect((host, port))).await,
        Ok(Ok(_))
    )
}
