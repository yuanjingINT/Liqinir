//! Persistent subscriptions. No periodic sysfs or command polling.
use crate::{
    Update,
    model::{Event, Kind},
};
use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::collections::HashMap;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
    time::{Duration, sleep},
};
use zbus::{
    Connection, MatchRule, MessageStream, Proxy,
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue},
};

type Sender = mpsc::Sender<Update>;
type Properties = HashMap<String, OwnedValue>;
type Objects = HashMap<OwnedObjectPath, HashMap<String, Properties>>;

pub fn start(tx: Sender) {
    for name in ["network", "power", "session", "bluetooth", "usb", "niri"] {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut delay = 5;
            loop {
                let result = match name {
                    "network" => network(&tx).await,
                    "power" => power(&tx).await,
                    "session" => session(&tx).await,
                    "bluetooth" => bluetooth(&tx).await,
                    "usb" => usb(&tx).await,
                    _ => niri(&tx).await,
                };
                let detail = result
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "subscription ended".into());
                eprintln!("{name}: {detail}; retry in {delay}s");
                let _ = tx.send(Update::Health(name.into(), false)).await;
                sleep(Duration::from_secs(delay)).await;
                delay = (delay * 2).min(60);
            }
        });
    }
}

async fn healthy(tx: &Sender, name: &str) {
    let _ = tx.send(Update::Health(name.into(), true)).await;
}
async fn emit(tx: &Sender, event: Event) {
    let _ = tx.send(Update::Event(event)).await;
}
async fn changes(conn: &Connection, sender: &str, path: &str) -> Result<MessageStream> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(sender)?
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .path_namespace(path)?
        .build();
    Ok(MessageStream::for_match_rule(rule, conn, Some(32)).await?)
}

async fn network_name(conn: &Connection, manager: &Proxy<'_>) -> Option<String> {
    let paths: Vec<OwnedObjectPath> = manager.get_property("ActiveConnections").await.ok()?;
    for path in paths {
        let proxy = Proxy::new(
            conn,
            "org.freedesktop.NetworkManager",
            path,
            "org.freedesktop.NetworkManager.Connection.Active",
        )
        .await
        .ok()?;
        if proxy.get_property::<u32>("State").await.ok() == Some(2) {
            return proxy.get_property::<String>("Id").await.ok();
        }
    }
    Some("已连接".into())
}
async fn network(tx: &Sender) -> Result<()> {
    let conn = Connection::system().await?;
    let proxy = Proxy::new(
        &conn,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )
    .await?;
    let mut stream = changes(
        &conn,
        "org.freedesktop.NetworkManager",
        "/org/freedesktop/NetworkManager",
    )
    .await?;
    let mut previous: Option<Option<String>> = None;
    loop {
        let connected = proxy.get_property::<u32>("State").await? >= 50;
        let name = if connected {
            network_name(&conn, &proxy).await
        } else {
            None
        };
        if previous.as_ref() != Some(&name) {
            let _ = tx.send(Update::Network(name.clone())).await;
            if previous.is_some() {
                emit(
                    tx,
                    if let Some(n) = &name {
                        Event::new(Kind::NetworkConnected, "网络已连接", n)
                    } else {
                        Event::new(Kind::NetworkDisconnected, "网络已断开", "正在等待连接")
                    },
                )
                .await;
            }
            previous = Some(name);
        }
        healthy(tx, "network").await;
        stream
            .next()
            .await
            .context("NetworkManager signal stream ended")??;
        // Collapse a burst of NM device property changes before refreshing state.
        sleep(Duration::from_millis(120)).await;
        while matches!(
            tokio::time::timeout(Duration::from_millis(1), stream.next()).await,
            Ok(Some(_))
        ) {}
    }
}
async fn power(tx: &Sender) -> Result<()> {
    let conn = Connection::system().await?;
    let manager = Proxy::new(
        &conn,
        "org.freedesktop.UPower",
        "/org/freedesktop/UPower",
        "org.freedesktop.UPower",
    )
    .await?;
    let device = Proxy::new(
        &conn,
        "org.freedesktop.UPower",
        "/org/freedesktop/UPower/devices/DisplayDevice",
        "org.freedesktop.UPower.Device",
    )
    .await?;
    let mut stream = changes(&conn, "org.freedesktop.UPower", "/org/freedesktop/UPower").await?;
    let mut previous = None;
    loop {
        let battery = if device
            .get_property::<bool>("IsPresent")
            .await
            .unwrap_or(false)
        {
            device
                .get_property::<f64>("Percentage")
                .await
                .ok()
                .map(|n| n.round().clamp(0.0, 100.0) as u8)
        } else {
            None
        };
        let plugged = !manager.get_property::<bool>("OnBattery").await?;
        let _ = tx.send(Update::Power(battery, plugged)).await;
        if previous.is_some_and(|old| old != plugged) {
            let mut event = Event::new(
                if plugged {
                    Kind::PowerConnected
                } else {
                    Kind::PowerDisconnected
                },
                if plugged {
                    "电源已连接"
                } else {
                    "已切换电池供电"
                },
                battery
                    .map(|b| format!("电量 {b}%"))
                    .unwrap_or_else(|| "电源状态已更新".into()),
            );
            if !plugged && battery.is_some_and(|b| b <= 10) {
                event.priority = 3;
            }
            emit(tx, event).await;
        }
        previous = Some(plugged);
        healthy(tx, "power").await;
        stream
            .next()
            .await
            .context("UPower signal stream ended")??;
    }
}
async fn session(tx: &Sender) -> Result<()> {
    let conn = Connection::system().await?;
    let manager = Proxy::new(
        &conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await?;
    let path: OwnedObjectPath = if let Ok(id) = std::env::var("XDG_SESSION_ID") {
        manager.call("GetSession", &(id,)).await?
    } else {
        let sessions: Vec<(String, u32, String, String, OwnedObjectPath)> =
            manager.call("ListSessions", &()).await?;
        let uid = unsafe { libc::getuid() };
        let mut selected = None;
        for (_, user, _, _, path) in sessions {
            if user != uid {
                continue;
            }
            let p = Proxy::new(
                &conn,
                "org.freedesktop.login1",
                path.clone(),
                "org.freedesktop.login1.Session",
            )
            .await?;
            if p.get_property::<bool>("Active").await.unwrap_or(false)
                && matches!(
                    p.get_property::<String>("Type")
                        .await
                        .unwrap_or_default()
                        .as_str(),
                    "wayland" | "x11"
                )
            {
                selected = Some(path);
                break;
            }
        }
        selected.context("No active graphical login session")?
    };
    let mut stream = changes(&conn, "org.freedesktop.login1", path.as_str()).await?;
    let proxy = Proxy::new(
        &conn,
        "org.freedesktop.login1",
        path,
        "org.freedesktop.login1.Session",
    )
    .await?;
    loop {
        let locked = proxy.get_property::<bool>("LockedHint").await?;
        let _ = tx.send(Update::Lock(locked)).await;
        healthy(tx, "session").await;
        stream
            .next()
            .await
            .context("login1 signal stream ended")??;
    }
}
async fn bluetooth(tx: &Sender) -> Result<()> {
    let conn = Connection::system().await?;
    let manager = Proxy::new(
        &conn,
        "org.bluez",
        "/",
        "org.freedesktop.DBus.ObjectManager",
    )
    .await?;
    let mut stream = changes(&conn, "org.bluez", "/org/bluez").await?;
    let objects: Objects = manager.call("GetManagedObjects", &()).await?;
    let mut devices = HashMap::<String, (bool, String)>::new();
    for (path, interfaces) in objects {
        if let Some(p) = interfaces.get("org.bluez.Device1") {
            let connected = p
                .get("Connected")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false);
            let name = p
                .get("Alias")
                .and_then(|v| <&str>::try_from(v).ok())
                .unwrap_or("蓝牙设备")
                .to_string();
            devices.insert(path.to_string(), (connected, name));
        }
    }
    healthy(tx, "bluetooth").await;
    while let Some(message) = stream.next().await {
        let message = message?;
        let (interface, props, _): (String, Properties, Vec<String>) =
            message.body().deserialize()?;
        if interface != "org.bluez.Device1" {
            continue;
        }
        let Some(connected) = props.get("Connected").and_then(|v| bool::try_from(v).ok()) else {
            continue;
        };
        let Some(path) = message.header().path().map(|p| p.to_string()) else {
            continue;
        };
        let previous = devices.get(&path).cloned();
        let proxy = Proxy::new(&conn, "org.bluez", path.as_str(), "org.bluez.Device1").await?;
        let name = proxy
            .get_property::<String>("Alias")
            .await
            .unwrap_or_else(|_| "蓝牙设备".into());
        if previous.as_ref().map(|p| p.0).unwrap_or(false) != connected {
            emit(
                tx,
                Event::new(
                    if connected {
                        Kind::DeviceConnected
                    } else {
                        Kind::DeviceDisconnected
                    },
                    if connected {
                        "蓝牙设备已连接"
                    } else {
                        "蓝牙设备已断开"
                    },
                    &name,
                ),
            )
            .await;
        }
        devices.insert(path, (connected, name));
    }
    anyhow::bail!("BlueZ signal stream ended")
}
async fn usb(tx: &Sender) -> Result<()> {
    let mut child = Command::new("udevadm")
        .args(["monitor", "--udev", "--property", "--subsystem-match=usb"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut lines =
        BufReader::new(child.stdout.take().context("udevadm stdout unavailable")?).lines();
    let mut frame = HashMap::<String, String>::new();
    let mut names = HashMap::new();
    healthy(tx, "usb").await;
    while let Some(line) = lines.next_line().await? {
        if line.is_empty() {
            if frame.get("DEVTYPE").map(String::as_str) == Some("usb_device") {
                let path = frame.get("DEVPATH").cloned().unwrap_or_default();
                match frame.get("ACTION").map(String::as_str) {
                    Some("add") => {
                        let name = frame
                            .get("ID_MODEL_FROM_DATABASE")
                            .or_else(|| frame.get("ID_MODEL"))
                            .cloned()
                            .unwrap_or_else(|| "USB 设备".into())
                            .replace('_', " ");
                        names.insert(path, name.clone());
                        emit(tx, Event::new(Kind::DeviceConnected, "新设备已接入", name)).await;
                    }
                    Some("remove") => {
                        let name = names.remove(&path).unwrap_or_else(|| "USB 设备".into());
                        emit(tx, Event::new(Kind::DeviceDisconnected, "设备已拔出", name)).await;
                    }
                    _ => {}
                }
            }
            frame.clear();
        } else if let Some((key, value)) = line.split_once('=')
            && frame.len() < 128
        {
            frame.insert(key.into(), value.chars().take(1000).collect());
        }
    }
    anyhow::bail!("udevadm exited: {}", child.wait().await?)
}
async fn niri(tx: &Sender) -> Result<()> {
    let path = std::env::var_os("NIRI_SOCKET").context("NIRI_SOCKET unavailable")?;
    let mut socket = tokio::net::UnixStream::connect(path).await?;
    socket.write_all(b"\"EventStream\"\n").await?;
    let mut lines = BufReader::new(socket).lines();
    let mut windows = HashMap::<u64, String>::new();
    healthy(tx, "niri").await;
    while let Some(line) = lines.next_line().await? {
        let data: serde_json::Value = serde_json::from_str(&line)?;
        if let Some(changed) = data
            .get("WindowsChanged")
            .and_then(|v| v.get("windows"))
            .and_then(|v| v.as_array())
        {
            windows.clear();
            for w in changed {
                record_window(w, &mut windows, tx).await;
            }
        }
        if let Some(w) = data
            .get("WindowOpenedOrChanged")
            .and_then(|v| v.get("window"))
        {
            record_window(w, &mut windows, tx).await;
        }
        if let Some(id) = data
            .get("WindowClosed")
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_u64())
        {
            windows.remove(&id);
        }
        if let Some(focus) = data.get("WindowFocusChanged") {
            let title = focus
                .get("id")
                .and_then(|v| v.as_u64())
                .and_then(|id| windows.get(&id))
                .cloned();
            let _ = tx.send(Update::Window(title)).await;
        }
    }
    anyhow::bail!("niri IPC ended")
}
async fn record_window(w: &serde_json::Value, windows: &mut HashMap<u64, String>, tx: &Sender) {
    if let Some(id) = w.get("id").and_then(|v| v.as_u64()) {
        let title = w
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .chars()
            .take(200)
            .collect::<String>();
        windows.insert(id, title.clone());
        if w.get("is_focused").and_then(|v| v.as_bool()) == Some(true) {
            let _ = tx.send(Update::Window(Some(title))).await;
        }
    }
}
