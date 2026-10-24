# Typeless 2.8.2 逆向分析报告

> 目标：`/Applications/Typeless.app`（macOS，arm64）
> 目的：还原其「按住说话 → 云端转写/润色 → 写回前台输入框」的完整链路，并据此设计 Rust 复刻版 `rustype`。
> 结论分级：**观察** = 直接来自产物或符号；**推断** = 由观察组合得出；**未知** = 现有证据无法确认。
> 分析过程中的尝试、失败与取舍记录在 [THINKING_PROCESS.md](THINKING_PROCESS.md)。

---

## 1. 目标身份

| 项目 | 值 | 来源 |
| --- | --- | --- |
| 应用 | Typeless.app，v2.8.2（build 2.8.2.131） | `Info.plist` |
| Bundle ID | `now.typeless.desktop` | `Info.plist` |
| 运行时 | Electron（`NSPrincipalClass = AtomApplication`） | `Info.plist` |
| 架构 | arm64（thin Mach-O） | `file` / `lipo -info` |
| 签名 | Developer ID Application: Simply LLC (947QKAND4W) | `codesign -dv` |
| 主 asar | 292,065,720 字节，SHA-256 `49ee4061c662c4284e1d79327e2b0c4389d2df1fc1de9004e252e6e6f8528ad9` | `shasum -a 256` |
| 云 API | `https://api.typeless.com` | 主进程包 `Bi` 常量 |
| 本地库 | better-sqlite3 + drizzle（`Resources/drizzle` 迁移） | asar 文件树 / 导入语句 |

**观察**：asar 内没有 whisper/ggml/onnx/mlmodel 等本地模型文件；主进程只包含 HTTP/WS 客户端、Opus 编码器和 SQLite。→ **推断**：ASR 与 LLM 润色全部在服务端完成，客户端只负责采集、编码、上传与写回。

---

## 2. REA 使用记录与工具链限制

REA MCP 工具在本会话不可用，按技能要求走 CLI：

```bash
npx -y rea-agents@latest doctor --json
npx -y rea-agents@latest analyze-javascript-application /Applications/Typeless.app \
    --integrity-policy record-and-continue --json
```

| 步骤 | 结果 |
| --- | --- |
| `doctor` | `execution_failure`：Ghidra provider 的 macOS 进程归属辅助程序需用 `xcrun swiftc` 编译；本机 CommandLineTools 的 Swift SDK 与编译器版本不匹配（`redefinition of module 'SwiftBridging'`、`SDK is not supported by the compiler`），编译失败。 |
| 第一次 JS 分析 | `artifact_integrity_mismatch`：声明中 `app.asar.unpacked/.../better_sqlite3.node` 的 SHA-256 与伴随文件不一致（打包/签名后常见）。 |
| 第二次 JS 分析（record-and-continue） | `execution_failure`，阶段 `startup`，仍是同一个 Swift 辅助程序编译问题；返回部分 Evidence：`ev_6b2436ff5f9cca67982a29aa43fbed715b53571e9592699f827ebf5a877a99ff`，目标目录 SHA-256 `acacdbd0c93041e71220c99822d527d021d6dcb67accd3efcae92645ecd8b596`。 |

**因此**：REA 的 JS/原生分析 provider 在本机被工具链问题阻断，本次结论来自**直接产物取证**（asar 解析、反混淆、Swift 符号表、字符串、plist）。这不是 REA 结论的替代品，但证据链等价可复核；下文每条结论都标注了来源。

本机没有 Xcode（`xcode-select -p` → `/Library/Developer/CommandLineTools`），因此也无法用常规 `swiftc` 反编译 Swift dylib。`nm` 保留了完整的 Swift mangled 符号，足以还原导出 API。

---

## 3. 取证与反混淆方法

```bash
# 1) 解析 asar 头（8 字节 pickle 头 + JSON 索引）
node re/asar_list.js
# 2) 提取 dist/main、dist/preload、dist/renderer、worker
node re/asar_extract.js dist/main dist/preload dist/renderer
# 3) 反混淆（javascript-obfuscator：字符串数组 + 轮转 + 解码函数）
node re/deob.mjs re/extract/index.js re/main.deob.js
```

主进程 `dist/main/index.js`（628 KB）是 Vite/Rollup 打包后再经 javascript-obfuscator 处理：字符串被抽到 `_0x5d8c()` 数组并由 IIFE 轮转，调用点形如 `_0x5763(0x1a2)`。webcrack 2.16 默认用 `isolated-vm` 做沙箱，而该原生模块没有 Node 26 的预编译产物；改为注入基于 `node:vm` 的 `sandbox` 选项后，4 秒完成解码，得到 20,838 行可读代码（`re/main.deob.js`）。

渲染进程 `dist/renderer/static/js/DWYvEY4x.js` 未做字符串混淆（仅压缩/改属性），可直接分析；音频 worklet `C7lI2ZxR.js` 是未压缩源码。

---

## 4. 进程与窗口结构

**观察**（`cs` 常量，`re/main.deob.js:80`）：

| 页面 | 用途 |
| --- | --- |
| `hub.html` | 主界面：历史、词典、设置、统计、个人化 |
| `floating-bar.html` | 录音悬浮条（波形、模式、提示） |
| `onboarding.html` | 引导、权限、快捷键与麦克风测试 |
| `login.html` | OAuth 登录 |
| `sidebar.html` / `interactive-card.html` | 侧栏与「Ask anything」结果卡片 |

主进程注册 **112 个 `ipcMain.handle` 通道**（完整清单可由 `re/main.deob.js` 提取），可归为：`audio:*`（录音/Opus/云端请求）、`keyboard:*`（全局按键、文本插入）、`context:*`（前台应用/光标上下文）、`permission:*`、`db:history-*`、`sync-engine:transcription-history:*`、`auth:*`、`page:*`、`updater:*`。

**观察**：本地历史使用 `history` 表（`refined_text`、`audio` BLOB、`audio_local_path`、`mode`、`mode_meta`、`duration` 等），并有到云端的 `push/pull/load_older/wipe` 同步通道；`mode_meta` 里保存 `ai_result.delivery` 等结果字段。

---

## 5. 全局快捷键与按键监听

**观察**：主进程**没有**使用 Electron `globalShortcut`；它通过 koffi 加载 Swift 动态库 `libKeyboardHelper.dylib`（`re/main.deob.js:11797` 起）：

```text
updateTargetShortcuts(char* json)      设置需要关注的组合键
resetPressingKeycodes()                清空当前按键状态
startMonitor(bool (*cb)(int32,int32,char*,int32,int32,char*)) -> bool
processEvents()                        主线程泵事件
stopMonitor()
setWatcherInterval(double)
getKeyboardDeviceList() -> char*       JSON（外部键盘列表）
```

回调参数为 `(keyCode, keyName?, ... , pressedKeysJSON)`，另有哨兵值：`-8888` 表示监听器需重启、`-9999` 表示状态重置。JS 侧每 16 ms 节流广播一次按键快照，并以 50 ms 周期兜底同步（`re/main.deob.js:12660-12840`）。

**观察**：默认快捷键（`re/main.deob.js:4223`）：

| 功能 | macOS | Windows / Linux |
| --- | --- | --- |
| 听写（按住） | `Fn` | `RightAlt` |
| Ask anything（改写/提问选中文本） | `Fn+Space` | `RightAlt+Space` |
| 翻译模式 | `Fn+LeftShift` | `RightAlt+RightShift` |
| 粘贴上一条转写 | `Ctrl+Cmd+V` | `LeftCtrl+RightShift+V` |

**推断**：使用 IOHIDManager 级别的键盘监听而非 Carbon 热键，是为了拿到 `Fn/Globe` 键与「外部键盘布局」；这也是 UI 中「外接键盘按键不工作？点此更改」提示的原因。

---

## 6. 音频链路

**观察**：

1. 渲染进程 `getUserMedia` 采集，`AudioContext` 目标采样率 **16000 Hz**（`Ut = 0x3e80`）。
2. `AudioWorkletProcessor`（`C7lI2ZxR.js`，源码可读）把输入下混为单声道，按 **1024 个 Float32 样本**一条 `{type:'recording:data', samples}` 消息发出；`recording:start/stop` 控制开关，停止时 flush 余量并回 `recording:stopped`。
3. 渲染侧把 Float32 转 Int16 并累积；`chunkTime` 默认 1000 ms，按块回调（供波形/流式钩子使用）。
4. 主进程用 worker（`worker/chunkSessionWorker.js`）调用 `libopuschunk.dylib` 做连续编码：Opus **32 kbps**、单声道、complexity **9**，输出 **Ogg**。另有 `opusWorker.js` + `libopusenc.dylib` 做整段转码兜底。
5. 录音期间可 `muteAudio/unmuteAudio`（`libUtilHelper`），开始/结束播放提示音。

**推断**：最终请求发送的是整段 Ogg，而不是流式识别；主进程里唯一的 WebSocket 地址构造函数（`getWsApiHost`）没有任何调用点。

---

## 7. 云端 AI 请求（核心接口）

**观察**（`re/main.deob.js:17818-17950`）：一次 `POST https://api.typeless.com/ai/voice_flow`，`multipart/form-data`，超时 40 s，可被 `audio:abort-ai-voice-flow-request` 取消。

| 字段 | 含义 |
| --- | --- |
| `audio_id` | 本次录音 UUID |
| `mode` | `voice_transcript` / `voice_command` / `voice_translation` |
| `audio_file` | `audio/ogg` 文件 |
| `audio_context` | 前台应用、窗口、光标/选中文本、设备等 JSON（上传前经 `ia()` 做隐私裁剪） |
| `audio_metadata` | 时长、采样信息、`mic_device_info` 等 |
| `parameters` | `voice_command` → `selected_text`；`voice_translation` → `output_language` |
| `is_retry` / `send_time` / `user_over_time` / `device_name` | 重试、时延统计、设备信息 |

响应（成功路径）：`{ data: { refine_text, delivery, user_prompt, web_metadata, external_action }, debug }`。
`refine_text` 直接用于插入；`delivery = "external"` 表示结果交给外部动作（如打开链接）而不是写回输入框；付费/限额通过 `important_notification` 返回。

**推断**：服务端在一次调用内完成 ASR + 意图判断 + 润色/翻译/改写，客户端不持有提示词模板；「模式」只影响服务端行为与 `parameters`。

---

## 8. 上下文采集（决定润色质量的关键）

**观察**：主进程每 1000 ms 轮询一次前台信息（`hf` 类），并调用 `libContextHelper.dylib`：

```text
getFocusedAppInfoAsync(timeout_ms, callback)
getFocusedElementInfoAsync(timeout_ms, callback)
getFocusedVisibleTextAsync(before, after, callback)
getFocusedElementRelatedContentAsync(before, after, max, callback)
setFocusedWindowEnhancedUserInterface()
```

**观察**：送云的 `audio_context` 结构包含：

- `active_application`: `app_name`、`app_identifier`、`window_title`、窗口位置、`app_type`（native/browser）、`app_metadata`、可选 `browser_context`/`visible_screen_content`；
- `text_insertion_point`: 输入区类型、屏幕位置、可编辑/富文本能力、`cursor_state`（光标位置、是否有选区、`selected_text`、`text_before_cursor`、`text_after_cursor`、`full_field_content`）、输入区前后文本；
- `device_environment`、`context_metadata`（采集版本、耗时、是否被黑名单域名屏蔽、质量评级）。

`is_text_capture_blocked_by_blacklist` 说明存在按域名/应用禁用上下文抓取的策略（对应 `/app/get_blacklist_domain`）。

**推断**：Typeless 的「跨应用语气适配 / 自动补全专有名词 / 选中文本改写」主要靠这份上下文 + 个人词典 + 历史个人化，而不是本地模型。

---

## 9. 文本写回（插入）

**观察**：`libInputHelper.dylib` 导出：

```text
insertText(const char*) -> int32           成功返回 1
insertRichText(const char* html, const char* text) -> int32
deleteBackward(int32)                       删除已写入内容（回退/重写）
getSelectedText(timeout_ms) -> JSON
getCurrentInputState(timeout_ms) -> JSON
getSelectedTextBySimulateCopyAsync(timeout_ms, callback)
```

Swift 符号进一步暴露实现：`savePasteboard` / `restorePasteboard` / `performTextInsertion(with:)` / `simulatePasteCommand(completion:)`。→ **推断**：主路径是「保存剪贴板 → 写入文本 → 合成 Cmd+V → 恢复剪贴板」，AX 直接插入作为兜底；JS 侧插入后加 500 ms 互斥锁，避免连发。

**观察**：`keyboard:type-transcript` IPC 直接调用 `insertText`；`voice_command` 会先读取选中文本，必要时用 `deleteBackward` 覆盖原选区。

---

## 10. 权限、设备与系统集成

**观察**（`libUtilHelper.dylib` 导出）：`checkAccessibilityPermission`、`getDeviceId`、`getAudioDevicesJSON(Async)`、`muteAudio`/`unmuteAudio`/`isAudioMuted`、`launchApplicationByName`、`deviceIsLidOpen(Async)`。

- 缺失辅助功能权限时，主进程不会启动键盘监听（`systemPreferences.isTrustedAccessibilityClient(false)` 判断）。
- `Info.plist` 声明麦克风、相机、蓝牙用途；应用通过 `LSUIElement` 风格的悬浮条 + Tray 工作。
- 账号体系为邮箱/Google/Apple/SSO OAuth，Bearer access token + refresh token，本地存储用 PBKDF2 派生密钥加密；`rsa:encrypt` 通道表明部分数据使用 RSA 公钥加密后再上传。

---

## 11. 结论汇总

### 观察（可直接复核）

1. Electron + 6 个窗口 + 112 个 IPC 通道 + 本地 SQLite + 云端历史同步。
2. 4 个自研 Swift dylib（keyboard / input / context / util）通过 koffi 调用；另有 libopusenc / libopuschunk。
3. 全局按键由 HID 级监听器提供，默认 `Fn` 系快捷键，16 ms 节流广播。
4. 音频 16 kHz 单声道、1024 样本/消息、Opus 32 kbps Ogg。
5. 识别+润色合并为一次 `/ai/voice_flow` multipart 调用，40 s 超时，返回 `refine_text`。
6. 上下文包含前台应用、窗口标题、选中文本、光标前后文、整段输入框内容。
7. 写回以「剪贴板 + Cmd+V」为主，带剪贴板恢复。

### 推断

- ASR/LLM 均在服务端；客户端没有模型文件，也没有流式识别通道。
- 「个人化 / 词典 / 语气」通过上下文 + 词典 + 历史在服务端生效。
- 使用 HID 监听是为了 Fn/Globe 键与外接键盘布局。

### 未知

- 服务端提示词、模型与 `ia()` 的具体裁剪规则（仅能看到调用点）。
- Swift dylib 内部实现细节（Ghidra/IDA provider 不可用，仅有符号级证据）。
- `audio_context` 的 `visible_screen_content` 是否默认开启（受黑名单与屏幕录制权限影响）。
- 各套餐的时长/字数限制阈值（仅见 UI 文案与 `important_notification` 机制）。

---

## 12. 对 Rust 复刻的设计影响

| 能力 | Typeless 观察 | rustype 设计 |
| --- | --- | --- |
| 触发 | HID 监听 Fn；按住/双击免提；组合键切模式 | `CGEventTap` 监听修饰键（默认右 Option，可配 Fn）；同样支持按住、双击免提、Esc 取消、组合键切模式 |
| 模式 | 听写 / 命令（选中文本）/ 翻译 | 相同三模式；命令模式读取 AX 选中文本 |
| 音频 | 16 kHz 单声道，Opus 32 kbps | 16 kHz 单声道，WAV 16-bit（兼容性优先，Opus 可后续加） |
| 识别 | 私有云一次调用 | OpenAI 兼容 `/audio/transcriptions`（可指向本地网关/Whisper 服务） |
| 润色 | 服务端提示词 | 可配置提示词模板 + 个人词典 + 应用上下文 |
| 写回 | 剪贴板 + Cmd+V，AX 兜底 | 剪贴板 + Cmd+V（默认），或 `insert.method="type"` 直接合成 Unicode 事件 |
| 上下文 | 应用/窗口/选区/光标前后文 | 前台应用 + 窗口标题 + AX 选中文本（不采集整段输入框，隐私优先） |
| 存储 | SQLite + 云同步 | 本地 JSONL，无云同步 |
| 分发 | Electron 安装包 + Squirrel 更新 | 单二进制 + 菜单栏常驻按钮 + launchd（可选） |

**未复用 Typeless 云端接口**：该接口需要 Typeless 账号 token，且属于第三方服务；复刻版使用可配置的 OpenAI 兼容端点，既能在本机接本地模型，也能接任意云服务。这是刻意的设计选择，不是能力缺失。

---

## 13. 复现命令

```bash
node re/asar_list.js                       # asar 清单
node re/asar_extract.js dist/main dist/preload dist/renderer
node re/deob.mjs re/extract/index.js re/main.deob.js
nm -gU /Applications/Typeless.app/Contents/Resources/lib/*/build/*.dylib
python3 re/search.py 'ai/voice_flow' re/main.deob.js 200 5
```

产物：`re/main.deob.js`（主进程反混淆结果）、`re/extract/`（解包产物，含 renderer 与 worklet 源码）。
