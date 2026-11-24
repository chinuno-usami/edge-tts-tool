# OpenAI 兼容 `/v1/audio/speech` TTS 源接入 — 实施计划

> 状态：已完成（P0–P4，2026-11-24）  
> 目标读者：后续实现 agent  
> 仓库：`/Users/sjx/source/tts`（Rust 桌面端 Edge-TTS 工具）  
> 关联日期：2026-11-24  
> 更新：2026-11-24 — 补充本机 `mlx-audio` 测试环境与 VoiceDesign `instruct` 需求

---

## 1. 目标与非目标

### 1.1 目标

在现有桌面 TTS 客户端中新增 **OpenAI 兼容** 的语音合成源，通过：

```http
POST {base_url}/v1/audio/speech
Authorization: Bearer {api_key}   # api_key 非空时才附加
Content-Type: application/json
```

调用兼容服务（官方 OpenAI、自建网关、本地 `mlx-audio`、各类 OpenAI-compatible TTS），返回音频字节，复用现有播放 / 导出 MP3 流程。

用户可在 UI 中在以下两者间切换：

1. **Edge-TTS**（现有 WebSocket 实现，行为不变）
2. **OpenAI Compatible**（新增 HTTP `/v1/audio/speech`）

**本阶段还必须支持**：请求体可选扩展字段 **`instruct`**（自然语言音色/风格描述），用于 Qwen3-TTS **VoiceDesign** 等模型定制声音音色。本地验收以本机 `mlx-audio` 为准（见 §1.3）。

### 1.2 非目标（本阶段不做）

- 不改造成通用多 provider 插件系统（先 enum 分发即可）
- 不实现流式播放（仍一次拿完整音频再播；即使服务支持 `stream=true` 也不接）
- 不接入 Azure Speech SDK / 其他非 OpenAI 协议
- 不做系统钥匙串加密存 Key（先明文本地 JSON，README 注明风险）
- 不强制改 crate 名 `edge-tts-tool`（可只改窗口标题/README 副标题）
- 不实现 `mlx-audio` 全部扩展字段（如 `ref_audio` / `ref_text` / `temperature` / `top_p` / `top_k` / `gender` / `lang_code` / `seed` 等）；**唯一必做的兼容扩展是 `instruct`**
- 不在本阶段把 OpenAI 兼容源的 UI pitch 滑条映射到 `mlx-audio` 的 `pitch` 浮点字段（仍灰显；音色定制走 `instruct`）

### 1.3 本地验证环境（mlx-audio，已探活）

本机已有可用于联调/验收的 OpenAI 兼容服务：

| 项 | 值 |
|----|----|
| 服务 | `mlx-audio`（FastAPI，OpenAPI title=`FastAPI`） |
| Base URL | `http://127.0.0.1:8081` |
| Speech | `POST /v1/audio/speech` |
| Voices | `GET /v1/audio/voices?model=...` |
| Models | `GET/POST/DELETE /v1/models` |
| 测试模型 | `mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16` |
| API Key | **可不填**（本地服务无鉴权时不要强制要求 Key） |

探活结论（实现时勿假设与此相反）：

1. `GET /v1/models` 当前可能返回空列表；**合成请求仍应直接携带 `model` 全名**，由服务按需加载，不要依赖 models 列表非空。
2. `GET /v1/audio/voices?model=mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16` 可能返回空 `data`（VoiceDesign 不是 voice-pack 模型）。因此 **不能只靠 voice 下拉**；必须提供 **`instruct` 文本输入** 来定制音色。
3. OpenAPI `SpeechRequest`（本机 8081）关键字段：
   - **必填**：`model`, `input`
   - **可选扩展**：`instruct`, `voice`, `speed`, `pitch`, `gender`, `lang_code`, `ref_audio`, `ref_text`, `temperature`, `top_p`, `top_k`, `repetition_penalty`, `seed`, `response_format`(默认 `mp3`), `stream`, ...
4. VoiceDesign 语义：用自然语言 `instruct` 描述目标音色/风格（例：`温柔女声，语速适中，带一点微笑感` / `A cheerful young female voice with high pitch and energetic tone.`）。同 instruct 结果相近但不保证逐样本完全一致。

**推荐本地冒烟请求**（实现后可用 curl 对照客户端）：

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

客户端联调默认填写建议（仅测试配置，**不要**把本地 URL 写成强制全局 Default，以免未起服务的用户困惑；可用 UI 预填或 README 示例）：

```text
source   = OpenAiCompatible
base_url = http://127.0.0.1:8081
api_key  = （留空）
model    = mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16
voice    = （可留空；VoiceDesign 主要靠 instruct）
instruct = 用户输入的音色描述
```

---

## 2. 现状（实现前必读）

### 2.1 架构要点

| 模块 | 路径 | 现状 |
|------|------|------|
| GUI / 合成入口 | `src/app.rs` | `start_synthesis` 直接调用 `edge_tts::synthesize`；`refresh_voices_async` 直接调用 `fetch_voices_list` |
| Edge 客户端 | `src/edge_tts/client.rs` | WebSocket + Sec-MS-GEC + SSML；输出固定 `audio-24khz-48kbitrate-mono-mp3` |
| 类型 | `src/edge_tts/types.rs` | `Voice`、`SpeakOptions { voice, rate, pitch, volume }` |
| 配置 | `src/config.rs` | `AppConfig` 仅 voice/device/rate/pitch/volume/hotkey/last_text，**无源/API 配置** |
| 播放 | `src/audio/` | rodio 播放字节；导出默认 `.mp3` |
| 测试 | `tests/app_tests.rs` | Edge token/SSML/真机合成；无 provider 抽象测试 |

### 2.2 关键调用链（改造点）

```text
UI 朗读按钮 / Cmd|Ctrl+Enter
  -> TtsApp::start_synthesis(auto_play)
      -> SpeakOptions { voice, rate, pitch, volume }
      -> edge_tts::synthesize(text, &options)   // 硬编码
      -> AsyncMessage::SynthesisSuccess { audio, auto_play }
          -> AudioController::play / last_audio 缓存 / 导出 MP3
```

**结论**：当前无 provider 层。最小改动是在 `start_synthesis` / `refresh_voices_async` 按源分发；不必一上来上 trait object。

---

## 3. 协议与参数映射

### 3.1 请求体（OpenAI Audio Speech + 兼容扩展）

基础字段（官方 OpenAI 常见）：

```json
{
  "model": "tts-1",
  "input": "要合成的文本",
  "voice": "alloy",
  "response_format": "mp3",
  "speed": 1.0
}
```

面向本地 `mlx-audio` / VoiceDesign 的推荐体（**MVP 必支持 `instruct`**）：

```json
{
  "model": "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16",
  "input": "要合成的文本",
  "instruct": "温柔女声，语速适中，带一点微笑感",
  "response_format": "mp3",
  "speed": 1.0
}
```

序列化规则：

1. `model` / `input` / `response_format` / `speed`：**始终发送**
2. `voice`：仅当非空时发送（`skip_serializing_if = Option::is_none` 或空串跳过）—— VoiceDesign 可无 voice
3. `instruct`：仅当非空时发送；有内容时必须出现在 JSON 中（**禁止**被中间层剥掉）
4. 不要因官方 OpenAI 无 `instruct` 就从结构体删除该字段；对不认识的字段，官方服务通常会 4xx，用户改用官方模型时可自行清空 instruct

响应：成功时为 **原始音频二进制**（非 JSON）；失败时多为 JSON error（`mlx-audio` 也可能返回 FastAPI `detail`）。

### 3.2 UI 参数映射

| UI / SpeakOptions | Edge-TTS | OpenAI Compatible |
|-------------------|----------|-------------------|
| `voice` | SSML voice short name | JSON `voice`（可空；VoiceDesign 可不用） |
| `rate`（-50..=100，单位 %） | SSML `rate` | 映射 `speed = clamp(1.0 + rate/100.0, 0.25, 4.0)` |
| `pitch` | SSML Hz | **不发送**；UI 灰显并提示「请使用 Instruct 描述音色/风格」 |
| `volume` | SSML % | **不发送**；继续只作用于本地播放音量 |
| （新增）`model` | 无 | JSON `model`；结构体 Default 可为 `tts-1`，本地测试填 Qwen3 VoiceDesign 全名 |
| （新增）`instruct` | 无 | JSON `instruct`；**多行文本框**，用于定制音色；持久化到 config |
| （新增）`response_format` | 固定 mp3 | 固定传 `"mp3"`（与现有播放/导出一致） |

### 3.3 Base URL 规范化规则（必须实现）

配置里的 `base_url` 可能是：

- `https://api.openai.com`
- `https://api.openai.com/v1`
- `https://my-gateway.example.com/openai`
- 带尾斜杠的变体

实现要求：

1. trim + 去掉末尾 `/`
2. 若已以 `/v1` 结尾，则拼 `/audio/speech`
3. 否则拼 `/v1/audio/speech`
4. 单测覆盖上述几种输入

示例函数语义：

```rust
fn join_speech_url(base_url: &str) -> String;
// "https://api.openai.com"      -> "https://api.openai.com/v1/audio/speech"
// "https://api.openai.com/"     -> 同上
// "https://api.openai.com/v1"   -> "https://api.openai.com/v1/audio/speech"
// "https://api.openai.com/v1/"  -> 同上
```

### 3.4 错误处理

- HTTP 非 2xx：依次尝试解析 `{"error":{"message":"..."}}`、FastAPI `{"detail":...}`；失败则回退 `HTTP {status}: {body_prefix}`
- 超时：连接/请求超时建议 **60s** 起步（本地大模型首包/冷启动可能明显慢于云端 Edge；可配置但 MVP 硬编码即可）
- 空 `input`：与 Edge 一致，直接返回 `Ok(vec![])`，不发请求
- API Key：**可选**。仅当 `api_key` 非空时附加 `Authorization: Bearer ...`；为空时**允许发请求**（本地 `mlx-audio` 场景）。不要再做「Key 为空直接拒绝」的硬校验。
- `instruct` 为空：允许请求（仍可合成）；UI 可对 VoiceDesign 模型给出非阻断提示「建议填写 Instruct 以定制音色」

---

## 4. 推荐实现策略（MVP）

**采用 enum 分发，不做完整 trait object。**

理由：改动面小、易交接、行为清晰；后续若再加源，再抽 `TtsProvider` trait。

```text
match config.source {
  TtsSource::Edge => edge_tts::synthesize(...),
  TtsSource::OpenAiCompatible => openai_tts::synthesize(...),
}
```

---

## 5. 文件改动清单

### 5.1 新增

```text
src/openai_tts/
  mod.rs          # 模块导出
  client.rs       # synthesize + join_speech_url + 错误解析
  types.rs        # OpenAiSpeechRequest 等（可选，也可放 client）
  voices.rs       # 预设 voice 列表；可选远程拉取
```

可选（若希望公共类型上移，非 MVP 必须）：

```text
src/tts/
  mod.rs          # SpeakOptions / Voice / TtsSource 公共定义
```

**MVP 建议**：先不移动 `edge_tts::types`，OpenAI 侧接收 `&str` + 独立 options 或复用 `SpeakOptions` + 额外 `OpenAiConfig`。

### 5.2 修改

| 文件 | 变更 |
|------|------|
| `src/lib.rs` | `pub mod openai_tts;` |
| `src/config.rs` | 增加 `TtsSource`、`OpenAiConfig`（含 `instruct`）、serde default；按源记住 voice |
| `src/app.rs` | 源切换 UI；`instruct` 输入框；合成/拉 voice 分发；pitch 条件禁用；状态文案去硬编码 Edge |
| `tests/app_tests.rs` | URL join、speed 映射、请求序列化（含 `instruct` skip）、config 向后兼容；可选本机 8081 集成测试 |
| `README.md` | 使用说明、配置字段、参数差异 |
| `Cargo.toml` | **优先不加依赖**；已有 `reqwest`/`serde`/`tokio` 足够 |

### 5.3 尽量不改

- `src/edge_tts/client.rs` 核心协议逻辑
- `src/audio/*` 播放管线（继续吃 MP3 bytes）
- `src/hotkey/*`

---

## 6. 配置设计

### 6.1 建议数据结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TtsSource {
    #[default]
    Edge,
    OpenAiCompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiConfig {
    pub base_url: String,
    pub api_key: String,           // 可空；空则不发 Authorization
    pub model: String,
    pub voice: String,             // 可空；VoiceDesign 可不用
    /// 自然语言音色/风格描述；发送为 JSON `instruct`（空则省略字段）
    #[serde(default)]
    pub instruct: String,
    /// 是否尝试 GET 远程 voices（很多兼容服务有；官方未必有；VoiceDesign 常为空）
    #[serde(default)]
    pub fetch_remote_voices: bool,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            // 通用默认保持官方形态；本地 mlx-audio 联调请按 §1.3 填写
            base_url: "https://api.openai.com".to_string(),
            api_key: String::new(),
            model: "tts-1".to_string(),
            voice: "alloy".to_string(),
            instruct: String::new(),
            fetch_remote_voices: false,
        }
    }
}

/// README / 手动测试可用的本机预填示例（不必作为 Default）
/// base_url = "http://127.0.0.1:8081"
/// model    = "mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16"
/// voice    = ""
/// instruct = "温柔女声，语速适中，带一点微笑感"

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    // --- 现有字段保留 ---
    pub voice: String,                 // 兼容旧字段：表示「当前源」选中的 voice
    pub output_device: Option<String>,
    pub rate: i32,
    pub pitch: i32,
    pub volume: i32,
    pub hotkey: HotkeyConfig,
    pub last_text: String,

    // --- 新增（全部 #[serde(default)]）---
    #[serde(default)]
    pub source: TtsSource,
    #[serde(default)]
    pub openai: OpenAiConfig,
    /// Edge 源上次使用的 voice（切回 Edge 时恢复）
    #[serde(default = "default_edge_voice")]
    pub edge_voice: String,
}
```

### 6.2 向后兼容硬性要求

1. 旧 `config.json` **缺少新字段时必须能 load**，默认 `source = Edge`
2. 旧配置只有 `voice` 时：将其视为 Edge voice，并同步到 `edge_voice`
3. `persist_config` 时：
   - 写 `source` / `openai.*`（**含 `instruct`**）
   - 按当前源更新 `voice`（方便旧逻辑/调试）以及对应的 `edge_voice` 或 `openai.voice`

### 6.3 安全说明（README 必写）

API Key 以明文写入用户配置目录（与现工程 JSON 持久化一致）。文档中明确风险；本阶段不引入 keychain。

---

## 7. `openai_tts` 模块规格

### 7.1 对外 API（建议签名）

```rust
pub async fn synthesize(
    text: &str,
    rate: i32,                 // UI rate%，内部转 speed
    cfg: &OpenAiConfig,        // base_url / api_key / model / voice / instruct
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>;

pub fn rate_to_speed(rate: i32) -> f32;
pub fn join_speech_url(base_url: &str) -> String;

pub fn get_preset_voices() -> Vec<Voice>;  // 复用 edge_tts::Voice 或平行结构

/// 可选；仅当 cfg.fetch_remote_voices == true
/// 注意：GET /v1/audio/voices?model=... ；VoiceDesign 可能返回空列表
pub async fn fetch_voices_list(
    cfg: &OpenAiConfig,
) -> Result<Vec<Voice>, Box<dyn std::error::Error + Send + Sync>>;
```

> 若希望少动类型：`Voice` 可继续用 `crate::edge_tts::Voice`（名字别扭但 MVP 可接受），或在公共层复制一个简化 `Voice`。优先 **复用** `edge_tts::types::Voice`，用 `Voice::new(short, friendly, locale, gender)` 填预设，避免 UI 大改。

### 7.2 HTTP 细节

- Method: `POST`
- Header：`Content-Type: application/json`；**仅当 `api_key` 非空**时加 `Authorization: Bearer {api_key}`
- Body: 见 §3.1；`speed` 用 `rate_to_speed`；按规则附带 `voice` / `instruct`
- 成功：`response.bytes()`
- 校验：非空音频；可选检查 MP3 头（`ID3` 或 `0xFFEx`），失败仅 log warn，不硬失败（部分兼容服务封装可能不同）
- 超时：本地大模型建议 ≥60s

### 7.3 预设 voices（默认）

至少包含常见 OpenAI 音色（名称以 short_name 存储）：

- `alloy`, `echo`, `fable`, `onyx`, `nova`, `shimmer`

`friendly_name` 可用英文名；`locale` 可用 `en-US`；`gender` 可填 `Unknown` 或合理猜测。

额外：

1. UI 允许用户 **手填自定义 voice 字符串**（兼容服务常有自定义音色 ID）
2. 允许 voice **留空**（适配 VoiceDesign：`voices` 接口可能为空，音色靠 `instruct`）

### 7.4 远程 voices（可选，P1）

- 尝试 `GET {base}/v1/audio/voices?model={model}`（本机 `mlx-audio` 要求带 `model` query；缺参会 422）
- 失败或空列表时回退预设，并在 status 提示；VoiceDesign 空列表属正常
- **默认关闭**（`fetch_remote_voices = false`），避免官方 OpenAI 404 干扰

---

## 8. UI / `app.rs` 改造规格

### 8.1 状态字段

在 `TtsApp` 增加或从 config 映射：

- 当前 `source`
- OpenAI：`base_url` / `api_key` / `model` / `instruct` 的可编辑缓存（与 config 同步）
- 切源时切换 voice 列表与 `selected_voice_name`

### 8.2 设置区 UI（`show_settings`）

在标题或语音区增加：

1. **TTS 源** Combo：`Edge-TTS` / `OpenAI 兼容`
2. 当源为 OpenAI 时显示分组：
   - Base URL（单行；本地测试可填 `http://127.0.0.1:8081`）
   - API Key（密码遮罩；可空，placeholder 写「本地服务可留空」）
   - Model（单行可编辑；提供快捷选项：`tts-1` / `tts-1-hd` / `mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16`）
   - Voice Combo（预设）+ 可清空/手填自定义
   - **Instruct（音色描述）**：多行 `TextEdit`（至少 2–3 行），hint 如「用自然语言描述目标音色，例如：温柔女声，沉稳男声…」；变更后 `persist_config`
   - 按钮：「刷新语音列表」（OpenAI 源下含义变为加载预设或远程；空列表不报错）
3. Prosody：
   - Edge：语速 / 音调 / 音量 全开
   - OpenAI：语速 + 音量；**音调 Slider `add_enabled(false)`** + hover「当前源请用 Instruct 定制音色，不使用音调滑条」
4. 文案替换硬编码：
   - 窗口/标题可改为更中性，如「TTS 语音合成」或保留 Edge 并副标题说明多源
   - `正在调用 Edge-TTS 生成音频...` → `正在合成音频...` 或按源显示
   - `已加载 Edge 云端语音` → 按源显示
   - 「☁ 刷新云端语音」hover 按源切换说明

### 8.3 合成路径

`start_synthesis`：

```text
if source == OpenAiCompatible && base_url.trim().is_empty()
  -> 本地错误提示，return
# 注意：api_key 为空不阻断

status = 正在合成...
spawn async:
  match source {
    Edge => edge_tts::synthesize(&text, &SpeakOptions{...}).await,
    OpenAiCompatible => openai_tts::synthesize(&text, rate, &openai_cfg).await,
      # openai_cfg 内含 model/voice/instruct/api_key/base_url
  }
```

异步消息枚举 **不必改**（继续 `SynthesisSuccess` / `SynthesisFailed`）。

### 8.4 切源行为

1. 离开 Edge 前：把当前 voice 写入 `edge_voice`
2. 进入 OpenAI：voice 列表切到 OpenAI 预设；`selected_voice_name = openai.voice`（若列表中不存在则仍保留并显示为自定义）
3. 回到 Edge：恢复 `edge_voice` / preset+云端列表
4. 立即 `persist_config()`

### 8.5 启动行为

- `source == Edge`：保持现有 `refresh_voices_async`（拉微软列表）
- `source == OpenAiCompatible`：加载预设；仅当 `fetch_remote_voices` 才请求远程

---

## 9. 测试计划

### 9.1 单元测试（必做，无网络）

| 用例 | 断言 |
|------|------|
| `join_speech_url` 多种 base | 见 §3.3；含 `http://127.0.0.1:8081` → `.../v1/audio/speech` |
| `rate_to_speed` | `0 -> 1.0`，`100 -> 2.0`，`-50 -> 0.5`，超界钳制到 0.25/4.0 |
| 请求 JSON serde（无 instruct） | 含 `model/input/response_format/speed`；无 `instruct` 键 |
| 请求 JSON serde（有 instruct） | 含非空 `"instruct":"..."`；`voice` 空时 JSON 中无 `voice` |
| `AppConfig` 旧 JSON 反序列化 | 无 `source/openai` 字段时 default Edge；缺 `instruct` 时为空串 |
| 空文本 synthesize | 不 panic，返回空 Vec（可用 mock 或前置分支单测） |

### 9.2 集成测试（本机 mlx-audio 优先 / `#[ignore]`）

**首选联调目标**：本机 `http://127.0.0.1:8081` + VoiceDesign 模型（见 §1.3）。

```bash
# 默认指向本机 mlx-audio；也可覆盖
OPENAI_BASE_URL=http://127.0.0.1:8081 \
OPENAI_MODEL=mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16 \
OPENAI_INSTRUCT='温柔女声，语速适中，带一点微笑感' \
  cargo test test_openai_tts_synthesize_mlx_audio -- --ignored --nocapture
```

断言：

1. HTTP 成功且返回非空 bytes
2. 尽可能识别为 mp3
3. 请求确实带上了 `instruct`（可用单测锁定序列化；集成侧以服务接受并返回音频为准）

官方 OpenAI 云端测试仍可另写 `#[ignore]` 用例，但 **不是本任务主验收路径**。

若 8081 未启动：ignored 测试应清晰失败（连接错误），不要在默认 `cargo test` 中跑。

### 9.3 回归

现有 Edge 测试必须继续通过（含 `test_sec_ms_gec_*`、`test_build_ssml`、`test_config_serde`）。

---

## 10. 实施任务拆分（给实现 agent 的顺序）

按严格顺序执行；每步可单独 PR/提交。

### P0 — 配置与 URL/映射纯函数（无 UI）

1. 扩展 `AppConfig` + `TtsSource` + `OpenAiConfig` + serde default
2. 实现 `join_speech_url`、`rate_to_speed`
3. 补单测：config 兼容、URL、speed
4. `cargo test` 相关用例通过

### P1 — OpenAI HTTP 客户端

1. 新增 `src/openai_tts/`
2. 实现 `synthesize`（含可选 `Authorization`、`instruct`/`voice` 条件序列化）
3. 实现 `get_preset_voices`
4. `lib.rs` 导出
5. 增加可 `ignore` 的本机 mlx-audio 集成测试骨架（§9.2）
6. `cargo test`（非 ignored）通过

### P2 — App 分发（先可无完整设置面板）

1. `start_synthesis` 按 `config.source` 分发（传入完整 `OpenAiConfig`，含 instruct）
2. `refresh_voices_async` 按源分发
3. `persist_config` 写入新字段（含 instruct）
4. 用本机 `8081` + VoiceDesign 模型做一次手动/临时代码冒烟

### P3 — UI 完整化

1. 源切换 Combo
2. OpenAI 配置表单（URL/Key/Model/Voice/**Instruct 多行输入**）
3. pitch 条件禁用；Instruct 提示文案
4. 状态文案中性化 / 按源显示
5. 切源 voice 记忆；instruct 随 OpenAI 配置持久化

### P4 — 文档与收尾

1. 更新 `README.md`（架构树、使用说明、参数差异、`instruct`、mlx-audio 本地示例、安全提示）
2. 确认导出仍为 MP3 文件名/过滤器合理
3. 全量 `cargo test`
4. 对本机 `http://127.0.0.1:8081` 做一次真实冒烟（§1.3 curl 或 ignored 测试）

---

## 11. 验收标准

- [ ] 默认行为与现网一致：无新配置时仍走 Edge，旧 `config.json` 可加载
- [ ] 选择 OpenAI 兼容源后，指向 `http://127.0.0.1:8081`，模型 `mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16`，**API Key 留空**也能合成并播放
- [ ] UI 可输入 `instruct`；修改后的音色描述会出现在实际请求 JSON 中，并能影响/定制音色
- [ ] `voice` 可留空完成 VoiceDesign 合成
- [ ] 导出文件仍为可播放 MP3（在服务返回 mp3 的前提下）
- [ ] rate 滑条影响 OpenAI `speed`；pitch 在 OpenAI 源下不可调
- [ ] volume 仍影响本地播放
- [ ] API Key 为空不误拦本地服务；仅当 Key 非空时才加 Authorization
- [ ] 错误 HTTP 状态能在状态栏看到可读信息
- [ ] Edge 源回归：刷新云端语音、合成、快捷键、设备选择均正常
- [ ] 单测覆盖 URL join / speed 映射 / `instruct` 序列化 / 旧配置兼容

---

## 12. 风险与决策（已拍板）

| 议题 | 决定 |
|------|------|
| 抽象方式 | MVP 用 `enum` 分发，不强制 trait |
| voice 存储 | **按源分别记住**（`edge_voice` + `openai.voice`），`voice` 字段保持为「当前源」冗余副本以兼容 |
| pitch | OpenAI 源 UI 灰显，不发送（即使 `mlx-audio` schema 有 `pitch`） |
| **instruct** | **必做**：配置 + UI 多行输入 + 请求条件序列化；用于 VoiceDesign 定制音色 |
| 主验收环境 | 本机 `mlx-audio`：`http://127.0.0.1:8081` + `mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16` |
| API Key | **可选**；空则不发 Authorization |
| 音频格式 | 只保证 `mp3` |
| 远程 voices | 默认关；预设 + 手填自定义；VoiceDesign 空列表可接受 |
| 其他 mlx 扩展字段 | 本阶段不做（`ref_audio` 等） |
| 依赖 | 不新增 crate（除非实现中确有必要） |
| Key 存储 | 明文配置文件 + README 风险说明 |

### 实现中若遇歧义时的默认选择

1. 兼容服务路径怪异：先靠 `join_speech_url` 规则；不要做「完整 speech URL」高级选项，除非用户后续明确要求
2. `Voice` 类型：优先复用 `edge_tts::types::Voice`，避免 UI 大面积改类型
3. 窗口标题：可改为「TTS 语音合成」，README 说明支持 Edge + OpenAI 兼容源（含本地 mlx-audio）
4. `Default` 的 `OpenAiConfig` 仍可指向官方形态；本地 8081 参数以 README/示例/手动填写为准，避免默认连不上服务
5. 若 speech 报模型未加载：可尝试 `POST /v1/models?model_name=...`，但客户端 MVP 不强制封装「装载模型」按钮；优先在请求里直传 model 全名

---

## 13. 参考实现片段（非最终代码，仅约束行为）

### 13.1 speed 映射

```rust
pub fn rate_to_speed(rate: i32) -> f32 {
    ((1.0 + rate as f32 / 100.0).clamp(0.25, 4.0) * 100.0).round() / 100.0
}
```

### 13.2 请求结构

```rust
#[derive(Serialize)]
struct OpenAiSpeechRequest<'a> {
    model: &'a str,
    input: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    voice: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instruct: Option<&'a str>,
    response_format: &'a str, // "mp3"
    speed: f32,
}
```

### 13.3 本机联调备忘

```text
Base URL : http://127.0.0.1:8081
Endpoint : /v1/audio/speech
Model    : mlx-community/Qwen3-TTS-12Hz-1.7B-VoiceDesign-bf16
Instruct : （必填体验项）自然语言音色描述
Auth     : 无
```

---

## 14. 交接检查清单（实现 agent 开工前）

1. 阅读本文件全文（尤其 **§1.3 / §3.1 / instruct 相关决策**）+ `README.md` + `src/app.rs` 的 `start_synthesis` / `persist_config` / `show_settings`
2. 阅读 `src/config.rs`、`src/edge_tts/{types,client,voices}.rs`
3. 确认本机 `curl http://127.0.0.1:8081/` 可访问；按 §1.3 做一次 speech 冒烟（可选但强烈建议）
4. 按 §10 从 P0 开始，不要跳过 config 兼容与 `instruct` 序列化单测
5. 每完成一个 P 级，运行 `cargo test`
6. 不要删除或削弱现有 Edge Sec-MS-GEC / SSML 测试
7. 不要在本任务中做无关重构（字体、hotkey、audio 播放线程等）

---

## 15. 完成定义（DoD）

当且仅当：

1. §11 验收项全部满足  
2. `cargo test` 通过  
3. README 已更新  
4. 本计划中 MVP 范围（P0–P4）已落地  

才可标记任务完成。若只完成到「能编译的半成品」，在交接说明中明确卡在哪个 P 级与剩余项。
