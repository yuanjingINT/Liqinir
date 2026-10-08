mod config;
mod dito;
mod model;
mod providers;

use anyhow::{Context, Result};
use model::{Event, Kind, Snapshot, State};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Semaphore, mpsc, oneshot, watch},
    time::{Duration, timeout},
};

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Snapshot,
    Emit { event: Event },
    Dismiss,
    Pause { value: bool },
    Ask { prompt: String },
    CancelAgent,
    OpenDito,
    ClearHistory,
    SetMotion { reduced: bool },
    Lock { locked: bool },
}
pub enum Update {
    Event(Event),
    Network(Option<String>),
    Power(Option<u8>, bool),
    Lock(bool),
    Window(Option<String>),
    Health(String, bool),
    AgentDelta(String),
    AgentDone(Option<String>),
    Command(Command, oneshot::Sender<Result<(), String>>),
}

fn runtime_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("LIQINIR_RUNTIME_DIR") {
        return Ok(path.into());
    }
    Ok(
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR 未设置")?)
            .join("liqinir"),
    )
}

struct SocketGuard {
    _lock: File,
    path: PathBuf,
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
fn bind() -> Result<(UnixListener, SocketGuard)> {
    let dir = runtime_dir()?;
    anyhow::ensure!(dir.is_absolute(), "运行目录必须使用绝对路径");
    std::fs::create_dir_all(&dir)?;
    let metadata = std::fs::symlink_metadata(&dir)?;
    anyhow::ensure!(
        metadata.is_dir() && metadata.uid() == unsafe { libc::getuid() },
        "运行目录应是当前用户拥有的真实目录"
    );
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join("daemon.lock"))?;
    anyhow::ensure!(
        unsafe {
            libc::flock(
                std::os::fd::AsRawFd::as_raw_fd(&lock),
                libc::LOCK_EX | libc::LOCK_NB,
            )
        } == 0,
        "Liqinir 已运行"
    );
    let path = dir.join("island.sock");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok((listener, SocketGuard { _lock: lock, path }))
}

async fn actor(
    mut input: mpsc::Receiver<Update>,
    tx: mpsc::Sender<Update>,
    output: watch::Sender<Snapshot>,
    config: config::Config,
    demo: bool,
) {
    let mut state = State::new(config.reduced_motion);
    let mut agent_task: Option<tokio::task::JoinHandle<()>> = None;
    state.push(
        Event::new(
            Kind::Boot,
            "你好，Liqinir",
            "桌面会话已启动 · Dito 随时待命",
        ),
        Instant::now(),
    );
    if demo {
        state.status.network = Some("Studio Wi-Fi".into());
        state.status.battery = Some(86);
        state.status.charging = Some(false);
        state.status.providers = vec!["demo".into()];
    }
    output.send_replace(state.snapshot());
    loop {
        let deadline = state.deadline;
        let next = tokio::select! {
            update = input.recv() => update,
            _ = async { if let Some(deadline) = deadline { tokio::time::sleep_until(deadline.into()).await } else { std::future::pending::<()>().await } } => {
                state.advance(Instant::now()); output.send_replace(state.snapshot()); continue;
            }
        };
        let Some(update) = next else {
            break;
        };
        let now = Instant::now();
        let before = serde_json::to_string(&state.snapshot()).unwrap_or_default();
        match update {
            Update::Event(event) => {
                state.push(event, now);
            }
            Update::Network(name) => state.status.network = name,
            Update::Power(battery, charging) => {
                state.status.battery = battery;
                state.status.charging = Some(charging);
            }
            Update::Lock(locked) => state.lock(locked, now),
            Update::Window(title) => state.status.active_window = title,
            Update::Health(name, available) => {
                state.status.providers.retain(|n| n != &name);
                if available {
                    state.status.providers.push(name);
                    state.status.providers.sort();
                }
            }
            Update::AgentDelta(text) => {
                if state.agent.busy {
                    state.agent.reply.push_str(&text);
                }
            }
            Update::AgentDone(error) => {
                if state.agent.busy {
                    state.agent.busy = false;
                    state.agent.error = error.clone();
                    state.push(
                        Event::new(
                            Kind::Agent,
                            if error.is_some() {
                                "Dito 需要你的关注"
                            } else {
                                "Dito 已回复"
                            },
                            error.unwrap_or_else(|| state.agent.reply.chars().take(120).collect()),
                        ),
                        now,
                    );
                }
                agent_task = None;
            }
            Update::Command(command, reply) => {
                let result = match command {
                    Command::Snapshot => Ok(()),
                    Command::Emit { event } => {
                        state.push(event, now);
                        Ok(())
                    }
                    Command::Dismiss => {
                        state.advance(now);
                        Ok(())
                    }
                    Command::Pause { value } => {
                        state.pause(value, now);
                        Ok(())
                    }
                    Command::Lock { locked } => {
                        state.lock(locked, now);
                        Ok(())
                    }
                    Command::ClearHistory => {
                        state.clear_history();
                        Ok(())
                    }
                    Command::SetMotion { reduced } => {
                        state.status.reduced_motion = reduced;
                        Ok(())
                    }
                    Command::OpenDito => dito::open(&config.dito).map_err(|e| e.to_string()),
                    Command::CancelAgent => {
                        if let Some(task) = agent_task.take() {
                            task.abort();
                        }
                        state.agent.busy = false;
                        state.agent.error = Some("已取消".into());
                        Ok(())
                    }
                    Command::Ask { prompt } => {
                        if state.status.locked {
                            Err("请先解锁桌面".into())
                        } else if state.agent.busy {
                            Err("Dito 正在回复，请等待或取消当前请求".into())
                        } else if prompt.trim().is_empty() || prompt.chars().count() > 8000 {
                            Err("消息长度应为 1–8000 字符".into())
                        } else {
                            state.agent = model::Agent {
                                busy: true,
                                prompt: prompt.clone(),
                                ..Default::default()
                            };
                            let config = config.dito.clone();
                            let status = state.status.clone();
                            let tx = tx.clone();
                            agent_task = Some(tokio::spawn(async move {
                                let error = dito::ask(config, prompt, status, tx.clone())
                                    .await
                                    .err()
                                    .map(|e| format!("{e:#}"));
                                let _ = tx.send(Update::AgentDone(error)).await;
                            }));
                            Ok(())
                        }
                    }
                };
                let _ = reply.send(result);
            }
        }
        let snapshot = state.snapshot();
        if serde_json::to_string(&snapshot).unwrap_or_default() != before {
            output.send_replace(snapshot);
        }
    }
    if let Some(task) = agent_task {
        task.abort();
    }
}

async fn read_frame(reader: &mut (impl AsyncBufRead + Unpin)) -> Result<Option<Vec<u8>>> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    let count = reader.take(65537).read_until(b'\n', &mut bytes).await?;
    if count == 0 {
        return Ok(None);
    }
    anyhow::ensure!(
        count <= 65536 && bytes.last() == Some(&b'\n'),
        "IPC frame exceeds 64 KiB or is incomplete"
    );
    Ok(Some(bytes))
}
async fn send(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    value: &impl serde::Serialize,
) -> Result<()> {
    let mut data = serde_json::to_vec(value)?;
    data.push(b'\n');
    timeout(Duration::from_secs(3), writer.write_all(&data)).await??;
    Ok(())
}
async fn client(
    socket: UnixStream,
    tx: mpsc::Sender<Update>,
    mut state: watch::Receiver<Snapshot>,
) -> Result<()> {
    let (reader, mut writer) = socket.into_split();
    let mut reader = BufReader::new(reader);
    let snapshot = state.borrow_and_update().clone();
    send(&mut writer, &snapshot).await?;
    loop {
        tokio::select! {
            bytes = read_frame(&mut reader) => {
                let Some(bytes) = bytes? else { return Ok(()); };
                let result = match serde_json::from_slice::<Command>(&bytes) {
                    Ok(command) => {
                        let (reply, response) = oneshot::channel();
                        tx.send(Update::Command(command, reply)).await?;
                        response.await?
                    }
                    Err(error) => Err(error.to_string()),
                };
                send(&mut writer, &json!({"type": "response", "ok": result.is_ok(), "error": result.err()})).await?;
            }
            changed = state.changed() => {
                changed?; let snapshot = state.borrow_and_update().clone();
                send(&mut writer, &snapshot).await?;
            }
        }
    }
}

async fn daemon(config: config::Config, demo: bool) -> Result<()> {
    let (listener, _guard) = bind()?;
    let (tx, input) = mpsc::channel(128);
    let (output, state) = watch::channel(State::new(config.reduced_motion).snapshot());
    tokio::spawn(actor(input, tx.clone(), output, config, demo));
    if !demo {
        providers::start(tx.clone());
    }
    eprintln!(
        "liqinir listening at {}{}",
        runtime_dir()?.join("island.sock").display(),
        if demo {
            " (demo: no live providers)"
        } else {
            ""
        }
    );
    let capacity = Arc::new(Semaphore::new(32));
    loop {
        tokio::select! {
            socket = listener.accept() => {
                let (socket, _) = socket?;
                let Ok(permit) = capacity.clone().try_acquire_owned() else { continue; };
                let tx = tx.clone(); let state = state.clone();
                tokio::spawn(async move { let _permit = permit; if let Err(error) = client(socket, tx, state).await { eprintln!("IPC: {error}"); } });
            }
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
    }
}
async fn request(value: Value, follow: bool) -> Result<()> {
    let mut socket = UnixStream::connect(runtime_dir()?.join("island.sock"))
        .await
        .context("Liqinir 未运行；先执行 liqinir daemon")?;
    send(&mut socket, &value).await?;
    let mut lines = BufReader::new(socket).lines();
    let mut latest = Value::Null;
    while let Some(line) = lines.next_line().await? {
        let value: Value = serde_json::from_str(&line)?;
        if value.get("type").and_then(Value::as_str) == Some("state") {
            latest = value;
            if follow {
                println!("{}", serde_json::to_string(&latest)?);
            }
        } else if !follow {
            anyhow::ensure!(
                value.get("ok") == Some(&Value::Bool(true)),
                "{}",
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("请求失败")
            );
            // Command acknowledgement is sufficient; state streams are asynchronous.
            println!("{}", serde_json::to_string_pretty(&latest)?);
            return Ok(());
        }
    }
    anyhow::bail!("IPC connection ended")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config_path = args
        .windows(2)
        .find(|a| a[0] == "--config")
        .map(|a| a[1].as_str());
    match args.first().map(String::as_str) {
        Some("daemon") => {
            daemon(
                config::Config::load(config_path)?,
                args.iter().any(|a| a == "--demo"),
            )
            .await
        }
        Some("status") => request(json!({"op":"snapshot"}), false).await,
        Some("watch") => request(json!({"op":"snapshot"}), true).await,
        Some("emit") => {
            let event: Event =
                serde_json::from_str(args.get(1).context("用法：liqinir emit '<event JSON>'")?)?;
            request(json!({"op":"emit","event":event}), false).await
        }
        Some("ask") => request(json!({"op":"ask","prompt":args[1..].join(" ")}), false).await,
        Some("dismiss") => request(json!({"op":"dismiss"}), false).await,
        Some("dito") => dito::open(&config::Config::load(config_path)?.dito),
        _ => {
            println!(
                "Liqinir 0.1.0\n  daemon [--demo] [--config PATH]  启动事件核心\n  status / watch                状态与事件流\n  emit '<event JSON>'            注入事件\n  ask <消息>                    与 Dito 对话\n  dismiss                       收起当前事件\n  dito                          打开 Dito 完整终端"
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_oversized_frames() {
        let (mut a, b) = tokio::io::duplex(70000);
        let producer = tokio::spawn(async move {
            a.write_all(&vec![b'x'; 66000]).await.unwrap();
        });
        assert!(read_frame(&mut BufReader::new(b)).await.is_err());
        producer.await.unwrap();
    }
    #[test]
    fn rejects_unknown_commands() {
        assert!(serde_json::from_str::<Command>(r#"{"op":"shell","command":"rm -rf"}"#).is_err());
    }
}
