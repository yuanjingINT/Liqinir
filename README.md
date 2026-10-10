# Liqinir

Liqinir 是给 niri + DankMaterialShell / Shorin DMS Niri 的 Rust 事件核心和液态玻璃状态栏胶囊。它把状态栏上方的短状态反馈集中到现有 Noctalia 顶栏中央：网络连接、电源插拔、解锁、USB / 蓝牙设备、DMS 通知和 Dito 回复都可以在这里显示。

状态栏 widget 由 Noctalia 原生绘制，核心由 Rust 运行。这样它使用现有顶栏的独占空间，不再创建会覆盖浏览器的独立浮窗。独立的 Quickshell 配置仍保留在 [`shell.qml`](shell.qml) 作为非 Noctalia 环境的备用预览；niri 的 xray 参数保留在 [`niri/liqinir-glass.kdl`](niri/liqinir-glass.kdl) 中。

## 安装

当前默认集成需要 niri、Noctalia v5（plugin API 32）、Rust/Cargo 和已安装的 [Dito](https://github.com/yuanjingINT/Dito)。Quickshell 0.3+ 用于备用预览，安装脚本也会检查它。先克隆并安装：

```bash
git clone https://github.com/yuanjingINT/Liqinir.git
cd Liqinir
./scripts/install.sh
```

安装脚本会：

- 构建一个 `liqinir` Rust 二进制并放到 `~/.local/bin`；
- 安装 Noctalia bar widget，将胶囊放到现有顶栏中央；
- 保留独立 Quickshell 配置作为备用，不默认启动浮窗服务；
- 安装 DMS daemon plugin，把已经由 DMS 接收的通知转发到 Rust 核心；
- 备份 niri 配置后加入 `liqinir-glass.kdl`。

安装后 Noctalia 保留为唯一顶栏，Liqinir 只作为中央 widget 运行；`liqinir-island.service` 默认停用，避免重复创建 layer surface。启动后可以用 `liqinir status` 查看状态，`liqinir watch` 观察 JSON 状态流，或者用下面的命令注入一条测试消息：

```bash
liqinir emit '{"kind":"notification","title":"测试消息","body":"液态玻璃已启动"}'
```

### Kitty 液态玻璃

Kitty 的玻璃效果由 compositor 绘制：终端背景半透明并模糊桌面，文字和光标保持不透明，圆角和细高光边框由 niri 绘制。安装只追加独立配置，不会覆盖 Kitty 的配色或快捷键：

```bash
./scripts/install-kitty-glass.sh
```

安装脚本会在 `~/.local/state/liqinir/backups/` 保存 Kitty 和 niri 的原配置。关闭并重新打开 Kitty 后生效；`Ctrl+Shift+A` 后按 `m` / `l` 可以临时调整透明度。

点击顶栏胶囊会打开 Dito 原生终端，继续使用它的权限门和确认流程。状态栏 widget 受 Noctalia 的单行 bar 高度约束；有消息时黑色胶囊会在顶栏内横向展开，显示标题和正文，消息结束后自动恢复紧凑状态，不会创建覆盖应用内容的独立窗口。

这是早期版本，已在当前 Arch Linux / niri / Noctalia 环境验证。桌面通知目前通过 DMS bridge 转发，需启用该插件并运行 DMS；仅运行 Noctalia 时，网络、电源、设备、解锁与 Dito 事件仍可显示。`niri/liqinir-glass.kdl` 使用支持 blur/xray 的 niri 构建；安装前请确认你的构建支持其中的 layer-rule 参数。

## 预览与开发

不接触系统服务可以运行演示核心：

```bash
cargo build
./scripts/liqinir-demo.sh
```

演示模式只生成启动事件和固定状态，不读取系统总线。核心单元测试覆盖了通知去重、优先级抢占、队列上限、锁屏脱敏、暂停计时和 IPC 帧上限：

```bash
cargo test
```

配置在 [`config/config.toml`](config/config.toml)。`dito.root` 和 `dito.adapter` 可以指定 Dito 源码目录与适配器；默认会从 `PATH` 中 `dito` 的真实路径定位 npm 全局安装。

## 事件与安全边界

Rust 核心不执行任意收到的命令。IPC 只接受白名单操作（显示状态、发送受限事件、展开/收起、Dito 请求和打开 Dito 终端），每帧最多 64 KiB，每个 Dito 提示最多 8000 字符，agent 输出最多 16000 字符。运行目录和 Unix socket 为当前用户所有、权限 0700/0600，并用锁文件防止重复启动。

锁屏时清空当前岛、排队内容、历史和 Dito 回复，避免在锁屏上暴露通知或活动窗口；解锁后只显示「欢迎回来」。通知由 DMS bridge 限速为每秒 20 条并去重，Rust 队列最多 32 条、历史最多 64 条。

## 参考

- [DankMaterialShell](https://github.com/AvengeMedia/DankMaterialShell)：Quickshell/DMS 服务和插件模型。
- [shorin-dms-niri](https://github.com/SHORiN-KiWATA/shorin-dms-niri)：终端透明度、DMS/Niri 预设和 blur.kdl 的视觉参考。
- [niri window effects](https://github.com/niri-wm/niri/blob/main/docs/wiki/Window-Effects.md)：layer surface 背景模糊与 xray 的实现依据。
- [Dito](https://github.com/yuanjingINT/Dito)：已安装的 Dito agent，通过 `integrations/dito/island-adapter.mjs` 按需接入。

## 许可证

本项目采用 [GPL-3.0-only](LICENSE)。
