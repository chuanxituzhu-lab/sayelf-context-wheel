# SAYELF Context Wheel Engine

**规范仓库名 / Repository:** `sayelf-context-wheel`

**当前版本 / Current version:** `0.1.4`

**平台 / Platform:** Windows 10/11 · Tauri 2 · Rust · TypeScript · SVG

## 中文

Context Wheel 是面向 CAD 和其他专业软件的本地鼠标轮盘。按住可配置的鼠标侧键或中键，划向命令并松开执行；轮盘按当前前台软件切换。应用不需要账户或云端服务。

### 下载

**最新版本：v0.1.4 · Windows x64 安装包**

[下载 Context Wheel 安装程序](https://github.com/chuanxituzhu-lab/sayelf-context-wheel/releases/latest/download/Context.Wheel_0.1.4_x64-setup.exe)

README 会随每次版本更新指向 GitHub 最新版本的安装包。安装包尚未代码签名，Windows 可能显示未知发布者提示。

### 功能

- 根据前台应用切换轮盘；支持 Desktop、AutoCAD 与 SolidWorks Profile，并可在 Profile Studio 中添加应用和场景。
- 内圈固定 8 格；外圈默认 16 格（可改 8 格），最多三圈。每一格可单独从命令库放入功能或清空，其他格位置不变。
- 内置命令库 124 条（AutoCAD / Civil 3D / 天正、SketchUp、SolidWorks、Revit、通用），95 个矢量图标；AutoCAD 优先显示本机原生图标。
- AutoCAD 命令执行前自动按两次 Esc（`^C^C` 前缀），不会混入正在进行的命令。
- 五种配色：科技蓝、深海蓝、冰川蓝、CAD 黑白、高对比黑黄；图标与底色对比度均经自动测试 ≥ 4.5:1（实际 ≥ 11:1）。
- 死区取消、边缘避让及不抢焦点的轮盘窗口。
- 可本地学习命令使用频率并调整位置，也可固定方向以保持肌肉记忆。
- Logo 已包含在应用资源和安装包中，无需单独安装。
- 新增专业软件时，可从本机可见运行窗口中选择并自动填写进程名；也保留手动填写方式。窗口标题只用于本机临时识别，不保存。

### 安装与使用

1. 下载并双击 Windows 安装程序，按向导完成安装。
2. 首次启动时在 Profile Studio 选择触发键：侧键 4、侧键 5 或中键。
3. 按住触发键，朝命令方向移动鼠标，然后松开；回到中心或按 Esc 可取消。
4. 之后可从系统托盘打开 Profile Studio 编辑轮盘。

普通使用不需要安装开发工具。缺少 WebView2 Runtime 时，安装器可能需要联网获取该运行时。

### 本地数据与迁移

Profile 和使用习惯保存在 `%APPDATA%/local.contextwheel.engine/`。应用不上传轮盘配置或习惯数据。Profile Studio 可以把习惯导出为 JSON，再手动复制到另一台电脑导入；目标电脑需要有匹配的 Profile。快捷命令定义和快捷键不会随习惯文件导入。

命令执行器报告输入已发送，不代表目标软件一定接受或完成该操作。真实 CAD 环境、自定义快捷键和各版本兼容性仍需按实际工作区验证。

## English

Context Wheel is a local-first mouse marking menu for CAD and other professional desktop software. Hold a configurable side or middle mouse button, move toward a command, and release to run it. The wheel switches with the foreground application. No account or cloud service is required.

### Download

**Latest release: v0.1.4 · Windows x64 installer**

[Download the Context Wheel installer](https://github.com/chuanxituzhu-lab/sayelf-context-wheel/releases/latest/download/Context.Wheel_0.1.4_x64-setup.exe)

This README points to the latest versioned GitHub release. The installer is unsigned; Windows may show an unknown publisher warning.

### Features

- Switches wheels by foreground application. Desktop, AutoCAD, and SolidWorks profiles are included; more applications and scenes can be added in Profile Studio.
- A fixed eight-slot inner ring and 16-slot outer rings (switchable to 8), up to three rings. Each slot is filled from the command library or cleared individually; other slots never move.
- Built-in library of 124 commands (AutoCAD / Civil 3D, SketchUp, SolidWorks, Revit, general) with 95 vector icons; native AutoCAD icons are preferred when available.
- AutoCAD commands start with `^C^C` (two Esc presses) so they never mix into a running command.
- Five themes including high-contrast black/yellow; icon/background contrast is unit-tested at >= 4.5:1.
- A cancel dead zone, edge avoidance, and a non-activating overlay.
- Learns command frequency locally and can adjust positions, or keep fixed positions for muscle memory.
- The SAYELF logo is bundled with the application and installer; no separate logo installation is needed.
- New application profiles can be created by choosing a visible running app, which fills the executable name automatically. Manual entry remains available; window titles are used locally and are not saved.

### Install and use

1. Download and open the Windows installer, then follow the setup wizard.
2. On first launch, choose a trigger in Profile Studio: Mouse Button 4, Mouse Button 5, or middle button.
3. Hold the trigger, move toward a command, and release. Return to the center or press Esc to cancel.
4. Reopen Profile Studio from the system tray to edit the wheel.

No developer tools are needed for normal use. If WebView2 Runtime is missing, the installer may need an internet connection to obtain it.

### Local data and migration

Profiles and usage habits are stored in `%APPDATA%/local.contextwheel.engine/`. The app does not upload wheel configuration or habit data. Profile Studio can export habits to JSON for manual transfer to another computer. The destination needs matching profiles; command definitions and shortcuts are not included in the habit file.

An executor success means input was sent; it does not guarantee that the target application accepted or completed the command. Validate behavior in the actual CAD environment, including custom shortcuts and software versions.

## Build and checks / 构建与检查

On Windows with Node.js 20.19+, Rust stable MSVC, Visual Studio C++ Build Tools, and the Windows SDK, one command runs every check and builds the installer:

```powershell
powershell -ExecutionPolicy Bypass -File .\Build.ps1
```

Add `-SkipE2E` to skip the desktop E2E run (it takes over the mouse for about a minute). Individual steps:

```powershell
npm ci
node tests/schema.mjs
node tests/wheel-model.test.mjs
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run tauri -- build --bundles nsis
./tests/e2e.ps1 -Trigger xbutton1 -ExecutorCases
```

Brand assets (`icon.ico`, installer header/sidebar, tray icon, wheel hub logo) are generated from `branding/sayelf-logo-master.png` by `python tools/make-brand-assets.py` and committed, so normal builds do not need Python.

The desktop E2E tests use isolated test windows; they do not replace acceptance testing in real CAD documents. See [VERIFICATION.md](VERIFICATION.md) for test evidence and limitations, and `BUILD-DECISION-*.md` for implementation decisions.

## Versioning / 版本管理

Each update uses a new version number in the application metadata and installer filename. The README download link is updated to the latest versioned installer. Current version: **0.1.4**.

## 更新记录 / Changelog

### 0.1.4

- 新增“选择正在运行的软件”窗口，创建应用 Profile 时可直接选择本机可见程序，自动填写进程名；手动输入仍可用。
- 窗口标题和完整进程路径仅在本机枚举时短暂读取，不进入 Profile、日志或使用数据。

### 0.1.3

- AutoCAD 第二圈 16 个常用命令配置为清晰的内置矢量图标，避免本机原生图标资源异常时出现空白或文件占位图。
- 更新时只给未改动的默认 AutoCAD 命令补图标；保留用户自定义命令、已选图标和方向，之后仍可在 Profile Studio 人工调整。

### 0.1.2

- 外圈固定 16 格（可改 8 格），每格独立放入或清空功能；新建外圈预填常用功能；“用常用功能填满空格”“清空本圈”。
- 内置命令库与 95 个图标，旧配置未写 `icon` 的命令也能按软件和动作自动匹配正确图标。
- 修复冰川蓝主题图标几乎不可见（对比度 1.35:1 → 11.4:1）；新增高对比主题；AutoCAD 原生图标加浅色衬底。
- SAYELF Logo 集成到安装程序图标、安装向导页眉与侧栏、卸载程序、系统托盘；轮盘中心 Logo 由 2 MB 降至 14 KB。
- AutoCAD 命令 `^C^C` 前缀改为真实 Esc；快捷键支持 F1–F24、Home/End/PgUp/PgDn/Insert/Backspace。
- 修复：高级 YAML 与可视化编辑器互相覆盖；删除轮盘未保存就改动当前场景；添加外圈后编辑器跳回内圈。选择中键作为触发键时，提示它与 CAD 平移冲突。
