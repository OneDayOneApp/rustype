# 思考与决策日志

> 这份文档记录的不是结论，而是**得出结论文的过程**：先查什么、为什么换路、哪些尝试失败了、每个设计取舍的理由。
> 结论与证据见 [REVERSE_ENGINEERING.md](REVERSE_ENGINEERING.md)，使用说明见 [../README.md](../README.md)。

---

## 0. 起点：先确认“要逆向的东西到底是什么”

用户说“用 REA 逆向 typeless，并复刻一个 Rust 版本”。在动手之前，我先要回答三个问题：

1. `Typeless` 在机器上是什么形态？（原生 / Electron / Tauri）
2. 它做什么？（语音输入？翻译？）
3. 复刻的最小可用闭环是什么？

查 `/Applications/Typeless.app`：

- `Info.plist` 里 `NSPrincipalClass = AtomApplication`、`ElectronAsarIntegrity` → 确定是 Electron；
- `Contents/Resources/app.asar` 有 292 MB → 主逻辑在里面；
- `Resources/lib/` 下有 6 个自研 dylib（keyboard/input/context/util + opus 编码）→ 说明大量能力在原生层；
- asar 里没有 whisper/ggml/onnx 模型 → **推断 ASR 在云端**，这直接决定了复刻版的架构（客户端只做采集/上传/写回）。

**这一步的思考**：如果它是原生 Swift App，路线会是 Hopper/Ghidra 反编译；如果是 Electron，优先解 asar + 反混淆；如果是 Tauri，则看 Rust 二进制符号。最终确认是 Electron + 原生辅助库的混合形态，于是把工作拆成“JS 主进程”和“Swift dylib 符号”两条线。

---

## 1. REA：先按规矩走，再接受现实

技能要求先用 REA。我先检查会话里有没有 REA 工具——没有，于是走 CLI：

```bash
npx -y rea-agents@latest doctor --json
npx -y rea-agents@latest analyze-javascript-application /Applications/Typeless.app --json
```

第一次失败：`run-npm` 写 `~/.npm` 被沙箱拒绝。把 `npm_config_cache` 指到工作区后解决。

`doctor` 失败：Ghidra provider 在启动前要编译一个 Swift 的“进程归属”辅助程序，而本机 CommandLineTools 的 Swift SDK 与编译器版本不匹配：

```
redefinition of module 'SwiftBridging'
SDK is not supported by the compiler
```

第一次 JS 分析失败：asar 声明的 `better_sqlite3.node` 完整性哈希与解包文件不符（签名/打包后常见）。按提示加 `--integrity-policy record-and-continue` 重跑，仍然在 `startup` 阶段因为同一个 Swift 编译问题失败。

**当时的判断**：这是环境问题，不是目标问题。继续修 Swift 工具链的性价比很低（没有 Xcode，SDK 版本错配），而 JS 分析完全可以用手工产物取证替代。REA 的部分 Evidence（`ev_6b24…`、目录 SHA-256 `acacdbd0…`）保留在报告里，作为“尝试过、被阻断”的记录。

> 这里刻意没有假装 REA 成功：报告里明确写了工具链阻断和替代取证方式，结论按“观察/推断/未知”分级。

---

## 2. 解 asar 与反混淆：为什么不能直接用 webcrack

asar 本质是“8 字节 pickle 头 + JSON 索引 + 连续文件数据”。我写了一个 20 行的 Node 脚本直接解析头、按 offset 取文件，避免依赖 `asar` 包。先只取 `package.json`、`dist/main`、`dist/preload`、`dist/renderer`——不整包解压 292 MB。

拿到主进程 `dist/main/index.js`（628 KB）后发现是 javascript-obfuscator 处理过的：

```js
const _0x54c8a6=_0x4963;(function(_0x1fc24d,_0xdd4c44){...}(_0x5d8c,0x43999));
```

**判断**：字符串被抽到数组并轮转，直接读代码没意义，必须先解码。

第一反应是 webcrack（它专门处理这类混淆）。但：

- webcrack 2.16 的 `engines` 要求 Node 22/24，本机只有 Node 26；
- 它默认用 `isolated-vm` 做沙箱，而 `isolated-vm` 没有 Node 26 的预编译产物，`npx` 直接失败。

**转折点**：读 webcrack 源码发现 `sandbox` 是可选参数——它只需要一个“执行表达式并返回结果”的函数。于是写了一个 6 行的 `node:vm` 沙箱：

```js
const sandbox = async (expr) => vm.runInNewContext(expr, Object.create(null), { timeout: 10000 });
const result = await webcrack(code, { sandbox, unpack: false, mangle: false });
```

4 秒完成，得到 20,838 行可读代码。**这一步的经验**：遇到工具链不兼容时，先看它的可注入点，而不是换工具重写。

---

## 3. 从代码里“顺藤摸瓜”的顺序

面对 2 万行反混淆代码，我没有从头读，而是按“用户动作 → 数据流”找入口：

1. 搜 `voice_flow` → 命中 `/ai/voice_flow`，拿到整个云端请求的字段和响应；
2. 搜 `audio:` → 命中 12 个 IPC 通道，找到 opus 编码与上传；
3. 搜 `keyboard` / `Fn` → 找到默认快捷键表和原生监听器绑定；
4. 搜 `insertText` → 找到写回策略；
5. 最后才看窗口、数据库、同步这些外围。

**为什么是这个顺序**：复刻的核心闭环是“按键 → 录音 → 转写 → 写回”，其余（历史、同步、账号）都是产品外壳，可以后置。先锁定闭环，才能判断客户端到底承担了多少工作。

几个关键发现及推理：

- `getUserMedia` 不在主进程 → 录音在 renderer；顺藤找到 `AudioWorklet` 源码（未混淆），确认 16 kHz、1024 样本/消息、单声道下混；
- 主进程只有 `libopusenc`/`libopuschunk` 的 FFI，没有 ASR 库 → 再次确认服务端识别；
- 只有一个 WebSocket URL 构造函数、没有任何调用点 → **排除流式识别**；
- `libKeyboardHelper` 用 HID 级监听而不是 Electron `globalShortcut` → 解释得通：`Fn/Globe` 键 Carbon 热键注册不了，且要支持外接键盘布局。

---

## 4. 设计复刻版时的取舍

拿到行为后，开始做选择。每个选择都问三个问题：**必要性、风险、可验证性**。

### 4.1 触发键：继续用 Fn，还是换？

- 事实：Typeless 自己也监听 Fn，两个应用会抢同一个键；实测用户按 Fn 时 Typeless 的界面先响应。
- 选项 A：要求用户退出 Typeless —— 把冲突转嫁给用户，不好；
- 选项 B：默认换成右 Option（按住时不会输入任何字符，也不与 Typeless 冲突），Fn 仍可在配置里启用。
- 决定：B。并在状态机里支持 `right-alt` 这种“左右修饰键”区分（CGEvent 的 flags 不分左右，改从 flagsChanged 的 keycode 识别）。

### 4.2 云端接口：复用 Typeless 的，还是走标准协议？

- 复用 `/ai/voice_flow` 需要 Typeless 账号 token，属于第三方私有服务，且随时可能变；
- 标准 OpenAI 兼容接口可以接 OpenAI、Groq、本地 Whisper，用户可替换。
- 决定：**两次调用**（ASR + chat completions），提示词放在客户端。代价是多了网络往返，收益是可移植、可审计。

### 4.3 音频格式：照抄 Opus，还是先用 WAV？

- Typeless 用 16 kHz 单声道 Opus 32 kbps（chunk worker 里 `0x7d00`、复杂度 9）；
- 复刻版优先保证“能通”：WAV 16-bit 所有端点都收；Opus 需要额外引入编码库。
- 决定：先 WAV，把 Opus 参数写进报告作为后续优化项。

### 4.4 上下文：照抄“整段输入框内容”，还是最小化？

- Typeless 会上传光标前后文、整段输入框内容、可见屏幕内容；
- 复刻版只取前台应用名、窗口标题、AX 选中文本。
- 理由：隐私优先，且这三项已足够支撑“命令模式改写选中文本”的卖点。

### 4.5 写回：AX 插入，还是剪贴板 + Cmd+V？

- AX 插入对富文本/网页编辑器兼容性差；
- 剪贴板 + Cmd+V 是 Typeless 的主路径（符号里的 `savePasteboard`/`simulatePasteCommand` 可证）。
- 决定：默认 paste，保留 `type` 作为兜底。

---

## 5. 实现中“想错了再改”的记录

### 5.1 录音启动延迟

第一次真实测试：按住 2.5 秒只录到 2.09 秒。怀疑是键盘事件延迟，但改成 1 秒按住只录到 0.2 秒，说明是**设备启动开销**（CoreAudio 打开输入流约 0.4–1.2 秒），不是事件问题。

- 方案：常驻音频流（`keep_mic_warm`），空闲时丢弃样本，按下时清空缓冲；
- 结果：第二次起 2.5 秒按住能录到 2.48 秒；
- 代价：麦克风指示灯常亮，写进 README 并允许关闭。

### 5.2 状态机被“误触修饰键”打断

原逻辑是“没有任何快捷键匹配就停止录音”。测试时按住 Fn 再碰一下 Cmd，录音立刻结束。

- 重新定义：停止的条件是**触发键本身被松开**（`held_loose`），不是“组合键不匹配”；
- 顺带支持左右修饰键区分，避免右 Option 松开后左 Option 还按着导致状态卡住。

### 5.3 用悬浮条 ✓ 停止后，快捷键“失灵”

现象：点 ✓ 停止后，下一次按右 Option 没反应。

- 原因：Engine 以为还在录音（只有它自己的 Stop 才会清状态），而 Session 已经被 UI 停掉；下一次按键被引擎当成“结束”而不是“开始”。
- 修复：`abort_recording()` 清空引擎状态并加锁存，直到所有触发键释放；同时清掉双击计时，避免误进免提。
- 这是典型的“两个状态源”问题：UI 和键盘引擎都能停止录音，必须让它们共享同一个事实。

### 5.4 菜单栏左键高亮不恢复

tray-icon 0.21 在 macOS 上对左键调用 `highlight(true)` 后从不取消，图标永久橙色。

- 先尝试保留“按住图标说话”，实测高亮卡死；
- 决定：改成标准“点击弹菜单”，按住说话交给快捷键。功能上等价，视觉上不再误导。

### 5.5 代理与额度

- 直连 `api.openai.com` 超时，但 `curl -x 127.0.0.1:7890` 返回 200 → 系统代理存在，reqwest 默认没走；
- 加 `provider.proxy`，客户端显式 `Proxy::all`；
- 然后 OpenAI 返回 `429 insufficient_quota`（账户无余额）；
- 切 Groq：直连 403、代理 200，确认 `whisper-large-v3-turbo` 可用后切换，真实语音转写成功。

---

## 6. 测试策略：怎么证明“真的能用”

分三层，成本从低到高：

1. **纯逻辑单测**（35 项）：配置、快捷键解析、状态机（含双击免提、锁存）、WAV 编解码、提示词、HTTP 响应解析；
2. **进程内 HTTP 集成测试**：用 `TcpListener` 起一个微型服务器，验证 multipart 字段、鉴权头、响应解析——不依赖外网；
3. **真实端到端**：合成 Fn/右 Option 事件 + 合成鼠标点击 + `say` 生成语音，观察日志与 `history.jsonl`，确认“录音 → 转写 → 写回”。

中间还发现一个测试竞态：两个测试同时改 `RUSTYPE_API_KEY` 环境变量。加了一把测试专用互斥锁，避免偶发失败。

---

## 7. 现在的边界与没做的事

- 悬浮条只显示录音态，转写/润色阶段直接隐藏（菜单栏标题显示 `…`）；
- 没有做 Typeless 的“外部动作”（打开链接、创建文档）与云端历史；
- 服务端提示词仍是黑盒，复刻版的提示词是按行为推断重写的；
- REA 的原生 provider 未跑通，dylib 只做到符号级证据，没有伪代码级确认。

## 8. 如果继续做，优先级会怎么排

1. 悬浮条增加“识别中/润色中”状态与失败提示（当前只有声音和日志）；
2. Opus 编码（带宽与 Typeless 对齐）；
3. 本地 Whisper 后端（离线、无额度限制）；
4. 菜单栏图标加入录音/处理动画；
5. 用带时间戳的分段请求做“边说边出字”（需要服务端支持流式）。
