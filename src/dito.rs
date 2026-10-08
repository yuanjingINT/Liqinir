use crate::{Update, config::Dito, model::Status};
use anyhow::{Context, Result};
use std::{path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
    time::{Duration, timeout},
};

fn executable(command: &str) -> Option<PathBuf> {
    if command.contains('/') {
        return std::fs::canonicalize(command).ok();
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .find_map(|dir| std::fs::canonicalize(dir.join(command)).ok())
}
pub fn root(config: &Dito) -> Result<PathBuf> {
    if let Some(root) = &config.root {
        return Ok(root.clone());
    }
    let executable = executable(&config.command)
        .context("未找到 Dito；安装 dito-agent 或在 config.toml 设置 dito.root")?;
    Ok(executable
        .parent()
        .and_then(|p| p.parent())
        .context("无法定位 Dito 安装目录")?
        .into())
}
fn adapter(config: &Dito) -> PathBuf {
    if let Some(path) = &config.adapter {
        return path.clone();
    }
    if let Some(path) = std::env::var_os("LIQINIR_DITO_ADAPTER") {
        return path.into();
    }
    let local =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("integrations/dito/island-adapter.mjs");
    if local.exists() {
        return local;
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join(".local/share/liqinir/dito/island-adapter.mjs")
}
pub async fn ask(
    config: Dito,
    prompt: String,
    status: Status,
    tx: mpsc::Sender<Update>,
) -> Result<()> {
    let root = root(&config)?;
    let mut child = Command::new("node")
        // Keep the daemon's MemoryDenyWriteExecute hardening while allowing
        // Dito's Node/V8 adapter to run without JIT-generated executable pages.
        .arg("--jitless")
        .arg(adapter(&config))
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("无法启动 Dito 适配器")?;
    let input = serde_json::to_vec(&serde_json::json!({"prompt": prompt, "status": status}))?;
    let mut stdin = child.stdin.take().context("Dito stdin unavailable")?;
    stdin.write_all(&input).await?;
    stdin.shutdown().await?;
    drop(stdin);
    let mut lines = BufReader::new(child.stdout.take().context("Dito stdout unavailable")?).lines();
    let stderr = child.stderr.take().context("Dito stderr unavailable")?;
    let errors = tokio::spawn(async move {
        let mut bytes = Vec::new();
        let _ = stderr.take(4000).read_to_end(&mut bytes).await;
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let work = async {
        let mut received = 0usize;
        let mut error = None;
        while let Some(line) = lines.next_line().await? {
            if line.len() > 65536 {
                anyhow::bail!("Dito 输出超出限制");
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            match value.get("type").and_then(|v| v.as_str()) {
                Some("delta") => {
                    let delta = value
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let delta = delta
                        .chars()
                        .take(16000usize.saturating_sub(received))
                        .collect::<String>();
                    received += delta.chars().count();
                    if !delta.is_empty() {
                        let _ = tx.send(Update::AgentDelta(delta)).await;
                    }
                }
                Some("error") => {
                    error = Some(
                        value
                            .get("text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Dito 请求失败")
                            .to_string(),
                    )
                }
                _ => {}
            }
        }
        let exit = child.wait().await?;
        let detail = errors.await.unwrap_or_default();
        if let Some(error) = error {
            anyhow::bail!("{error}");
        }
        if !exit.success() {
            anyhow::bail!(
                "Dito 适配器退出：{}",
                detail.chars().take(500).collect::<String>()
            );
        }
        if received == 0 {
            anyhow::bail!("Dito 未返回正文；请先在终端运行 dito doctor / dito config");
        }
        Ok(())
    };
    timeout(
        Duration::from_secs(config.timeout_seconds.clamp(10, 600)),
        work,
    )
    .await
    .context("Dito 请求超时")?
}

pub fn open(config: &Dito) -> Result<()> {
    std::process::Command::new("kitty")
        .args(["--class", "liqinir-dito", &config.command])
        .spawn()
        .context("请安装 kitty，或直接在终端运行 dito")?;
    Ok(())
}
