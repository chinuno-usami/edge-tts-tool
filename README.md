# Edge TTS 语音合成桌面工具 (Edge TTS Desktop Tool)

基于 **Rust** 开发的高性能微软 Edge-TTS 语音合成桌面客户端，专为 **Windows** 平台打造（兼具跨平台能力）。支持自然神经语音合成、声卡输出设备选择（如物理扬声器、耳机或虚拟音频线 VB-Cable）、全局快捷键显示/最小化隐藏窗口、参数调节以及 MP3 导出。

---

## ✨ 核心特性

- 🎙 **高质量神经语音**：集成微软 Edge Read Aloud 在线语音合成服务，内置动态 `Sec-MS-GEC` 鉴权算法，永久免失效、无需购买 Azure 官方 API 密钥。
- 🎧 **指定输出设备路由**：支持枚举系统所有音频输出设备，并在下拉菜单中直接选择将声音输出至扬声器、耳机或虚拟声卡（如 VB-Audio Virtual Cable、Voicemeeter，适合直播推流或变声器路由）。
- ⌨ **全局快捷键呼出/隐藏**：支持全局系统快捷键（默认 `Ctrl + Shift + T`，可在设置窗口中自定义为任意组合键），一键最小化隐藏或还原聚焦。
- 🎚 **语速、音调、音量精细调节**：
  - 语速（-50% ~ +100%）
  - 音调（-50Hz ~ +50Hz）
  - 音量（0% ~ 100%）
- 🗂 **内置丰富中文与多国预设**：预置微软晓晓（温暖女声）、云希（生动男声）、云健（旁白/影视解说）、晓伊、东北话晓北、陕西话晓妮、台湾普通话、粤语、英语与日语角色，并支持一键在线获取微软全部云端角色。
- 💾 **音频保存与导出**：支持一键将合成的高品质语音保存为本地 `.mp3` 文件。
- 🎨 **原生优雅 GUI**：基于 `eframe` / `egui`，内存占用极低（~30MB），自动匹配 Windows 微软雅黑（Microsoft YaHei）中文字体，Release 模式自动隐藏 CMD 黑框（`windows_subsystem = "windows"`）。
- ⚙ **配置持久化**：自动保存上次使用的语音角色、输出设备、语速/音调/音量及自定义快捷键。

---

## 🛠 项目架构

```
tts/
├── Cargo.toml               # 项目配置与依赖清单
├── .gitignore
├── README.md
├── src/
│   ├── main.rs              # 程序入口，窗口初始化与中文字体注册
│   ├── lib.rs               # 库导出，便于测试与模块化
│   ├── app.rs               # GUI 主界面、交互逻辑、事件响应
│   ├── edge_tts/            # Edge-TTS 客户端模块
│   │   ├── mod.rs           # 接口导出
│   │   ├── client.rs        # WebSocket 连接、Sec-MS-GEC 算法、SSML 生成
│   │   ├── voices.rs        # 预设与云端角色列表拉取
│   │   └── types.rs         # 核心数据结构 (Voice, SpeakOptions)
│   ├── audio/               # 音频驱动与播放
│   │   ├── mod.rs           # 接口导出
│   │   ├── devices.rs       # 基于 cpal 的音频输出设备枚举
│   │   └── player.rs        # 基于 rodio 的独立音频播放线程与设备路由
│   ├── hotkey/              # 全局系统快捷键管理 (global-hotkey)
│   │   └── mod.rs           # 快捷键注册、序列化与轮询
│   ├── config.rs            # 用户偏好设置持久化 (JSON)
│   └── font.rs              # 中文字体自动加载 (Windows 微软雅黑 / macOS 苹方)
└── tests/
    └── app_tests.rs         # 单元与集成测试套件
```

---

## 🚀 编译与运行

### 1. Windows 环境直接构建运行

在 Windows 电脑上安装好 Rust 工具链后，进入项目根目录：

```bash
# 调试模式运行
cargo run

# 构建 Windows Release 生产可执行文件 (无黑框控制台)
cargo build --release
```
编译产物位于 `target\release\edge-tts-tool.exe`，直接双击即可运行！

### 2. 跨平台交叉编译至 Windows (可选)

如果在 Linux 或 macOS 上构建 Windows 二进制文件，可借助 `cargo-xwin`：

```bash
# 安装 cargo-xwin
cargo install cargo-xwin

# 添加 Windows 目标架构
rustup target add x86_64-pc-windows-msvc

# 交叉编译
cargo xwin build --release --target x86_64-pc-windows-msvc
```

---

## 🧪 自动化测试

运行项目的单元测试与网络连通性测试：

```bash
cargo test -- --nocapture
```

测试覆盖：
- `Sec-MS-GEC` 算法动态生成与 64 位大写十六进制验证
- SSML 标签构建与 XML 转义验证
- 音频输出设备枚举与系统默认设备检测
- 真实 Edge-TTS WebSocket 语音合成与 MP3 数据流验证
- 用户配置文件序列化与反序列化

---

## 📋 使用说明

1. **输入文本**：在输入框中输入或点击“粘贴”写入待朗读文本。
2. **选择音频输出设备**：
   - 如果想正常听音，保持选择“默认音频输出设备”。
   - 如果需要将声音推流给直播软件、OBS、Discord、或游戏语音，在下拉菜单中选择您的虚拟声卡（例如 `CABLE Input (VB-Audio Virtual Cable)`）。
3. **选择语音与参数**：选择角色（支持在输入框快速筛选，如输入“云希”或“晓晓”），微调语速和音量。
4. **朗读与停止**：
   - 点击 **▶ 朗读 / 播放** 开始播放。
   - 点击 **⏹ 停止播放** 即可立即停止当前发声。
   - 点击 **💾 导出 MP3 文件** 将当前生成的音频直接保存至本地。
5. **快捷键窗口最小化/唤出**：
   - 默认按下 `Ctrl + Shift + T` 会将窗口最小化隐藏。
   - 在其他软件中再次按下 `Ctrl + Shift + T`，窗口将瞬间还原并自动获取焦点。
   - 点击右上角快捷键按钮可随时更改按键（如改为 `Alt + Space`、`F9` 等）。
