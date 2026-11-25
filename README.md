# TTS 语音合成桌面工具

基于 **Rust** 开发的跨平台语音合成桌面客户端。支持：

1. **Edge-TTS**（微软 Edge Read Aloud，WebSocket + `Sec-MS-GEC`）
2. **OpenAI 兼容** HTTP `POST /v1/audio/speech`（官方 OpenAI、自建网关、本地 `mlx-audio` / Qwen3 VoiceDesign 等）

并提供声卡输出设备选择、全局快捷键、语速/音量调节、MP3 导出。

---

## ✨ 核心特性

- 🎙 **双 TTS 源**：Edge-TTS 与 OpenAI Compatible 可在 UI 中一键切换；旧配置默认仍走 Edge。
- 🧪 **本地大模型友好**：API Key 可留空；支持扩展字段 **`instruct`**（自然语言音色描述）以及可选 **`temperature` / `top_p`**，适配 Qwen3-TTS VoiceDesign / Base。
- 🎧 **指定输出设备路由**：枚举系统音频输出，可路由到扬声器、耳机或虚拟声卡（VB-Cable / Voicemeeter 等）。
- ⌨ **全局快捷键呼出/隐藏**：默认 `Ctrl + Shift + T`，可自定义。
- 🎚 **参数调节**：
  - Edge：语速 / 音调 / 音量
  - OpenAI 兼容：语速 → `speed`；音调灰显（请用 Instruct）；音量仍控制本地播放
- 🗂 **语音列表**：Edge 预设 + 云端拉取；OpenAI 预设（alloy/echo/...）+ 可手填/留空 voice
- 💾 **MP3 导出**：合成结果可保存为 `.mp3`
- ⚙ **配置持久化**：JSON 保存源、OpenAI 配置（含 `instruct`）、按源记忆 voice 等

---

## 🛠 项目架构

```
tts/
├── Cargo.toml
├── README.md
├── docs/
│   └── openai-compatible-tts-implementation-plan.md
├── src/
│   ├── main.rs              # 入口与窗口标题
│   ├── lib.rs
│   ├── app.rs               # GUI、源切换、合成分发
│   ├── edge_tts/            # Edge-TTS WebSocket 客户端
│   ├── openai_tts/          # OpenAI 兼容 /v1/audio/speech 客户端
│   │   ├── client.rs        # synthesize / URL join / speed 映射
│   │   └── voices.rs        # 预设与可选远程 voices
│   ├── audio/
│   ├── hotkey/
│   ├── config.rs            # AppConfig / TtsSource / OpenAiConfig
│   └── font.rs
└── tests/
    └── app_tests.rs
```

---

## 🚀 编译与运行

```bash
cargo run
cargo build --release
```

Windows Release 产物：`target\release\edge-tts-tool.exe`（crate 名暂未改）。

---

## 📡 OpenAI 兼容源

### 配置字段（`config.json` → `openai`）

| 字段 | 说明 |
|------|------|
| `base_url` | 服务根地址，如 `https://api.openai.com` 或 `http://127.0.0.1:8081`（可带/不带 `/v1`） |
| `api_key` | **可选**；非空时才加 `Authorization: Bearer ...`；本地服务可留空 |
| `model` | 如 `tts-1` 或 `mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16` |
| `voice` | 可空；VoiceDesign 主要靠 `instruct`；与 `ref_audio`/`ref_text` 同时使用时会被省略 |
| `instruct` | 自然语言音色/风格描述；空则不发送该 JSON 字段 |
| `ref_audio` | 语音克隆参考音频的**服务端路径**（由 mlx-audio 所在机器读取；客户端不做存在性校验）；空则不发送 |
| `ref_text` | 参考音频逐字稿；空则不发送 |
| `temperature` | 可选；采样温度（建议 0–2）。`null`/缺省则不发送，使用服务端默认 |
| `top_p` | 可选；nucleus sampling（0–1）。`null`/缺省则不发送，使用服务端默认 |
| `fetch_remote_voices` | 默认 `false`；为 true 时尝试 `GET /v1/audio/voices?model=...` |

### 参数差异

| UI | Edge-TTS | OpenAI Compatible |
|----|----------|-------------------|
| voice | SSML voice | JSON `voice`（可空） |
| rate | SSML `%` | `speed = clamp(1 + rate/100, 0.25, 4)` |
| pitch | SSML Hz | **不发送**（UI 灰显） |
| volume | SSML | 仅本地播放 |
| instruct | 无 | JSON `instruct`（非空才发） |
| temperature / top_p | 无 | mlx-audio / Qwen3-TTS 采样参数（勾选后才发；经典 tts-1 不发） |
| ref_audio / ref_text | 无 | Qwen3-TTS Base 语音克隆（非空才发；有参考时省略 `voice`） |

### 本地 mlx-audio / VoiceDesign 示例

本机服务示例（需自行启动 `mlx-audio`）：

```text
TTS 源   = OpenAI 兼容
Base URL = http://127.0.0.1:8081
API Key  = （留空）
Model    = mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16
Voice    = （可留空）
Instruct = 温柔女声，语速适中，带一点微笑感
```

等价 curl：

```bash
curl -X POST 'http://127.0.0.1:8081/v1/audio/speech' \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16",
    "input": "你好，这是音色定制测试。",
    "instruct": "温柔女声，语速适中，带一点微笑感",
    "response_format": "mp3",
    "speed": 1.0
  }' \
  --output /tmp/mlx-tts-test.mp3
```

> 说明：部分兼容服务的 models/voices 列表可能为空；客户端会直接携带完整 `model` 名请求合成。VoiceDesign 空 voice 列表属正常，请填写 Instruct。

### 本地 mlx-audio / Base 语音克隆示例

```text
TTS 源    = OpenAI 兼容
Base URL  = http://127.0.0.1:8081
API Key   = （留空）
Model     = mlx-community/Qwen3-TTS-12Hz-1.7B-Base-bf16
Voice     = （留空；有 ref 时即使填写也会被省略）
Ref Audio = /absolute/path/to/reference.wav
Ref Text  = 这是我说话的声音。
```

等价 curl：

```bash
curl -X POST 'http://127.0.0.1:8081/v1/audio/speech' \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "mlx-community/Qwen3-TTS-12Hz-1.7B-Base-bf16",
    "input": "你好，这是语音克隆测试。",
    "ref_audio": "/absolute/path/to/reference.wav",
    "ref_text": "这是我说话的声音。",
    "response_format": "mp3",
    "speed": 1.0
  }' \
  --output /tmp/mlx-tts-clone.mp3
```

> `ref_audio` 按 mlx-audio 约定发送**服务端可读取的路径**。客户端与服务端可不在同一台机器，因此客户端不会校验该路径是否存在；请填写 TTS 服务所在机器上的可读路径。

### ⚠ API Key 安全

API Key 以**明文**写入用户配置目录下的 JSON（与现有配置持久化方式一致）。请勿把含真实 Key 的配置提交到版本库或分享给他人。本阶段未接入系统钥匙串。

---

## 🧪 自动化测试

```bash
# 默认（含 URL/speed/instruct 序列化、旧配置兼容、Edge 回归）
cargo test -- --nocapture

# 本机 mlx-audio 集成（需 8081 已启动）
OPENAI_BASE_URL=http://127.0.0.1:8081 \
OPENAI_MODEL=mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16 \
OPENAI_INSTRUCT='温柔女声，语速适中，带一点微笑感' \
  cargo test test_openai_tts_synthesize_mlx_audio -- --ignored --nocapture
```

覆盖要点：
- `Sec-MS-GEC` / SSML / Edge 合成回归
- `join_speech_url` / `rate_to_speed` / `instruct` 条件序列化
- 旧 `config.json` 缺少 `source`/`openai`/`instruct` 时仍可反序列化

---

## 📋 使用说明

1. **选择 TTS 源**：顶部「TTS 源」选 Edge-TTS 或 OpenAI 兼容。
2. **OpenAI 源**：填写 Base URL /（可选）API Key / Model；VoiceDesign 请填写 Instruct；voice 可留空或手填。
3. **输入文本**并选择输出设备。
4. **朗读 / 停止 / 导出 MP3**；`Cmd/Ctrl+Enter` 可朗读并清空输入。
5. **全局快捷键**：默认 `Ctrl + Shift + T` 最小化/唤出窗口。
