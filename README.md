# 专注悬浮时钟（Focus Timer）

Windows 桌面悬浮倒计时时钟，用于「番茄工作法」式专注：置顶毛玻璃悬浮窗显示倒计时，
每次开始前**必填**任务，结束后自动累计每日专注时长，可缩到系统托盘继续计时（托盘图标直接显示剩余时间）。

## 功能

- **悬浮窗**：置顶无边框、可拖动、可缩放并记忆尺寸；Win11 Mica 毛玻璃（Win10 回退 Blur）
- **倒计时**：默认 30 分钟可自定义（5/15/25/30/45/60 分钟）；开始 / 暂停 / 继续 / 重置 / 结束；归零**不**自动下一轮
- **任务必填**：开始前必须填写本次主要任务，任务显示在悬浮窗并随记录保存
- **记录与统计**：本地 JSON 持久化，每日累计专注时长，按日翻看历史（倒序，最新在前）
- **系统托盘**：最小化后托盘图标直接显示剩余时间（分钟，最后 1 分钟显示秒），悬停显示完整 `mm:ss`；菜单含 显示 / 暂停继续 / 结束 / 退出，点击图标恢复窗口
- **提醒**：归零时 Windows 右下角系统通知（Toast）
- **开机自启**：默认开启，可在统计面板关闭

## 技术栈

- Tauri 2（Rust 后端 + 原生 Web 前端，无打包器、无 node_modules）
- 数据存储：本地 JSON（`sessions.json` / `settings.json`）
- 毛玻璃：`window-vibrancy`（Mica / Blur）

## 构建 / 运行

> 本机 Rust GNU 工具链装在工作区根目录下（`.rustup` / `.cargo`，未进系统 PATH），构建前先设置环境变量（把 `<项目目录>` 换成实际路径）。

```powershell
$PROJECT_DIR = "<项目目录>"   # 例如 D:\code\Timer
$env:RUSTUP_HOME = "$PROJECT_DIR\.rustup"
$env:CARGO_HOME  = "$PROJECT_DIR\.cargo"
$env:PATH = "$PROJECT_DIR\.cargo\bin;$env:PATH"

cd "$PROJECT_DIR\app\src-tauri"
cargo test            # 运行 22 个单元测试
cargo build           # debug 版（含调试符号，体积大，启动慢，仅开发用）
cargo build --release # release 版（约 23 MB，无控制台，推荐使用）
cargo run             # 运行 debug 版
```

数据默认写入 `%APPDATA%\com.focus.timer\`；开发时可重定向到工作区：

```powershell
$env:FOCUS_TIMER_DATA_DIR = "$PROJECT_DIR\data"
```

> 说明：本仓库在受限沙箱内构建时，crates.io 的 TLS 会被拦截，需先用仓库内的
> `cargo-bridge.js` 起一个本地镜像桥（监听 `127.0.0.1:18080`）并把 cargo registry 指过去；
> 在普通 Windows 环境构建无需此桥。

## 目录

- `SPEC.md` — 功能需求规格（含验收标准）
- `app/src-tauri/src/timer.rs` — 计时状态机（纯逻辑，可单测）
- `app/src-tauri/src/storage.rs` — 会话持久化（可单测）
- `app/src-tauri/src/main.rs` — 应用接线（命令 / 托盘 / 通知 / 自启 / 毛玻璃）
- `app/src-tauri/tauri.conf.json` — Tauri 配置（窗口 / 图标 / 标识符）
- `app/dist/` — 前端（`index.html` / `styles.css` / `main.js`）
- `cargo-bridge.js` — 沙箱构建用的本地 crates.io 镜像桥（普通环境不需要）
