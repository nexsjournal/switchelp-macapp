# 06. 本地决策模型的运行时与打包工程事实

**日期**：2026-09-30
**范围**：把一个小型决策模型（Laya / open-jev / kev 类，非自回归编码器或 0.6B–2B 小生成式）
跑在 Switchelp（Tauri 2 + Rust + React）里，macOS（arm64 首发、x86_64 待定）+ Windows（x86_64）。
回答"能不能塞、怎么塞、要多大、有什么坑"。
**不在范围**：模型质量评测、训练/微调流程、Jev API 的协议细节（另文）、本仓库代码改动。

**方法**：crates.io / GitHub Releases / HuggingFace blobs API / PyPI 中央目录 / Apple 官方 JSON API / 各家官方定价页直读。
体积与延迟数字均取自上列来源，**宣称值与实测值分列**；查不到的写"未找到"。
档位标记：【官方文档】【仓库/代码】【第三方文章】【推测】。

---

## ① 一句话结论

**技术上塞得进去，但没有一条路是"顺手"的**：本项目现包 7.3 MiB，而最小的可用决策模型量化后也要 **260 MB**
（Laya Q4_K_M），随包分发等于体积 ×36；运行时最有价值的组合是 **`ort` + ONNX int4（in-process，零 sidecar）**
或 **llama.cpp sidecar + Laya GGUF**；macOS 侧签名/公证有 Tauri bundler 兜住（sidecar 会被自动签名并开硬化运行时），
但 **明明是免费的路在 Windows 上要先补网关/更新链路**，而在线 Jev 每天 1 万次只要 **$0.42–0.81/天**——
所以本地推理在 Switchelp 里应当定位成**隐私选项**，而不是省钱选项。

---

## ② 运行时选型对比表

| | `ort` | `llama-cpp-2` | `candle-core` | `tract-onnx` | `burn` |
|---|---|---|---|---|---|
| 版本（2026-09-30） | 2.0.0-rc.13 | 0.1.157 | 0.11.0 | 0.23.8 | 0.22.0-pre.4 |
| 模型格式 | ONNX | GGUF | safetensors / GGUF（llama.cpp 量化类型）/ ONNX（candle-onnx） | ONNX / NNEF / TFLite | Burn 自有格式 + `burn-onnx` 转原生 Rust 代码 |
| 是否纯 Rust | ❌ 包装 C++ ONNX Runtime（另有 `ort-candle` / `ort-tract` 纯 Rust 后端） | ❌ `llama-cpp-sys-2` 用 cmake 编 llama.cpp | ✅ | ✅ | ✅ |
| macOS arm64 | ✅ 预编译（含 CoreML EP） | ✅ 需本地 cmake + Metal | ✅ Metal | ✅ | ✅ CubeCL CPU / Metal |
| macOS x86_64 | ❌ **无预编译**（dist.tsv 无此 target） | ✅（官方 quart 出 macos-x64 包） | ✅ | ✅ | ✅ |
| Windows x86_64 | ✅ 预编译（DirectML / WebGPU / CUDA 变体） | ✅（官方出 win-cpu-x64） | ✅ | ✅ | ✅ |
| 构建复杂度 | 低：默认 `download-binaries` 在 build 期拉预编译包，无需 C++ 工具链；`load-dynamic` 可改运行时加载 | **高**：build.rs 走 `cmake::Config`，需要 cmake + C/C++ 工具链（含 CI 的 Windows runner） | 低：纯 Rust；但 `candle-onnx` 额外需要 `protoc` | 低：纯 Rust | 中：`burn-onnx` 是导入期代码生成，不是运行时加载 |
| 本机二进制体积 | macOS arm64 预编译归档 **8.9 MB**（lzma2，静态链）；动态库实测 `libonnxruntime.1.30.0.dylib` **31.7 MB**；Windows `onnxruntime.dll` **17.6 MB** | macOS arm64 全量发行包 **11.8 MB**（tar.gz，含 server/cli/lib）；Windows CPU x64 **19.2 MB**（zip） | 视后端而定，未找到逐平台数字 | 未找到 | 未找到 |
| 许可 | MIT OR Apache-2.0（包装）；ONNX Runtime 本体 **MIT** | MIT OR Apache-2.0；llama.cpp **MIT** | MIT OR Apache-2.0 | MIT OR Apache-2.0 | MIT OR Apache-2.0 |
| 对本项目的关键点 | 唯一"**进进程、零子进程、单文件分发**"的可行路线；F16/INT8/INT4 ONNX 现成 | 唯一能跑 Laya GGUF 与 Jev-Style/kev GGUF 的路线；但 Jev-Style 需要**自建 scorer**（见 ③） | GGUF 生态与 llama.cpp 不同步，算子覆盖需自验 | 只有 ONNX 子集；本项目没有实测依据 | 需要模型先转 Burn 格式，链路最长 |

**依据**：crates.io API（各 crate `max_version` / `license`）【仓库/代码】；
`pykeio/ort` `ort-sys/build/download/dist.tsv`（target × feature_set 矩阵）【仓库/代码】；
pykeio ort 文档《Prebuilt binaries》（x86-64-v3 基线、Windows 全带 DirectML、macOS 全带 CoreML、二进制经 attestation）【官方文档】；
`huggingface/candle` README（GGUF 量化、candle-onnx）与 `candle-onnx/README.md`（需 protoc）【仓库/代码】；
`sonos/tract` README（ONNX/NNEF/TFLite、ONNX-only 算子走 `tract-onnx-opl`）【仓库/代码】；
`tracel-ai/burn` README（`burn-onnx` 导入为原生 Rust）【仓库/代码】。
体积：CDN `Content-Length`、`gh api .../releases`、PyPI wheel 中央目录（读 zip EOCD + CD，未下载整个文件）【仓库/代码】。

> **x86_64 的一条硬限制**：pyke 的**所有 x86-64 预编译 ORT 基线是 x86-64-v3**（Haswell 2013+ / Gracemont 2021+），
> 老 Pentium 级机器直接跑不起来。官方文档同时建议"自己按用户群编译"【官方文档】。

---

## ③ 模型体积与延迟表（宣称 / 实测分列）

### 3.1 体积（HF blobs API 实测字节数，未做二次换算）

| 模型 | 变体 / 精度 | 文件 | 体积 | 许可 |
|---|---|---|---|---|
| **Laya English**（ModernBERT-large，421M） | safetensors（fp16） | `convaiinnovations/laya` `model.safetensors` | **803.6 MB** | apache-2.0 |
| | GGUF F16 / Q8_0 / Q6_K / **Q4_K_M** | `fr0stbit3/laya-gguf` | 754.8 / 401.9 / 328.1 / **259.6 MB** | apache-2.0 |
| | GGUF F16 / Q8_0 / Q4_K_M | `mys/laya-GGUF` | 806.9 / 430.6 / 400.5 MB | apache-2.0 |
| | ONNX INT8 / **INT4** | `techtheist/laya-onnx` `en/` | 554.7 / **262.4 MB** | apache-2.0 |
| | ONNX fp32（含外部 `.data`） | `receptron/laya-onnx` | 1607.2 MB | apache-2.0 |
| **Laya multilingual**（mmBERT-base，322M） | safetensors | `convaiinnovations/laya-multilingual` | 614.0 MB | apache-2.0 |
| | GGUF F16 / Q8_0 / Q4_K_M | `mys/laya-multilingual-GGUF` | 632.5 / 345.0 / 499.7 MB | apache-2.0 |
| | ONNX INT8 | `techtheist/laya-onnx` `multilingual/` | 873.2 MB | apache-2.0 |
| | CoreML fp16 / e8（每档按 L128–L1024 各一份） | `FluidInference/laya-coreml` | 614–618 / 427–431 MB（每档） | apache-2.0 |
| | LiteRT fp32 / wfp16 | `litert-community/laya-LiteRT` | 1606.7 / 804.8 MB | apache-2.0 |
| **Laya typed-decisions**（421M） | safetensors | `convaiinnovations/laya-typed-decisions` | 803.6 MB | apache-2.0 |
| **jev-d-0.4b-onnx** | ONNX fp32（外部 `.data`，数值等同 laya-typed-decisions） | `MissingPackage/jev-d-0.4b-onnx` | **1607.2 MB** | apache-2.0（继承） |
| **open-jev-deberta-v3-large**（DeBERTa-v3-large） | safetensors fp32 + head | `com-kotobalabs/...` | 1655.7 + 12.0 MB | apache-2.0 |
| | ONNX q4 / q4f16 / fp16 / fp32（外部 data） | `onnx-community/...-ONNX` | **455.3 / 331.7** / 833.8 / 1015.6+652.0 MB | apache-2.0 |
| **kev-0.8b** | LoRA adapter + head（**不是完整权重，需另下 Qwen3.5-0.8B 底座**） | `jaredpalmer/kev-0.8b` | 41.3 + 2.0 MB（+ 底座） | apache-2.0（adapter/head），底座 Qwen3.5 apache-2.0，数据集各自许可 |
| | GGUF F16 | `taigrr/kev-0.8b-gguf` | 1446.5 MB | apache-2.0 |
| | CoreML | `FluidInference/kev-0.8b-coreml` | 953–955 MB（每档） | apache-2.0 |
| **AgentJev-0.6B**（Qwen3-0.6B 底座） | fp32 全模块 | `aimeigaoshou/agent-jev` | **2282.8 MB** | apache-2.0 |
| | ONNX INT8 | `lujihong/agentjev-0.6b-int8-onnx` | 1018.2 MB | **未标注许可（license: None）→ 不建议使用** |
| **Jev-Style-0.8B-Decision-v3**（Qwen3.5-0.8B 底座） | GGUF F16 / Q8_0 / **Q4_K_M** | `chaoliangUNSW/...-GGUF` | 1446.5 / 774.2 / **504.8 MB** | apache-2.0，但**训练数据含限制/不明条款及 OpenAI/Anthropic 输出**（见 ⑤） |
| **Jev-Style-Qwen3.5-2B-Decision** | GGUF Q4_K_M | `chaoliangUNSW/...-GGUF` | 1251.4 MB | 同上 |

【仓库/代码：`https://huggingface.co/api/models/<id>?blobs=true` 的 `siblings[].size`】

**两个容易踩的坑（来自模型卡原文）**：
1. `kev-*` 在 HF 上**只有 adapter + head**（41 MB），不是可独立运行的模型；要落地得自己合并 Qwen3.5 底座，
   等于额外一层构建步骤与另一份许可链【仓库/代码：`jaredpalmer/kev-0.8b` README「Apache-2.0 for the adapter and head」】。
2. `jev-d-0.4b-onnx` 与 `receptron/laya-onnx` 都是**带外部 `.data` 的两文件 ONNX**，且 `jev-d` 的输入不是标准
   `input_ids/mask`，而是 `input_ids, attention_mask, marker_pos, marker_mask, qtype`，并要求
   `graphOptimizationLevel: "basic"`（`"all"` 会融合出 WebGPU 内核不接受的 `SkipLayerNormalization`）
   ——直接把 ONNX 塞进 `ort` 默认设置会**静默出错或加载失败**【仓库/代码：`MissingPackage/jev-d-0.4b-onnx` README「Four things that break silently」】。

### 3.2 延迟（**宣称**）

| 对象 | 数字 | 出处档位 |
|---|---|---|
| Laya（HF 模型卡 + 仓库 README） | 单次前向 **~33 ms**；T4：1 问 32.8 ms（multilingual）/ 39.5 ms（en），10 问批量 72.3 ms；50 问 337 ms；103–332 问/秒 | 【官方文档】HF card / repo README |
| Laya **CPU** | `Router(preload=True)`：**193–464 ms**（CPU 型号未标注） | 【官方文档】repo README 表 |
| Laya 长文 | 4,000 token 输入 ≈ **1.7 s（Apple GPU）**；仓库另载 `--head-max-len 384` 约 1.4× CPU 耗时 | 【官方文档】|
| Jev 在线 | 官方示例页写 **completed in 0.114 s** | 【官方文档】typesafe.ai |
| Jev 在线（第三方实测） | p50 **236–276 ms**（AbdelStark / nibzard，经 Laya 模型卡转引） | 【第三方文章】 |
| llama.cpp sidecar 空转开销 | **未找到**公开数字 | — |

### 3.3 延迟（**实测**，唯一有 Apple Silicon 具体型号的一条）

| 场景 | 数字 | 条件 |
|---|---|---|
| Jev-Style v3（Qwen3.5-0.8B）+ 自建 `jev-score`，`many_mode="batched"` | 10 个问题共享一个 4K-token state：**1,381 ms**；同场对照"一次一问"的 Laya 架构引擎 6,364 ms（**4.6×**） | **Apple M1 Max 64 GB，llama.cpp GGUF F16，warm p50，2026-09-23 单次 run** |
| 同上，8K-token state | **2.3–2.6 s** | 同上 |

【仓库/代码：`chaoliangUNSW/Jev-Style-0.8B-Decision-v3-GGUF` README「Results and speed」，
并自注「comparison engine 是自家 round-1 微调，不是官方 Laya checkpoint」】

> **关键工程约束**：Jev-Style 的模型卡明说「**Chat 或 text generation 拿不到决策**，决策是在每个选项的
> verdict slot 上读 logits」，随包提供的是一个**需要对着 llama.cpp 源码编译的自定义 `jev-score`（JSON-lines 进程）**，
> 测试基线是 llama.cpp commit `441df11f…`【仓库/代码】。这意味着：
> 若走 GGUF 路线，**`llama-server` 的 `/v1/chat/completions` 不够用**——要么自建 scorer 二进制（多一个 sidecar，
> 且与 llama.cpp 版本强耦合），要么用 `llama-cpp-2` 在 Rust 里自己取 logits（可行，`llama.cpp` 暴露 logits），
> 要么改用**编码器类模型（Laya / open-jev）**，它们的输出头就是分类 logits，可直接读。

**未找到**：Laya 在 Apple Silicon 上的 p50 实测；ORT/tract/candle 在本模型上的任何 Apple Silicon 或 x86 CPU 实测；
llama.cpp 与 ORT 在同一台机器上的横向对比。

---

## ④ 分发与签名

### 4.1 两种分发形态的体积账（本项目实测基线）

| 项 | 现状 | 依据 |
|---|---|---|
| 更新包 `Switchelp.app.tar.gz` | **7,698,412 B ≈ 7.34 MiB** | 【仓库/代码】`target/release/bundle/macos/` |
| 现有 sidecar `gptswitch-bridge-app-…` | **707,312 B** | 【仓库/代码】`src-tauri/binaries/` |
| 已配置 `externalBin` | `["binaries/gptswitch-bridge-app"]` | 【仓库/代码】`src-tauri/tauri.conf.json:34` |
| 已配置 `minimumSystemVersion` | `12.0` | 【仓库/代码】同上 |

**随包（bundle）**：选 Laya English Q4_K_M GGUF（259.6 MB）+ llama.cpp sidecar ⇒ 更新包 ≈ **7.3 MiB → 约 280 MiB**；
选 Laya ONNX INT4（262.4 MB）+ ORT 静态链接 ⇒ 同等量级，且没有第二个可执行文件。
两条都让"应用内更新"从"秒级"变成"分钟级"，并让 `latest.json` 单平台签名产物膨胀约 36 倍。

**首次使用下载（推荐）**：
- HF 直链**可用但不应当硬编码**：`https://huggingface.co/<repo>/resolve/main/<file>` 返回 **302** 到
  `https://us.aws.cdn.hf.co/xet-bridge-us/...?Expires=...&Signature=...`（Xet 后端签名 URL，**带过期时间**）
  ⇒ 必须在下载时解析重定向，不能缓存目标 URL【仓库/代码：实测 `curl -I`】。
- 国内可达性：`https://hf-mirror.com/` 根路径 **200**，`/convaiinnovations/laya/resolve/main/model.safetensors`
  亦 **200** ⇒ 存在可用镜像站，但**非官方**，需在应用里做成可配置镜像 + 官方源回退【仓库/代码：实测 HTTP 状态】。
- 完整性校验：**本项目已有 minisign 基础设施**（`plugins.updater.pubkey` 固化在已发布包里，
  `scripts/make-latest-json.mjs` 组装清单）【仓库/代码：`docs/architecture/06-updates.md`】。
  权重下载可复用**同一套 minisign 私钥签一份模型清单**（`name/size/sha256/minisign-sig`），
  因为 pubkey 已固化、只验签不换钥，不会触发"换密钥 = 老用户失联"的问题。
  ⚠️ **注意**：`pubkey` 一旦写进 `tauri.conf.json` 就随每个已发布包固化，
  任何"新增签名用途"都必须沿用现有密钥对，不能新生成。
- 失败降级：本项目更新链路已有 `update.signatureMismatch` 错误码与"如实说明"的界面约定
  【仓库/代码：`docs/architecture/06-updates.md`】，权重下载应复用同一姿态（校验失败即拒绝加载并回退到在线路径）。

### 4.2 sidecar 的签名与公证（macOS）

- **Apple 官方要求**（公证前置条件）：**所有**分发的可执行文件都必须有有效代码签名；
  必须用 Developer ID 证书；**应用与命令行目标都要开硬化运行时**；必须有安全时间戳；
  不得携带 `com.apple.security.get-task-allow` 的任一取值；链接 macOS 10.9+ SDK。
  【官方文档：Apple《Notarizing macOS software before distribution》JSON API】
- **Tauri 是否替我们做了**：是。`tauri-bundler` 的 macOS 打包把 `externalBin` 复制进
  `Contents/MacOS/`（`copy_binaries`），并以 `SignTarget { is_an_executable: true }` 加入签名列表；
  签名时 `target.is_an_executable && settings.macos().hardened_runtime` 决定是否开硬化运行时。
  也就是说 **sidecar 会被自动签名 + 开硬化运行时 + 由 bundler 统一提交公证**，不需要额外脚本。
  【仓库/代码：`tauri-apps/tauri` `crates/tauri-bundler/src/bundle/macos/{app.rs,sign.rs}`】
- **本仓库现状**：Developer ID 签名 ✅ 可以做；**公证 + staple ❌ 缺凭据**（钥匙串无 notarytool 配置）。
  即当前产出是"已签名未公证"，用户下载后仍会看到"无法验证开发者"【仓库/代码：`docs/development/03-signing-and-release.md`】。
  **推论**：一旦随包塞进 200+ MB 模型，未公证的代价被放大——用户要走 `xattr -dr com.apple.quarantine`
  才能打开一个几百 MB 的应用，说服成本更高【推测】。
- **本机踩过的坑（提醒，不重复踩）**：sidecar 与同名 cargo bin 重名会导致拷包互相覆盖；
  本仓库已把 sidecar 命名成 `gptswitch-bridge-app`（与包名区分）规避。
  **新增任何 sidecar（如 `llama-server`）时必须保持同一约定**：
  `binaries/<name>-<target-triple>` 且 `<name>` 不与任何 cargo bin 同名。
- **Windows**：`.msi` / `.exe` 走 Authenticode，需另购 OV/EV 证书；未签名时 SmartScreen 报"未知发布者"
  【仓库/代码：`docs/development/03-signing-and-release.md`】。sidecar 同样需要单独签名（Tauri 在
  Windows 侧的签名行为未核实，**未找到**证据）。

### 4.3 运行时体积的平台差异

| 平台 | ORT 预编译归档 | llama.cpp 发行包 | 备注 |
|---|---|---|---|
| macOS arm64（+CoreML） | 8.9 MB（lzma2，静态） | 11.8 MB（tar.gz） | 动态 dylib 实测 31.7 MB |
| macOS x86_64 | **无预编译** | 11.3 MB（tar.gz） | ORT 需自编译或用 llama.cpp |
| Windows x86_64（+DirectML） | 31.8 MB（lzma2） | 19.2 MB（zip，CPU） | x86-64-v3 基线 |

【仓库/代码：CDN `Content-Length`、`gh api repos/ggml-org/llama.cpp/releases/tags/b11275`、PyPI 中央目录】

### 4.4 系统版本下限的冲突

- **ONNX Runtime ≥ 1.24 的 macOS arm64 wheel 从 `macosx_13_0` 跳到 `macosx_14_0`**
  （1.23.x = 13_0；1.24.1–1.30.0 全部 = 14_0）⇒ **ORT 现阶段实际要求 macOS 14+**。
  【仓库/代码：PyPI release 历史 wheel tag 抽样】
- **llama.cpp 官方 macOS 发行包用 `-DCMAKE_OSX_DEPLOYMENT_TARGET=13.3` 构建** ⇒ **要求 macOS 13.3+**。
  【仓库/代码：`ggml-org/llama.cpp` `.github/workflows/release.yml`】
- 本项目 `tauri.conf.json` 声明 `minimumSystemVersion: "12.0"`，
  而 `docs/architecture/05-security-and-platforms.md` 的用户目标写的是「macOS 14+」。
  ⇒ **两者已有不一致**；引入任何本地推理都会把下限钉到 **13.3（llama.cpp）或 14.0（ORT）**，
  必须同时改 `tauri.conf.json`、文档矩阵与 `latest.json` 的平台键【推测：Tauri 的 `darwin-aarch64` 平台键
  不区分最低系统版本，因此老系统用户会照常收到更新但因无法加载模型而静默降级，需在应用内做版本探测与提示】。

---

## ⑤ 许可合规

| 对象 | 许可 | 闭源商业分发 | 依据 |
|---|---|---|---|
| ONNX Runtime 本体 | **MIT** | ✅ 可以（保留版权与许可声明） | 【仓库/代码】`microsoft/onnxruntime/LICENSE` |
| llama.cpp / ggml | **MIT** | ✅ 可以 | 【仓库/代码】`ggml-org/llama.cpp/LICENSE` |
| `ort` / `llama-cpp-2` / `candle` / `tract` / `burn` | MIT OR Apache-2.0 | ✅ 可以 | 【仓库/代码】crates.io |
| Laya（en / multilingual / typed-decisions） | **apache-2.0** | ✅ 可以；模型卡带 `commercial-use` 标签 | 【仓库/代码】HF cardData |
| open-jev-deberta-v3-large | apache-2.0（底座 `microsoft/deberta-v3-large` 为 MIT） | ✅ 可以 | 【仓库/代码】HF cardData |
| jev-d-0.4b-onnx | apache-2.0（自述 "inherited"，数值等同 laya-typed-decisions） | ✅ 可以 | 【仓库/代码】`NOTICE` 引用 |
| kev-* | apache-2.0（adapter/head）；底座 Qwen3.5 apache-2.0；**数据集各自许可** | ⚠️ 需逐条审数据集 | 【仓库/代码】`jaredpalmer/kev-0.8b` README |
| AgentJev-0.6B | apache-2.0（底座 Qwen3-0.6B apache-2.0） | ✅ 可以 | 【仓库/代码】HF cardData |
| **lujihong/agentjev-0.6b-int8-onnx** | **未标注（license: None）** | ❌ **不要用** | 【仓库/代码】HF cardData |
| **Jev-Style-0.8B-Decision-v3** | 文件头 apache-2.0，**但模型卡明写**"部分训练数据有受限或不明条款，部分训练样本是 OpenAI 与 Anthropic 模型的输出" | ⚠️ **有再分发风险，商用前须法务确认** | 【仓库/代码】GGUF README「Licence」 |
| GGUF 权重的再分发 | 随上游许可（本清单里均是 apache-2.0）；需保留 LICENSE/NOTICE 归属 | ✅（apache-2.0 系） | 【仓库/代码】各仓库 `LICENSE`/`NOTICE` 文件存在性 |
| 量化/格式转换产物 | Apache-2.0 派生作品仍为 Apache-2.0（Jev-Style v3 与 jev-d 均如此自述） | ✅ | 【仓库/代码】 |

**本项目已有的发布门禁可以承接**：`docs/architecture/05-security-and-platforms.md` §8 已要求
「依赖锁定、SBOM、许可证清单、漏洞扫描与签名证据纳入发布产物」，且明确
「商业分发是否可复用参考代码是单独工程门禁」【仓库/代码】。模型权重应作为**独立条目**进入同一份清单，
包含：仓库 ID、文件、SHA-256、许可、底座许可链、训练数据声明。

**红线小结**：MIT/Apache-2.0 的运行时与权重都可以闭源商业分发；唯一明确要避开的是
**未标注许可的权重**（`lujihong/agentjev-0.6b-int8-onnx`）与**训练数据条款不清的权重**（Jev-Style v3）。

---

## ⑥ 成本对照表（在线 vs 本地 vs 复用已有供应商）

### 6.1 在线 Jev 的官方价格

**Jev 1.13（`jev-1.13.0`）：$42 / Btok = $0.042 / Mtok，只按输入 token 计费，输出 token 免费。**
限流 100K tokens/秒、40 请求/秒；上下文 64k/请求（state 32k + 最长问题）；端点 `POST https://api.typesafe.ai/v1/systemone`。
官方未列出任何免费额度【官方文档：`docs.typesafe.ai/models.md`、`typesafe.ai` 首页】。

### 6.2 每天 1 万次决策

| 单次决策输入规模 | 每天 token | **每天成本** | 每月（30 天） |
|---|---|---|---|
| 300 tok（短 ticket + 1 个问题） | 3.0 M | **$0.126** | $3.8 |
| 1,000 tok | 10.0 M | **$0.42** | $12.6 |
| 1,928 tok（= 官方示例页 $0.000081/次 反推） | 19.3 M | **$0.81** | $24.3 |
| 8,000 tok（长文档 state） | 80.0 M | **$3.36** | $100.8 |

【官方文档价格 × 算术；反推的 1,928 tok：`$0.000081 ÷ $42 × 1e9`】

### 6.3 本地推理

| 项 | 值 | 档位 |
|---|---|---|
| API 费用 | **$0** | — |
| 电费（Jev-Style v3 实测 1,381 ms / 10 问 / 4K state，按 25 W 整机估算） | 约 **1 mWh/决策** ⇒ 1 万次 ≈ 9.7 Wh/天 ≈ **$0.0015/天**（$0.15/kWh） | 【推测】功耗为估值，耗时为【仓库/代码】实测 |
| 一次性成本 | 工程工时（见 ⑧）+ 若随包则每人 260 MB 下载流量 | — |
| 隐性成本 | 冷启动加载（Laya 模型卡自述 checkpoint 冷建 "seconds"，`max_loaded=1` 时 CPU 重载中位 **7.4 s**）；内存常驻（建议只保 1 份） | 【官方文档】repo README |

⇒ **本地推理的"省钱"幅度上限是每天 1 美元以内**（1 万次/天量级）。**它不是省钱方案，是隐私方案。**

### 6.4 第三条路：复用用户已配置的供应商（Switchelp 的原生优势）

Switchelp 的用户**本来就持有至少一个 OpenAI 兼容供应商的 Key**（这正是产品核心）。用其中最小档模型做决策：

| 供应商 / 模型 | 输入 $/Mtok | 输出 $/Mtok | 结构化输出 | 档位 |
|---|---|---|---|---|
| OpenAI `gpt-5-nano` | **$0.05** | $0.40 | JSON mode | 【官方文档】`developers.openai.com/api/docs/pricing.md` |
| OpenAI `gpt-4.1-nano` | $0.10 | $0.40 | JSON mode | 同上 |
| OpenAI `gpt-4o-mini` | $0.15 | $0.60 | JSON mode | 同上 |
| DeepSeek `deepseek-flash` | $0.15（off-peak，cache miss）/ $0.30 peak | $0.60 / $1.20 | **Json Output ✓** | 【官方文档】`api-docs.deepseek.com/quick_start/pricing` |
| 智谱 `GLM-4.7-Flash` / `GLM-4.5-Flash` / `GLM-4-Flash-250414` | **免费模型**（文档标注「免费文本模型」） | — | 未核实 | 【官方文档】`docs.bigmodel.cn` 模型总览 |
| Google Gemini Flash-Lite 免费档 | **未找到**（价格页 302 到 OAuth，无法直读） | — | — | — |

**1 万次/天（每次 ~300 输入 + ~30 输出 token）**：
`gpt-5-nano` ≈ 3.0 M×$0.05 + 0.3 M×$0.40 = **$0.15 + $0.12 = $0.27/天**；
智谱免费档 = **$0/天**（受其免费额度与限流约束，未核实具体上限）。

**这条路的技术要求与代价**：
1. 需要模型支持 **JSON/结构化输出**（上表已标注，OpenAI 与 DeepSeek 官方支持；智谱未核实）。
2. **语义不等价**：决策模型返回的是**校准过的概率分布**（Laya 模型卡自述 RLCD 训练、ECE 经温度拟合后 0.466→0.081），
   LLM 的 token 概率**不能当作校准置信度**用【官方文档】。若产品要用 `confidence` 做门控/路由
   （`docs/architecture` 的既有姿态是"不确定就交给用户"），这条路拿不到可信的 confidence。
3. 延迟：官方 Jev 示例 0.114 s / 第三方实测 Jev p50 236–276 ms；小 LLM 生成式输出通常更慢
   （官方示例页对照 "LLMs 8.566 s"）【官方文档 / 第三方文章】。

⇒ **第三条路是"零边际成本、零新增分发体积"的最优工程解，代价是失去 calibrated confidence**。
若产品的决策点只需 argmax（"这个问题属于 billing 还是 technical"），这条路的性价比最高。

---

## ⑦ Windows 差异

| 维度 | 现状 / 结论 |
|---|---|
| 本项目 Windows 支持现状 | 安装（NSIS，未签名）与打开、供应商/Key（keyring `windows-native`）、模型管理、进程重启可用；**凭据 helper 是显式桩（`error.windowsHelperUnimplemented`）⇒ 网关起不来、应用/还原被拦**；Codex 安装位置探测与共存模式不成立；**应用内更新在 Windows 不产签名产物**【仓库/代码：`docs/architecture/05-security-and-platforms.md` §6 实现现状】 |
| 对本地推理的直接影响 | 本地决策模型**不依赖 Codex 网关**，理论上可在 Windows 单独落地。但：①应用内更新链路在 Windows 是断的 ⇒ **首次使用下载权重的现成管道不存在**，要么随包（+260 MB 的 NSIS 安装包），要么自建下载器；②若决策要接进网关路由，Windows 网关本身还没起来 ⇒ **收益无法兑现** |
| CPU 推理 | ORT Windows x64 预编译可用（DirectML EP 默认含；**无 CUDA 也能跑，DirectML 走 D3D12**）；llama.cpp 官方出 `llama-cpp-2` 的 win-cpu-x64 包。**无需 CUDA** |
| 硬性硬件下限 | pyke 的 **x86-64-v3 基线**（Haswell 2013+ / Excavator / Ryzen）⇒ 老 Intel 低端机（Pentium）**直接不可用**；官方建议自编译以覆盖更广【官方文档】 |
| 体积 | ORT 预编译归档 31.8 MB（lzma2，+DirectML）；llama.cpp win-cpu-x64 19.2 MB（zip）——与 macOS 同量级 |
| 额外工作量估计 | **未找到**任何可比项目的工时数据。**推测**：若只做"Windows 上也能本地决策"且不接网关，主要成本是 ①新建权重下载+校验管道（macOS 侧可复用，Windows 需在无 updater 前提下自建）②`tauri.conf.json` 增 `x86_64-pc-windows-msvc` sidecar/静态库 ③CI 增加 Windows 构建矩阵 —— 属于**中等**工程量；若要求"接进网关路由"，则**先决于 Windows 凭据 helper 补齐**，那是另一条独立的、更大的工作流 |

---

## ⑧ 推荐方案（含推测）

> 以下推荐均为【推测】，基于 ②–⑦ 的事实；需要实测才能定论。

**方案 A（推荐，先做）：不做本地推理，先把"第三条路"做成一等公民。**
复用用户已配置的供应商 + 最小档模型（`gpt-5-nano` / `deepseek-flash` / 智谱免费档）做决策，
**$0–0.27/天（1 万次）**，零分发体积增加，零签名/公证变更，Windows 天然可用。
代价：拿不到 calibrated confidence ⇒ 只在 argmax 场景用，confidence 门控保留给在线 Jev。
**理由**：成本对照里本地推理的省钱上限 <$1/天，而工程与分发代价是数百 MB 与两个平台的签名链路。

**方案 B（若"隐私"是硬需求，则作为 A 的隐私档）：ONNX INT4 in-process，不引入 sidecar。**
`ort` + `techtheist/laya-onnx` `en/model_int4.onnx`（**262.4 MB**）+ **首次使用下载**（不随包）。
- 优点：无第二个可执行文件（规避 sidecar 重名坑与 Windows sidecar 签名不确定性）；
  Tauri bundler 只管一个二进制；ORT 是 MIT；Laya 是 apache-2.0 且带 `commercial-use`。
- 必须处理：`minimumSystemVersion` 从 12.0 提到 **14.0**（ORT ≥1.24 的 macOS 下限）；
  若必须支持 Intel Mac，**ort 无 x86_64-apple-darwin 预编译**，只剩 llama.cpp 或自编译两条路。
- 必做验证（当前全部**未找到**数据）：在 M 系列与 x86 CPU 上实测 p50/p95、内存常驻、
  冷启动时间、INT4 与 fp32 的决策一致率。

**方案 C（仅当产品需要 Jev-Style 那类 25.6K 长上下文决策时）：llama.cpp sidecar + GGUF。**
Laya GGUF Q4_K_M **259.6 MB**（`fr0stbit3/laya-gguf`）或 Jev-Style v3 Q4_K_M **504.8 MB**。
- 必须处理：`llama-cpp-sys-2` 依赖 **cmake + C/C++ 工具链**（CI 的 Windows runner 也要装）；
  macOS 部署目标 **13.3**；sidecar 命名避开 cargo bin 同名；
  若选 Jev-Style，**`llama-server` 不够**，需要取 logits 的读取逻辑（自建 scorer 或 `llama-cpp-2` 直读 logits）。
- 不建议选 Jev-Style v3：许可段自述训练数据条款不清，商用再分发有风险（⑤）。

**跨方案的三条公共工程要求**（都建立在既定基础设施上）：
1. 权重走**首次使用下载**，用**现有 minisign 密钥对**签一份"模型清单"（`name/size/sha256/sig`），
   不新生成密钥（pubkey 已固化）【仓库/代码：`docs/architecture/06-updates.md`】。
2. 下载源可配置 + `hf-mirror.com` 回退（镜像实测 200，但非官方，默认关闭）；解析 302 到 CDN 后才下载。
3. 校验失败 ⇒ 明确拒绝加载并回退在线路径，沿用 `update.signatureMismatch` 那套"如实说明"的 UI 姿态。

---

## ⑨ 来源清单

**运行时 / 打包**
1. crates.io API：`ort` / `llama-cpp-2` / `candle-core` / `tract-onnx` / `burn` / `onnxruntime` — `https://crates.io/api/v1/crates/<name>`【仓库/代码】
2. pykeio/ort 预编译矩阵与 provenance：`https://github.com/pykeio/ort/blob/main/ort-sys/build/download/dist.tsv`、`https://ort.pyke.io/`（Prebuilt binaries / linking 页）【官方文档】【仓库/代码】
3. `ort` 纯 Rust 备选后端：`https://ort.pyke.io/backends/tract`、`https://ort.pyke.io/backends/candle`【官方文档】
4. llama-cpp-sys-2 构建脚本（cmake）：`https://github.com/utilityai/llama-cpp-rs/blob/main/llama-cpp-sys-2/build.rs`【仓库/代码】
5. llama.cpp 发行资产与部署目标：`https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b11275`、`.github/workflows/release.yml`【仓库/代码】
6. ModernBERT 已被 llama.cpp 原生支持：`https://github.com/ggml-org/llama.cpp/blob/master/src/models/modern-bert.cpp`【仓库/代码】
7. candle / candle-onnx / burn / tract：各自 GitHub README 与 crate README【仓库/代码】
8. ONNX Runtime 发行资产：`https://api.github.com/repos/microsoft/onnxruntime/releases/tags/v1.30.0`；PyPI `onnxruntime` wheel 中央目录（macOS arm64 dylib 31.7 MB / Windows dll 17.6 MB）；wheel tag 历史（1.23=macosx_13_0 → 1.24+=macosx_14_0）【仓库/代码】

**产品侧（Jev / Laya / 替代品）**
9. TypeSafe 官方定价与限流：`https://typesafe.ai/`、`https://docs.typesafe.ai/models.md`、`https://docs.typesafe.ai/api.md`【官方文档】
10. Laya 模型卡与仓库 README：`https://huggingface.co/convaiinnovations/laya`、`https://github.com/NandhaKishorM/laya`（33 ms / 32.8 ms / 193–464 ms CPU / 冷重载 7.4 s / 第三方 Jev p50 236–276 ms 转引）【官方文档】
11. Jev-Style v3 GGUF 模型卡（体积表、M1 Max 实测延迟、许可免责声明）【仓库/代码】`https://huggingface.co/chaoliangUNSW/Jev-Style-0.8B-Decision-v3-GGUF`
12. open-jev / kev / AgentJev / jev-d 模型卡与 HF blobs API【仓库/代码】

**签名 / 分发**
13. Apple《Notarizing macOS software before distribution》官方 JSON API（Developer ID + 硬化运行时 + 安全时间戳 + 禁 get-task-allow + 10.9 SDK）【官方文档】
14. `tauri-apps/tauri` bundler：`crates/tauri-bundler/src/bundle/macos/{app.rs,sign.rs}`（externalBin 复制到 `Contents/MacOS/` 并以 executable 身份签名）【仓库/代码】
15. Tauri 2 sidecar 文档（`bundle.externalBin`、`-$TARGET_TRIPLE` 命名、`app.shell().sidecar()`）【官方文档】`https://tauri.app/develop/sidecar/`
16. 本仓库：`docs/development/03-signing-and-release.md`、`docs/architecture/06-updates.md`、`docs/architecture/05-security-and-platforms.md`、`src-tauri/tauri.conf.json`【仓库/代码】

**成本**
17. OpenAI 定价：`https://developers.openai.com/api/docs/pricing`【官方文档】
18. DeepSeek 定价：`https://api-docs.deepseek.com/quick_start/pricing`【官方文档】
19. 智谱模型总览（标注「免费文本模型」）：`https://docs.bigmodel.cn/cn/guide/start/model-overview`【官方文档】

**未核实 / 未找到（明确记录）**
- Laya 在 Apple Silicon 上的实测延迟；ORT / tract / candle 在本类模型上的任何实测；llama.cpp sidecar 空转开销。
- `llama-server` 单独二进制的解包后体积（只测到发行包 tar.gz/zip 大小）。
- Google Gemini Flash-Lite 免费档价格（价格页 302 至 OAuth，无法直读）。
- 智谱免费模型的结构化输出支持与免费额度上限。
- Tauri 在 Windows 上对 sidecar 的签名行为。
- `x86_64-apple-darwin` 上 `ort` 的可用性（dist.tsv 无此 target；未确认是否有社区构建）。
- 本地推理与在线 Jev 的**决策一致率**（决定"本地可否替代在线"的质量问题，本文未涉及）。
