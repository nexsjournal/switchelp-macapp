# Jev 的免费/开源替代品：Laya、open-jev 生态与各家复刻模型

**调研日期：** 2026-09-30
**调研范围：** Swift/桌面应用 Switchelp（Tauri 2 + React + Rust，本机网关注入 Codex）能否以**零成本开源方案**替代或旁路 TypeSafe Jev。只覆盖「决策模型」类别（输入 state + 类型化问题 Choice/Score/Noul，输出带校准概率的结构化决策，不生成文本）。
**范围声明：**
- 本文所有数字**均来自公开 API / 模型卡 / 仓库元数据**，未在本地实际下载或运行任何模型。凡模型作者自报的数字标「宣称」，第三方复现标「实测」，来源已逐条标注。
- 证据档位：【官方】= 发布方自述/模型卡；【仓库/代码】= 从 GitHub/HF API 元数据、文件清单、提交记录直接读出；【第三方文章】= 独立测评方；【推测】= 我的推断。
- 本次未能使用的工具：`/tmp/webtools/search.py` 的 Bing/DDG 后端在沙箱内返回空（`backend=none results=0`），WebSearch 工具在此供应商下不可用。仅 Bing HTML 直连 + HF/GitHub API + WebFetch 可用。因此**「未找到」不等于不存在**。
- 生态成熟度警示：Jev 于 2026-09-15 发布，本文绝大多数仓库创建于 2026-09-16 ~ 09-24 这 9 天内，存在明显的批量灌水特征（详见 §5.3）。

---

## ① 一句话结论

**能真正塞进桌面应用的只有一条路：`ollaya`（Apache-2.0，Rust，Tauri 2 同栈，macOS arm64 二进制压缩后仅 ~10–15 MB）+ `laya` 系权重（Apache-2.0，fp16 614–804 MB / INT8·GGUF 量化后 225–500 MB），走 `POST /v1/systemone` 本机 11435 端口。** 但必须接受一个前提：**Laya 不是零样本决策引擎**——它的官方模型卡自己写明零样本 typed-decisions 只有 0.362（多数类基线 0.461），且默认过度自信（ECE 0.466，需在自己数据上重拟合温度）。kev / von / openjev / NanoJev 这些复刻要么只发 LoRA（需额外 8–9 GB Qwen3.5 基座）、要么非商用（CC-BY-NC）、要么**无许可证**，分发风险显著高于 Laya。

---

## ② Laya 家族表

官方 HF 组织 `convaiinnovations`，代码库 GitHub `NandhaKishorM/laya`（★28,855 / 2,518 fork / Apache-2.0 / 创建 2026-09-18 / 最近推送 2026-09-29）【官方】【仓库/代码】

| 模型 | 骨干 | 参数量 | 体积（fp16 safetensors） | 上下文 | 许可证 | 来源 |
|---|---|---|---|---|---|---|
| `convaiinnovations/laya`（英文） | ModernBERT-large | 421M | 803.6 MB | 512 | Apache-2.0 | HF API【仓库/代码】 |
| `convaiinnovations/laya-multilingual` | mmBERT-base | 322M | 614.0 MB | 1024（RoPE 可到 8192） | Apache-2.0 | HF API【仓库/代码】 |
| `convaiinnovations/laya-typed-decisions` | ModernBERT-large | 421M | 803.6 MB | 1024 | Apache-2.0 | HF API【仓库/代码】 |

- 三个 checkpoint **同在一个 repo**（子目录 `multilingual/`、`typed-decisions/`），下载时只取需要的那一个。【官方】
- **「non-autoregressive」是什么意思**：不做逐 token 生成。双向编码器（ModernBERT/mmBERT）对「state + 类型化问题」做**一次前向**，直接从输出位置的分数读出每个选项的概率，排列组合成 choice/score/noul 答案。因为不生成 token，所以没有可解析的文本、也没有幻觉——代价是**只能回答你事先声明的问题与选项**。【官方】
- **运行时生态**：PyTorch 参考实现（PyPI `laya` 0.3.22，Apache-2.0，30 个 release）；`laya[onnx]` ONNX Runtime 路径；自研 `laya-ts` TypeScript SDK（仓库内 `laya-ts/`，含自己的 tests/examples）；`sdk/typescript`；内置 MCP server（`laya[mcp]`）；Docker compose（CPU/CUDA/HTTP/Spark 四套）。【仓库/代码】
- **成熟度：可跑**。仓库含 80+ 个 `tests/test_*.py`、`benchmarks/`、`examples/`、`notebooks/`（含 Kaggle 2×T4 微调 notebook）、`research/`、`verify/`、`nix`/`flake`。【仓库/代码】

### 2.1 社区量化与各运行时移植

| 仓库 | 面向运行时 | 体积实测 | 许可证 | 备注 |
|---|---|---|---|---|
| `mys/laya-GGUF` | llama.cpp | f16 806.9 MB / q8_0 430.6 MB / ud_q4_k_m **400.5 MB** | Apache-2.0 | 下载 6,655【仓库/代码】 |
| `mys/laya-multilingual-GGUF` | llama.cpp | f16 632.5 / q8_0 345.0 / ud_q4_k_m 499.6 MB | Apache-2.0 | 下载 8,580【仓库/代码】 |
| `Weidows/laya-multilingual-GGUF` | llama.cpp | IQ4_XS **224.6 MB** … Q8_0 325.3 MB（10 档） | Apache-2.0 | 下载 2,996；档位最全【仓库/代码】 |
| `mys/laya-typed-decisions-GGUF` | llama.cpp | f16 810.4 / q8_0 434.1 / q4_k_m 404.0 MB | Apache-2.0 | 下载 2,617【仓库/代码】 |
| `receptron/laya-onnx` | ONNX Runtime | 仅图 **3.6 MB**（`laya.onnx`，权重另取） | Apache-2.0 | 与 `receptron/laya` 配套【仓库/代码】 |
| `techtheist/laya-onnx` | ONNX Runtime（含 Web） | 全仓 1,690.3 MB（en int4 / en int8 / multilingual int8） | Apache-2.0 | 单文件体积未逐项公布【仓库/代码】 |
| `mizchi/laya-multilingual-onnx` | ONNX Runtime | 616.9 MB（`model.onnx`） | Apache-2.0 | 下载 0【仓库/代码】 |
| `FluidInference/laya-coreml` | Apple Core ML（CPU+GPU） | 每档 427–618 MB（int8 e8 ≈427–431 MB；fp16 ≈614–618 MB，L128–L1024 四档） | Apache-2.0 | 下载 365【仓库/代码】 |
| `aac6fef/laya-mlx` | Apple MLX | 803.6 MB safetensors | Apache-2.0 | 108 likes / 下载 0【仓库/代码】 |
| `aac6fef/laya-multilingual-coreml-ane` | Core ML + **Neural Engine** | 全仓 615.6 MB（weights 375.4 + bin 238.6 + mlmodel 1.6） | Apache-2.0 | 下载 546【仓库/代码】 |
| `litert-community/laya-LiteRT` | LiteRT（Android/移动） | en fp32 1,607 MB / **en fp16 804.8 MB**；multilingual fp32 1,228 MB / fp16 614.2 MB | Apache-2.0 | 面向移动端，桌面不必用【仓库/代码】 |
| `onnx-community/laya-multilingual-ONNX` | Transformers.js / WebGPU | 未逐项抓取 | — | 存在性确认【仓库/代码】 |
| `killkli/open-jev-laya-multilingual-onnx` | ONNX Runtime | 未抓取 | — | 下载 263【仓库/代码】 |

**结论**：macOS 桌面场景下，**`mys`/`Weidows` 的 GGUF（224–400 MB）+ llama.cpp**，或 **`receptron/laya-onnx`（3.6 MB 图 + 803 MB 权重）**，是体积/工程量最优的两组。

---

## ③ 其它开源决策模型表（复刻/衍生）

| 项目 | 基座 | 体积实测 | 许可证 | 成熟度 | 判断依据 |
|---|---|---|---|---|---|
| `jaredpalmer/kev-*` | Qwen3.5/3.8 | **仅 LoRA**：0.8b 41.3 MB / 4b 123.9 MB / 9b 165.2 MB（+ head.pt 5 MB） | Apache-2.0 | **可跑** | ★7,973；100 commits（API 上限）；有 `tests/`、`evals/`、`playground/`、`docs/`、`runs/`、`space/`；`kev-4b` 下载 12,014【仓库/代码】 |
| `wfzyx/von` | ModernBERT-large | 1,506.0 MB（fp32） | Apache-2.0 | **可跑** | ★780；100 commits；`tests/`、`benchmarks/`、`training/`、`js/`、`hf/`、`docs/`；HF 下载 **51,619**（本次调查中最高）【仓库/代码】 |
| `C-Tianyu/NanoJev` | Qwen3-0.6B | 2,274.6 MB / 变体（fp32） | **⚠ 无许可证**（HF `license: None`） | **可跑** | GitHub ★2,443 / 40 commits / MIT / `tests/`+`docs/`+`research/`+`configs/`；**但 HF 权重仓无 license 字段**【仓库/代码】 |
| `TheoLeeCJ/SemIf` | — | 未在 HF 找到对应权重仓 | MIT（代码） | **可跑** | ★4,594；25 commits；`benchmarks/`、`demo/`、`tests/`、`webgpu-demo/`、`exl3-bridge/`、`manifests/`、`THIRD_PARTY.md`。宣称「3090 在家跑」【仓库/代码】 |
| `aimeigaoshou/agent-jev`（AgentJev-0.6B） | Qwen3-0.6B | 2,282.8 MB（fp32） | Apache-2.0 | 未知 | HF 下载 863 / 36 likes；GitHub 未找到对应 `malevrigns/agent-jev`（该路径 401/不存在）【仓库/代码】 |
| `Heman10x-NGU/Verdict-open-jev`（含 `heman10x/rlcd-modernbert-151m`） | GLiClass ModernBERT base | 151M；onnx 578.2 MB / **fp16 onnx 289.7 MB** | GitHub **NOASSERTION**（不明确）；HF 权重 Apache-2.0 | **可跑** | GitHub ★110 / 26 commits；`core/`、`export/`、`rlcd/`、`tests/`、`webgpu-demo/`、`artifacts/`、`reports/`【仓库/代码】 |
| `openjev/openjev` | 未标注（体积推断为 27B 级） | fp16 全仓 **52.2 GB**；FP8 28.98 GB；MLX 27.26 GB / 4bit 14.43 GB；GGUF Q4_K_M **15.78 GB** | **⚠ CC-BY-NC-4.0（非商用）**；但 `openjev-GGUF` 仓标 **apache-2.0**（自相矛盾） | 未知 | HF 下载 4,288 / 81 likes；**无 GitHub 组织**（`gh api orgs/openjev` 404）；GGUF 仓 license 与母仓冲突【仓库/代码】 |
| `com-kotobalabs/open-jev-deberta-v3-large` | deberta-v3-large（MIT 基座） | 1,667.7 MB（model 1,655.7 + head 12.0） | Apache-2.0 | 未知 | HF 下载 2,675 / 69 likes；无独立 GitHub 仓库证据【仓库/代码】 |
| `leobitz/jev-berta-base-zeroshot-classifier` | deberta-v3-base | 未抓取 | MIT | 未知 | HF 下载 74【仓库/代码】 |
| `argos1111/modernbert-ja-310m-jev` | modernbert-ja-310m（MIT） | 未抓取 | **⚠ CC-BY-SA-4.0（传染性 ShareAlike）** | 未知 | HF 下载 1,021 / 15 likes【仓库/代码】 |
| `akhilaaa3/Jev-Omni` | gemma-4-12B-it（Apache-2.0） | GGUF Q4_K_M 7,039.4 MB + mmproj 116.4 MB | Apache-2.0 | 未知 | HF 下载 923 / **315 likes**；多模态（图文）；JevBench 榜 Capability 76.5（第三方）【仓库/代码】【第三方文章】 |
| `alibiserikbay/JevK5` | Qwen3.5 | GGUF：2b Q8_0 1,918.8 MB；4b Q4_K_M 2,583.3 MB；9b Q5_K_M 6,168.3 MB | Apache-2.0 | 未知 | JevK5-GGUF 下载 6,315；JevBench 榜 Capability 72.3（4B）【仓库/代码】【第三方文章】 |
| `chaoliangUNSW/Jev-Style-Qwen3.5-2B-Decision-GGUF` | Qwen3.5-2B-Base | BF16 3,716.8 / Q8_0 1,980.5 / **Q4_K_M 1,251.4 MB** | Apache-2.0 | 未知 | 下载 7,826（该系列最高）【仓库/代码】 |
| `vinnylarouge/jevlike` | 自研（option-attention head） | HF 无同名权重仓 | MIT（代码） | **疑似空壳/早期** | GitHub ★1,335 但**仅 3 次提交**，创建与最后推送同为 2026-09-16；目录含 `tests/`、`examples/`、`docs/`【仓库/代码】 |
| `logan-markewich/jeff`（GliFormer） | GliFormer | 未抓取 | MIT | **可跑（早期）** | ★273 / 9 commits / `src/`+`tests/`+`bench/`+`deploy/` / pushed 2026-09-20【仓库/代码】 |
| `mstrasser/Jeff-Qwen3.5-0.8B`（及 2B、Gemma4-E2B） | Qwen3.5-0.8B | 1,627.0 MB + readout 0.5 MB | Apache-2.0 | 未知 | HF 下载 235；注意与 `logan-markewich/jeff` **不是同一个项目**【仓库/代码】 |
| `MissingPackage/jev-d-0.4b-onnx` | — | 未抓取 | 未抓取 | **疑似空壳** | **GitHub 仓库 404**；HF 侧仅 0 下载 / 1 like【仓库/代码】 |
| `Heman10x-NGU/openJev-verdict-2.0` | — | — | — | **未找到** | 该名字在 HF 上不存在；实际项目名为 `Verdict-open-jev` / `rlcd-modernbert-151m`（151M 非自回归宣称可对得上）【仓库/代码】 |

**关于 kev 的关键工程事实**：kev 发布的是 **LoRA 适配器 + 指针头，不含基座权重**。以 ollaya 的 `kev:4b` manifest 为例，完整下载量 = Qwen3.5-4B-Base 5,082.5 MB + 3,805.6 MB + adapter 123.9 MB + head.pt 5.0 MB + tokenizer 19.1 MB ≈ **9.0 GB**。对桌面应用而言这是沉重的首次下载。【仓库/代码】

---

## ④ 服务/集成层表

| 项目 | 语言/形态 | 许可证 | 成熟度 | 关键事实 | 来源 |
|---|---|---|---|---|---|
| **`ollaya-dev/ollaya`** | Rust workspace + **Tauri 2 桌面端** | **Apache-2.0** | **可跑（最强）** | ★1,001；创建 2026-09-18 / 推送 2026-09-29；100+ commits；`crates/`：api / decision / lang / mlx / mlx-sys / registry / runner / server；**`desktop/` 是独立 Tauri 2 workspace（`@tauri-apps/cli` 2.11.5、Tailwind 4.3.3、TS 7.0.2）**；官方发布物含 `Ollaya-macos-arm64.dmg`、`ollaya-darwin-arm64.tar.zst`、Windows msi/exe、Linux deb/AppImage/rpm | 【仓库/代码】 |
| `receptron/laya` | TypeScript / Node 20+ / ONNX Runtime | MIT | **可跑** | ★631 / 16 commits / pushed **2026-09-21（已 9 天未动）**；npm `@receptron/laya` **0.1.2**（仅 3 个版本）；ONNX 权重首次使用时从 HF 下载（**约 1.7 GB fp32**），缓存 `~/.cache/receptron-laya`；文档明示**需约 2 GB RAM** 加载模型，另加每批几百 MB；`test/` 含 test_download / test_model / test_sequence；与 Python 参考实现输出「对齐到小数点后四位」【宣称】 | 【仓库/代码】【官方】 |
| `nokia-applied-research/AnyJev` | Python / PyPI `anyjev` 0.2.0 | Apache-2.0 | **可跑** | ★976 / 42 commits / 有 CI badge；作者署名 **Nokia Applied Research + Tencent Hunyuan**；`tests/`、`bench/`、`demo/`、`space/`、`docs/levels.md`、`THIRD_PARTY.md`、`ROADMAP.md`。把任意 LLM 变成 Jev 式端点（**无需训练**）：宣称 order-flip 0.230→0.073、ECE 0.240→0.095、5% 风险下可自动决断率 7.7%→52.0%（用 100–500 条标注）；经 vLLM embed server + 闭式头落地，**需要 GPU 才有意义** | 【仓库/代码】【官方】 |
| `featherless-ai/simple-jev` | Python / HF Transformers + PyTorch | Apache-2.0 | **可跑（活跃）** | ★574 / 57 commits / pushed **2026-09-30**；`common/`、`eval/`、`hf-server/`、`demos/`、`RFDT/`、`website/`。读 next-token logits 构造响应，**不让模型生成 JSON**。有**公开免费 demo API**（无鉴权、2k token 上限、2 RPS） | 【仓库/代码】 |
| `razorback16/openjev` | Python / vLLM + MLX | Apache-2.0 | **可跑** | ★541 / 54 commits / `tests/`、`Dockerfile`、`docker-compose`。主模型 **DiffusionGemma 26B-A4B（NVIDIA，Apache-2.0）**，跑 NVIDIA GPU 或 Apple MLX；同时可服务 `laya-1.0`、`verdict-1.4`、`clm-v0.1`、`jevk5-0.2`；宣称短文本 ~80 ms、网页决策 ~210 ms（单 H100）；有免费托管 `api.codiv.ai`（100M input tokens） | 【仓库/代码】【官方】 |
| `ekzhang/openjev-sglang` | Python / SGLang | **⚠ 无许可证（`license: null`）** | 可跑 | ★334 / 20 commits / `tests/`、`evals/`、`modal_app.py`。Qwen3.6-35B-A3B，**prefill-only**，每个容器一张 **B200**；部署走 Modal。桌面场景不可行 | 【仓库/代码】 |
| `1Panel-dev/laya-server` | TypeScript（前后端） | Apache-2.0 | **可跑** | ★83 但 **65 commits**、pushed 2026-09-30（活跃）；`backend/`、`frontend/`、`Dockerfile`、`compose.yaml`、`docs/`。自托管 API + Web UI，兼容 TypeSafe Jev API 格式 | 【仓库/代码】 |
| `mizorewww/laya-mlx` | Python / MLX | Apache-2.0 | **⚠ 星标/提交比异常** | ★**6,643** 但**仅 6 commits**（创建 09-19、推送 09-22）；含 `tests/`、`benchmarks/`、`examples/`、`NOTICE`。宣称 13.4 ms 中位（英文短决策）/ 7.4 ms（multilingual），峰值 MLX 分配 **943.6 / 687.6 MiB**，三 checkpoint 在 FP32/FP16 下 **63/63 一致（378/378 次比较）** | 【仓库/代码】 |
| `mizorewww/laya-coreml` | Python / Core ML + ANE | Apache-2.0 | **⚠ 星标/提交比异常** | ★1,530 / **5 commits**。宣称 **4.98 ms P50 / 5.31 ms P95**（M3 Max ANE FP16）、W8 变体 4.88 ms；**ANE bundle 总计 96-token 输入上限**；189/189 验证题通过；FP16 ANE L1024 图过 63/63 但真实 1024-token 请求约 **91.7 ms** | 【仓库/代码】 |
| `ipenywis/laya-ultrafast` | Python | MIT | 可跑（早期） | ★229 / 8 commits / `tests/`、`examples/`、`docs/`。laya 版 jev-ultrafast；宣称 M1 Max 上 5 次 Google Flights 跑 7.5–12.1 s | 【仓库/代码】 |
| `wdobry/laya-playground` | JS | MIT | **疑似空壳** | ★173 / 8 commits / **创建与推送同为 2026-09-20**。单页站 + 两个游戏 + 基准 | 【仓库/代码】 |
| `SiliconLabAI/OpenJev` | TypeScript / Vite | MIT | **疑似空壳** | ★158 但**仅 6 commits**；根目录出现含义不明文件 `sed2Mso9e` | 【仓库/代码】 |

---

## ⑤ 许可证与风险表

### 5.1 可用（Apache-2.0 / MIT，可商业分发）

| 对象 | 许可证 | 是否需标注/署名 |
|---|---|---|
| Laya 三 checkpoint（convaiinnovations） | Apache-2.0 | 需保留 LICENSE/NOTICE |
| Laya 代码 `NandhaKishorM/laya`、PyPI `laya` | Apache-2.0 | 同上 |
| 全部 `mys` / `Weidows` / `receptron` / `techtheist` / `mizchi` / `FluidInference` / `aac6fef` / `litert-community` 移植与量化 | Apache-2.0 | 同上 |
| `ollaya-dev/ollaya` | Apache-2.0 | 同上；**llama.cpp 为 MIT** |
| `jaredpalmer/kev-*`（LoRA） | Apache-2.0 | 基座 Qwen3.5/3.8 亦 Apache-2.0 → 链上干净 |
| `wfzyx/von` | Apache-2.0 | 基座 ModernBERT-large 为 Apache-2.0 |
| `akhilaaa3/Jev-Omni` | Apache-2.0 | 基座 `google/gemma-4-12B-it` 经核实为 **Apache-2.0、非 gated** |
| `com-kotobalabs/open-jev-deberta-v3-large` | Apache-2.0 | 基座 deberta-v3-large 为 MIT |
| `heman10x/rlcd-modernbert-151m` | Apache-2.0 | 但**上位仓库 LICENSE 为 NOASSERTION**，需人工确认 |
| `leobitz/...-jev-berta-base-zeroshot-classifier` | MIT | — |
| `nokia-applied-research/AnyJev`、`featherless-ai/simple-jev`、`razorback16/openjev`、`1Panel-dev/laya-server`、`receptron/laya`、`logan-markewich/jeff`、`TheoLeeCJ/SemIf`、`TianyuCodings/NanoJev`（代码）、`vinnylarouge/jevlike`（代码） | Apache-2.0 / MIT | 代码层面干净 |

### 5.2 雷区（产品要分发，逐条标出）

| 对象 | 问题 | 严重度 |
|---|---|---|
| `openjev/openjev` 及 `-FP8` / `-MLX` / `-MLX-4bit` | **CC-BY-NC-4.0，明确禁止商业使用**。而衍生仓 `openjev/openjev-GGUF` 却标 **apache-2.0** —— 同一模型两个许可证自相矛盾，法律状态不可依赖 | **高** |
| `C-Tianyu/NanoJev`（HF 权重） | **HF 权重仓无 license 字段**（`license: None`）。代码仓是 MIT，但 **权重默认「保留全部权利」**，不能默认可分发 | **高** |
| `argos1111/modernbert-ja-310m-jev` | **CC-BY-SA-4.0**：ShareAlike 具传染性，会约束衍生作品的分发条款 | **中高** |
| `ekzhang/openjev-sglang` | **无任何许可证**（GitHub `license: null`）→ 默认保留全部权利 | **高** |
| `Heman10x-NGU/Verdict-open-jev` | GitHub `LICENSE` 为 **NOASSERTION**（无法识别为标准许可证），需人工读原文 | 中 |
| `MissingPackage/jev-d-0.4b-onnx` | GitHub 仓库 404，许可证无从查证 | 中 |
| 所有 *Jev*/*TypeSafe* 相关命名 | 各类项目普遍带「Not affiliated with TypeSafe / Jev」免责声明（ollaya、openjev、razorback16 等均明示）→ **商标风险低但命名需谨慎** | 低 |

### 5.3 灌水特征（影响可信度，不影响许可证）

- **创建日期高度聚集**：本文涉及的仓库绝大多数创建于 2026-09-16 ~ 09-24。
- **共用同一套脚手架**：`AGENTS.md` 出现在 `NandhaKishorM/laya`、`nokia-applied-research/AnyJev`、`vinnylarouge/jevlike`、`ipenywis/laya-ultrafast`、`TheoLeeCJ/SemIf`、`jaredpalmer/kev` 等互不相关的仓库；`CLAUDE.md` + `.claude` + `skills-lock.json` 同时出现在 `ollaya` 与 `jaredpalmer/kev`。
- **星标与提交量严重脱节**：`mizorewww/laya-mlx` ★6,643 / **6 commits**；`TheoLeeCJ/SemIf` ★4,594 / 25 commits；`NandhaKishorM/laya` ★28,855 / 100 commits（API 单页上限）且仓龄仅 12 天。**不作为「刷星」的定论，但提示不要用星标当作成熟度证据。**
- **单次提交的 README 仓大量存在**：`SiliconLabAI/OpenJev`（6）、`wdobry/laya-playground`（8）、`mizorewww/laya-coreml`（5）。

---

## ⑥ 性能与体积（严格区分宣称 / 实测）

### 6.1 Laya 延迟（全部为「宣称」，来源为模型卡/仓库 README）

| 场景 | 数字 | 来源 |
|---|---|---|
| 单问题单次前向 | **33 ms** | 官方模型卡【官方】 |
| 1 问题，T4 GPU | 39.5 ms（英文）/ 32.8 ms（multilingual） | 官方【官方】 |
| 10 问题批量，T4 GPU | 158.6 ms（英文）/ 72.3 ms（multilingual） | 官方【官方】 |
| `Router(preload=True)` | **32.8 ms（GPU）/ 193–464 ms（CPU）** | 官方【官方】 |
| 4000 token 长文档 | 约 1.7 s（Apple GPU） | 官方【官方】 |

### 6.2 Laya 第三方实测（`Luni/laya-jev-benchmark`，单张 RTX 5090，2026-09-19）

| 基准 | 模型 | 准确率 | ECE | 延迟 p50 |
|---|---|---|---|---|
| PhishNChips 钓鱼 2,000 封 | Laya（原始） | **0.505**（≈随机猜） | 0.441 | 9 ms |
| 同上 | Laya（Platt 校准后） | 0.611 | — | 9 ms |
| 同上 | Jev（published，引用） | 0.626 | 0.154 | 239 ms |
| typed-decisions 400 例 | Laya（**零样本**） | 0.360 | 0.175 | 15.9 ms |
| 同上 | Laya（**在该任务上微调**） | 0.767 | 0.212 | 16.4 ms |
| 同上 | Jev 1.13.0（published） | 0.727 | 0.144 | 710 ms |
| 同上 | 「Teacher 自一致上限」 | 0.735 | — | — |

- 该第三方最重要的发现：**Laya 模型卡拿 83.8% 对比 Jev 的 67.8% 得出「+16.0% 优势」，这两个数字来自两个不同的基准，因此不构成比较。**【第三方文章】
- 吞吐（RTX 5090 fp16，warm）：1 问题 10.7 ms → 10 问题 42.6 ms → 50 问题 246 ms → 100 问题 496 ms；**冷启动 14.9 s**。【第三方文章】
- 原始 Laya 在钓鱼任务上 recall 仅 **0.012**（几乎全判「非钓鱼」）：AUROC 0.678 与 Jev 0.689 接近，说明**排序基本正确、阈值完全错**；Platt 缩放可修，温度缩放**不行**（无偏置项）。【第三方文章】

### 6.3 `sysone-bench`（独立，2026-09-26，1,190 例 / 1,550 题，密封 manifest，单 seed）

| 模型 | 准确率 | 说明 |
|---|---|---|
| Jev 1.13.0 | **0.9065** | 闭源 API（`api.typesafe.ai`） |
| Laya 0.3.11 | 0.6863 | 开源权重，**CPU**，commit 固定为 `55cf4c4e` |
| Qwen2.5-1.5B PCD | 0.6048 | 开源权重，CPU |

Jev 在全部 9 个 suite 上领先 Laya，其中 8 个通过 Holm 校正；triage 例外（+0.057，置换 p=0.0627，作者明确不主张）。**免责：真值由「人工复核 AI 草稿」产生，无第二评审、无 kappa、无仲裁**——作者自行披露。【第三方文章】

### 6.4 `JevBench v1.5.4`（Benchmark Heaven 自建，每系统 1,624 决策：904 公开 + 720 密封；112 个系统中 106 个上榜）

| 排名 | 系统 | Capability | Intelligence | Calibration | $/1k 决策 | 中位延迟 |
|---|---|---|---|---|---|---|
| 1 | **Jev 1.13.0** | **80.0** | 72.0 | 88.0 | $0.032 | 0.62 s |
| 2 | Winnow-12B Q8 | 79.3 | 74.4 | 84.1 | $0.028 | 0.34 s |
| 3 | Cygnet（Gemma-4-12B-it 冻结） | 79.0 | 71.1 | 87.0 | $0.028 | 0.23 s |
| 4 | Surogate Rune 26B-A4B v3 | 79.0 | 69.7 | 88.3 | $0.050 | 0.35 s |
| 5 | **Jev-Omni** | 76.5 | 70.5 | 82.6 | $0.029 | 0.38 s |
| 6 | djev | 76.4 | 72.3 | 80.4 | $0.053 | 0.25 s |
| 7 | **JevK5 v0.3（4B）** | 72.3 | 56.3 | **88.3** | **$0.017** | 0.18 s |
| 8 | Plumb-4B | 71.6 | 55.8 | 87.4 | $0.017 | 0.18 s |
| 9 | Decision 4B v1.2 | 71.1 | 53.7 | 88.6 | $0.017 | 0.18 s |
| 10 | Imajev-4B | 70.8 | 53.5 | 88.1 | $0.017 | 0.23 s |
| 11 | decider-4b v2（Mapika） | 70.7 | 55.8 | 85.6 | $0.015 | — |

榜单定义「Jev-class」= 成本与中位延迟都不超过 Jev 的 2 倍。**Laya 未出现在我抓取到的前 11 名切片中**（我不主张它在完整 106 行里的位置——未抓取）。注意第 5 名之后的 Intelligence 明显掉档（70→56），**Calibration 却普遍更高**（88 vs Jev 88）——即小模型「更诚实但更笨」。【第三方文章】

### 6.5 其它声称（⚠ 仅为宣称，未见第三方复现）

- `wfzyx/von`：**<15 ms 非自回归**（仓库描述）【官方】
- `Heman10x-NGU`：**151M 非自回归、超过 TypeSafe Jev 与 Laya**（项目描述），本次**未找到**支持该断言的第三方评测【官方】
- `ollaya`：`laya:en` **8–10 ms / 5 问题**（RTX 4090）；`winnow:e4b` 89 ms（RTX 4090）、typed-decisions 0.722（Jev 0.738）；`kev:4b` 0.669、`kev:9b` 0.722【官方】
- `mizorewww`：Core ML **4.98 ms P50**（M3 Max ANE FP16）；MLX **13.4 ms**（英文）/ 7.4 ms（multilingual）【官方】

### 6.6 桌面场景体积/内存速查

| 方案 | 首次下载 | 常驻内存 | 依据 |
|---|---|---|---|
| `ollaya` daemon（macOS arm64） | **9.9 MB（mlx 版）/ 14.6 MB（通用版）** 压缩包 | 未公布 | GitHub Releases【仓库/代码】 |
| `ollaya` 桌面 App（macOS arm64 dmg） | 61.8 MB | 未公布 | GitHub Releases【仓库/代码】 |
| `laya:en` 权重（经 ollaya） | ONNX 图 2.7 MB + 权重 803.6 MB | 未公布 | manifest【仓库/代码】 |
| `receptron/laya`（Node） | 约 1.7 GB fp32 | **约 2 GB** + 每批几百 MB | 官方 README【官方】 |
| Laya GGUF ud_q4_k_m | **400.5 MB** | 未公布 | HF【仓库/代码】 |
| laya-multilingual IQ4_XS | **224.6 MB** | 未公布 | HF【仓库/代码】 |
| Laya MLX 单短问题峰值分配 | — | **943.6 MiB**（英文）/ 687.6 MiB（多语） | laya-mlx README【官方】 |
| `kev:4b`（经 ollaya） | **约 9.0 GB**（含 Qwen3.5-4B 基座） | 未公布 | manifest【仓库/代码】 |

---

## ⑦ 推荐给 Switchelp 的三条落地组合（全部为【推测】）

> 前提约束：产品**要分发**、用户**尽量不花钱**、已有本机 Rust 网关 + 插件中心 + 随包工具探测。因此三条路都只选 Apache-2.0/MIT，并优先复用已有的「随包工具 + 网关」机制。

### 组合 A（推荐，工程量最小）：`ollaya serve` 作为 sidecar，Switchelp 只做客户端

- **做法**：在「随包工具清单探测」里加一项 `ollaya`，由插件中心按需拉取 `ollaya-darwin-arm64.tar.zst`（~10–15 MB）/ `ollaya-windows-amd64.zip`；首次使用时经用户同意再拉 `laya:en` 权重（~806 MB）。Switchelp 自己的 Rust 网关把决策请求转发到 `http://127.0.0.1:11435`。
- **理由**：① 许可证最干净（ollaya Apache-2.0，模型各自 Apache-2.0，llama.cpp MIT）；② **与 Switchelp 同为 Tauri 2 栈**（ollaya 桌面端用 `@tauri-apps/cli` 2.11.5），升级/调试心智一致；③ **ollaya 明文承诺「权重要么从作者 HF 仓按 commit + sha256 校验后拉取、要么直接用作者的 GGUF，自身只发 ~3 MB ONNX 图，绝不重托管权重」**——与 Switchelp 已有的 minisign 签名/校验习惯天然契合；④ 一次接入即获得可切换的 `laya` / `kev` / `von` / `winnow` / `jevk5` / `decider` 六个模型池。
- **风险**：ollaya 是 12 天大的项目（100+ commits、★1,001），API 可能快速变动（工作区版本 0.7.5，`resolver = "3"`、`edition 2024`、`rust-version 1.90` —— 对工具链有要求）。建议**钉死版本 + 校验 sha256 + 设计降级路径**。

### 组合 B（最省磁盘 / 无额外进程）：Rust 内嵌 ONNX Runtime，直读 `receptron/laya-onnx` 图

- **做法**：把 `receptron/laya-onnx` 的 `laya.onnx`（**3.6 MB**，Apache-2.0）随包发；权重复用 Laya 官方 803.6 MB fp16 safetensors（或改用 `Weidows` 的 IQ4_XS **224.6 MB** GGUF 走 llama.cpp）。Switchelp 的 Rust 侧直接用 `ort` crate 跑，**不起第二个进程**。
- **理由**：① 体积最小且进程模型最简单；② `receptron/laya` 的 README 明说「运行期不需要 PyTorch/Python」，且输出与 Python 参考实现**对齐到 4 位小数**（【官方】宣称）；③ 图的实际接口需以仓库 `export/` 脚本与 `test/` 为准。
- **风险**：① **需要自己实现问题模板、序列布局、校准与答案拼装**（`receptron/laya` 的 TS 源码是现成参考，MIT 可直接借鉴）；② `receptron/laya` 最后一次推送是 **2026-09-21，已 9 天未动**，npm 仅 3 个版本 —— 若上游停更，维护成本转移到 Switchelp 自己头上。

### 组合 C（最保守 / 先不做模型）：只做「Jev 兼容网关 + BYO-Key」，模型层留给用户

- **做法**：Switchelp 网关暴露 `/v1/systemone`，后端可切到 (a) 用户自带的 TypeSafe Key，(b) 用户自带的任意 `TYPESAFE_BASE_URL`，(c) 免费的 `api.codiv.ai`（razorback16 提供，100M input tokens 免注册卡）。模型文件完全不进安装包。
- **理由**：① **零分发合规风险**（不打包任何权重）；② 先把 Jev 兼容契约和 UI 做对，等生态稳定再选模型；③ `TYPESAFE_BASE_URL` 是 ollaya 与多个开源服务共同采用的**事实标准环境变量**，日后切换到组合 A/B 时上层无需改动。
- **风险**：无法对外宣称「完全本地/免费」。若必须离线可用，则此方案不成立。

**不建议**的组合：① 打包 `openjev/openjev` 任何形态（**CC-BY-NC-4.0 非商用**，且 GGUF 仓许可自相矛盾）；② 打包 `NanoJev` 权重（**无许可证**）；③ 默认走 `kev`（首次下载 ~9 GB）；④ 把 Laya **零样本**当作即插即用的决策引擎（官方自己的数字：0.362，低于 0.461 多数类基线）；⑤ 拿模型卡上的「83.8% vs 67.8%」做营销（已被第三方指出是跨基准拼比）。

**若一定要上 Laya，必须做的三件事**（均来自官方模型卡与第三方实测）：把概率**在自己数据上重新拟合温度**（ECE 0.466→0.081）、对二分类任务用 **Platt 缩放而非温度缩放**（补偏置才能跨过阈值）、**选项数超过 ~50 时抬高 `head_max_len` 或改两级层次选择**（Banking77 上 0.425 vs Jev 0.870）。

---

## ⑧ 来源清单

**官方 / 模型卡**
- https://huggingface.co/convaiinnovations/laya （含完整 limitations 段：零样本 0.362、ECE 0.466→0.081、Banking77 0.425、SST-5 0.372、issue #156 / #185）
- https://huggingface.co/convaiinnovations/laya-multilingual ・ https://huggingface.co/convaiinnovations/laya-typed-decisions
- https://github.com/NandhaKishorM/laya （★28,855 / Apache-2.0 / 80+ tests / laya-ts / sdk / benchmarks）
- https://laya.convaiinnovations.com/ ・ https://pypi.org/pypi/laya/json （0.3.22）
- https://huggingface.co/openjev/openjev （CC-BY-NC-4.0）・ https://huggingface.co/openjev/openjev-GGUF （标 apache-2.0，冲突）
- https://huggingface.co/wfzyx/von ・ https://huggingface.co/jaredpalmer/kev-4b ・ https://huggingface.co/C-Tianyu/NanoJev （无 license）
- https://huggingface.co/heman10x/rlcd-modernbert-151m ・ https://huggingface.co/Luni/laya-jev-benchmark
- https://github.com/ollaya-dev/ollaya/blob/main/README.md ・ https://github.com/ollaya-dev/ollaya/blob/main/Cargo.toml ・ https://github.com/ollaya-dev/ollaya/blob/main/desktop/package.json
- https://github.com/receptron/laya ・ https://registry.npmjs.org/@receptron/laya ・ https://pypi.org/pypi/anyjev/json
- https://github.com/nokia-applied-research/AnyJev ・ https://github.com/featherless-ai/simple-jev ・ https://github.com/razorback16/openjev ・ https://github.com/ekzhang/openjev-sglang ・ https://github.com/1Panel-dev/laya-server
- https://github.com/mizorewww/laya-mlx ・ https://github.com/mizorewww/laya-coreml ・ https://github.com/jaredpalmer/kev ・ https://github.com/wfzyx/von

**第三方评测 / 榜单**
- `sysone-bench`（独立，Jev 0.9065 vs Laya 0.6863）：https://github.com/instax-dutta/sysone-bench
- `JevBench v1.5.4`（Benchmark Heaven）：https://benchmarkheaven.com/jev-models
- `Laya vs Jev`（Luni，RTX 5090 复现）：https://huggingface.co/datasets/Luni/laya-jev-benchmark
- `Jev Decision Index`（Jev + 70 复刻 / 43 基准 / ~120k 决策，本次**未抓取内容**）：https://huggingface.co/spaces/multimodalart/jev-decision-index ・ https://github.com/apolinario/decision-index
- `jev-bench`（166,054 行 / 43 模型 / 22,773 测试记录）：https://huggingface.co/datasets/Praveenrajus/jev-bench
- TypeSafe 官方 eval 面板（厂商自评，非第三方）：https://evals.typesafe.ai/
- 清单来源（**仅作索引，未逐条验证**）：
  - https://github.com/OmniJev/awesome-jev-gallery （★479；含 Benchmark & Leaderboard / Open Source 两节，本文大量条目源自此）
  - https://github.com/kydlikebtc/awesome-jev （★587；自述「运行时与性能未经独立测试」）
  - https://github.com/hejunpenn/awesome-jev （★903）
  - https://github.com/AbdelStark/awesome-typesafe-jev （★548）

**工具可用性记录**
- `/tmp/webtools/search.py` 在本沙箱内失效：Bing 与 DDG 的 HTML 解析均返回 0 条（`# backend=none results=0`），原因疑似结果容器 class 变更/地区重定向；Bing HTML 直连 + 手动解码 `bing.com/ck/a?u=a1<base64>` 跳转可用。
- WebSearch 工具在本供应商下不可用（`Provider API kind openai-compatible does not encode provider-native WebSearch`）。

---

## 附：未能核实事项（明确留白）

1. **Laya 在 `JevBench v1.5.4` 完整 106 行榜单中的具体排名**——仅抓取到前 11 名，Laya 未在其中。是「未参赛」还是「排在第 12 名之后」**未知**。
2. **`Jev Decision Index` 的实际榜单数字**——HF Space 需交互渲染，本次未取到内容。
3. **`Heman10x-NGU`「151M 超过 Jev 与 Laya」的第三方复现**——未找到任何独立评测支持该断言。
4. **`wfzyx/von`「<15 ms」的独立复现与硬件条件**——未找到第三方验证；von 的 1,506 MB 为 fp32，CPU 上能否达 15 ms 高度存疑。
5. **`malevrigns/agent-jev` 与 `aimeigaoshou/agent-jev` 是否为同一项目**——前者 HF 返回 401、GitHub 404；后者 HF 存在（Apache-2.0，0.6B）。命名归属未确认。
6. **`MissingPackage/jev-d-0.4b-onnx`** 的 GitHub 仓库内容与许可证（404）。
7. **`ollaya` 各模型的 CPU 实测延迟与常驻内存**——其 README 只给了 RTX 4090 数字；macOS arm64 上 `laya:en` 的真实 P50 未公布。
8. **`techtheist/laya-onnx` 各量化档位的单文件体积**（仅知全仓 1,690.3 MB）。
9. **各家模型训练数据的具体来源与使用限制**——多个仓库声明使用 `banking77`、`boolq`、`ag_news`、`multi_nli`、`mteb/banking77`、`SetFit/sst5` 等公开数据集，但**未见任何审计报告**；`openjev/openjev` 声称测试集「已按 id 与内容双重检查剔除训练集」，属**自述**，无第三方验证。
10. **GitHub 星标是否被刷**——`laya-mlx` ★6,643 / 6 commits 等比值异常，但**无证据链**，仅作可信度提示。
