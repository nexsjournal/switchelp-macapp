# Jev（TypeSafe AI / System One）官方契约、价格、可用性与中国可达性

**调研日期：** 2026-09-30
**调研范围：** TypeSafe AI 于 2026-09-15 发布的第一个 System One 决策模型 Jev —— 其官方 API 契约、计量与价格、免费层与准入、分发渠道与 SDK、权重与许可证、数据处理条款、中国可达性、官方决策循环建议，以及生态里其它名词（decider / NLI / GLiClass / GliFormer / Laya）与 Jev 的关系。**不含**开源替代品的选型细节（见同目录 `02-open-alternatives-laya.md`）。

**证据档位：**【官方】= TypeSafe 自有站点/文档；【仓库/代码】= 从 GitHub / API 响应 / 仓库文件直接读出；【第三方文章】= 独立测评方或媒体；【推测】= 我的推断。
**方法说明：** 本文所有数字来自公开 HTTP API、官方文档 `.md` 源、GitHub API 与第三方公开测评，**未在本机安装或运行任何模型，未持有 TypeSafe API Key**。`/tmp/webtools/search.py` 的 Bing/DDG 后端在调研后段失效（`backend=none results=0`），WebSearch 工具在此供应商下不可用，因此**「未找到」不等于不存在**。

---

## ① 一句话结论

**官方 Jev 是纯托管的闭源云 API：`POST https://api.typesafe.ai/v1/systemone`，$0.042 / 百万输入 token、输出免费，无公开免费层（仅合同里保留「促销额度」条款）。** 契约极简且稳定（state + questions → answers），三类问题 Choice / Score / Noul，一次请求内全部并行评估，`/v1/models` 只列别名。**官方文档明确「state 与问题会离开本机上传到 TypeSafe」**，且**没有开放权重**；但 MCA 第 2.1 条**明文允许把 API 集成进自研应用供最终用户使用**，同时要求凭据保密 —— 这正好对应 Switchelp 的「用户自带 Key」模型，**不允许**预置共享 Key。中国可达性：`api.typesafe.ai` 解析到 **Cloudflare 非中国节点**，国内直连稳定性无官方保证（**未见任何国内云厂商上架**）。

---

## ② 官方 API 契约

### 2.1 端点与鉴权

| 项 | 值 | 档位 |
|---|---|---|
| 评估端点 | `POST https://api.typesafe.ai/v1/systemone` | 【官方】 |
| 模型列表 | `GET https://api.typesafe.ai/v1/models`（只列别名，不含版本化 ID） | 【官方】 |
| 鉴权 | `Authorization: Bearer <API_KEY>` + `Content-Type: application/json` | 【官方】 |
| 无 Key 访问 | 返回 `403` + `{"detail":{"error_type":"authentication_error",...}}` | 【仓库/代码】（实测） |
| 网络前置 | Cloudflare（`server: cloudflare`，`x-envoy-upstream-service-time`，`x-typesafe-request-id`） | 【仓库/代码】（实测） |
| 控制台 / Playground | `console.typesafe.ai/keys`、`console.typesafe.ai/playground` | 【官方】 |

### 2.2 请求体 schema

| 字段 | 必填 | 类型 | 说明 |
|---|---|---|---|
| `state` | ✅ | `string \| object \| array` | 待判断的状态。字符串、JSON 对象、或文本数组 |
| `model` | ✅ | string | 文档与 SDK 默认用 `jev-latest` |
| `questions` | ✅ | `map<string, Question>` | key 由调用方自取，**不发给模型、不参与推理**，答案按同一 key 返回 |

**Question 公共字段：** `type`（`"noul"`/`"choice"`/`"score"`）、`instructions`（`string | object | array`，**必填**）。

**各类型专属字段：**

| 类型 | 字段 | 约束 |
|---|---|---|
| Noul | `criteria`（可选对象，内含 `true` / `false` 各自的描述） | — |
| Choice | `criteria`（必填，`map<string, string|object|array|null>`） | **最多 255 个选项** |
| Score | `criteria`（必填，**有序数组** `array<string|object|array>`） | **至少 2 级、最多 10 级** |

### 2.3 响应体 schema（字段名以官方为准）

```
{ "model": "jev-1.13.0", "answers": {...}, "usage": { "input_tokens": 392, "output_tokens": 65 } }
```

| 类型 | 返回字段 |
|---|---|
| `noul` | `type`, `noul`（number 0–1）—— **Noul 不带 `confidence`** |
| `choice` | `type`, `choice`（string）, `probabilities`（map<string,number>）, `confidence`（0–1） |
| `score` | `type`, `score`（number）, `legend`（map<string,string>，索引→级别文字）, `probabilities`（map<string,number>）, `confidence`（0–1） |

官方 Quickstart 的真实响应示例：`"frustration"` 返回 `score: 1.0, confidence: 1.0, legend: {"0":"Calm...","1":"Frustrated but civil","2":"Very angry..."}`。Choice 的 `probabilities` 之和不恒为 1（官方示例 `technical 0.85 / sales 0.0 / billing 0.15`）。**`confidence` 是由概率分布形状推导的统计量，不是正确率**；官方给出三选项演示公式 `(3×最大概率 − 1) / 2`。

### 2.4 批量 / 并行 / 超时 / 限流 / 错误码

- **并行语义：** Jev **只读取 `state` 一次**，所有问题对同一 state 并行求值，**一次往返返回全部答案**。问题之间**互相不可见** —— 若第 2 题依赖第 1 题答案，必须拆成两次调用或由代码串接。【官方】
- **上下文预算：** 每请求 64k tokens；**`state` + 单个最长问题 ≤ 32k tokens**（`state` + 全部问题合计 ≤ 64k）。超出返回 `422`。【官方】
- **限流（Model 页，2026-09-30 读取）：** **100K tokens/秒 且 40 请求/秒**，任一超限返回 `429`。官方**明确警告「限流正在动态调整，可能随时变动」**，更高配额走 custom / enterprise（须联系官方销售邮箱；**原文邮箱在本仓库已脱敏**）。【官方】
- **限流数值存在版本差异：** 台湾 ai.com.tw 2026-09-20 文章写「250,000 tokens/秒、1,200 requests/分钟」，与当前官方 Model 页不一致。**以官方为准，并把「文档会变」当作已知风险。**【第三方文章】+ 冲突标注
- **错误码：** `401` 缺/错 Key；`422` 请求体校验失败；`429` 超限；`529` 过载。`429`/`529` 建议指数退避，SDK 默认重试并遵循 `retry-after`。**官方文档未给出超时值。**【官方】
- **输出格式：** 无流式（no streaming）、无 output-token 上限（Vercel 网关模型卡原文：“No streaming or output-token limit”）。【仓库/代码】

**来源：** https://docs.typesafe.ai/api ・ https://docs.typesafe.ai/models ・ https://docs.typesafe.ai/introduction/quickstart ・ https://docs.typesafe.ai/confidence ・ https://docs.typesafe.ai/primitives ・ https://docs.typesafe.ai/primitives/choice ・ https://docs.typesafe.ai/primitives/score ・ https://docs.typesafe.ai/primitives/noul

---

## ③ 定价、计量与免费额度

| 项 | 值 | 档位 |
|---|---|---|
| 计量方式 | **按输入 token** 计费；输出 token 免费（“FREE (too cheap to meter)”） | 【官方】 |
| 单价 | **$42 / Btok = $0.042 / Mtok = $0.000000042 / token** | 【官方】 |
| 输出单价 | $0 | 【官方】 |
| 单次决策实算 | 第三方按官方口径推算：**平均 950 输入 token / 决策 → $0.0399 / 1,000 决策**（534 条 v1.2 决策集） | 【第三方文章】 |
| 免费层 | **未找到任何公开免费层 / 免费试用额度**。`GET /v1/models` 无 Key 直接 403，无匿名配额 | 【官方】（确认为无） |
| 促销额度 | MCA §8.2 载明 “TypeSafe **may, but has no obligation to**, issue Promotional Credits”；促销额度先于付费额度消耗；**禁止为反复领取促销额度而开多个账号** | 【官方】 |
| 企业 / 更高配额 | 联系官方销售邮箱（custom / enterprise 计划；**原文邮箱在本仓库已脱敏**） | 【官方】 |
| 价格页 | `typesafe.ai/pricing` **不存在**（返回 Framer 的 Page Not Found）；官网首页直接写 “$42 Per Billion input tokens” 与 “238x Lower input price than Claude Fable 5.1” | 【官方】 |

**准入状态（重要）：** 官方口径一直是 “early access”【官方】；但中文第三方完整链路工具记录：**2026-09-21 邀请制已取消**，站点不再有「投申请 / 等回执 / 等人工审批」，注册后即收到 magic-link 邮件自助建 Key（判据：`POST /login` 直接回 `x-action-redirect: /login?sent=true`，`?waitlist=` 参数失效）【第三方文章】。另一中文社区清单则称 waitlist「反馈基本当天过」【第三方文章】。→ **结论：截至 2026-09-30 应为自助注册，无排队门槛，但此点未经官方页面直接确认。**

**来源：** https://docs.typesafe.ai/models ・ https://typesafe.ai/ ・ https://typesafe.ai/legal/mca ・ https://github.com/2951461586/Jev-Register-Tool（第三方）

---

## ④ 分发渠道与 SDK

| 渠道 | 状态 | 价格是否一致 | 档位 |
|---|---|---|---|
| **TypeSafe 官方 API** | 可用，唯一一线来源 | 基准价 $0.042/Mtok | 【官方】 |
| **Vercel AI Gateway** | **已上架** `typesafe-ai/jev`（`type: "evaluation"`，released 2026-09-15，ctx 32,000）。Vercel 官方声明「adds zero markup to provider token prices」→ 等价 $0.042/Mtok、走 Vercel 额度计费 | 一致（零加价） | 【仓库/代码】+【官方】 |
| **Vercel AI SDK 官方 provider** | `@ai-sdk/typesafe-ai` v3.0.11，**Apache-2.0**，在 `vercel/ai` 主仓 `packages/typesafe-ai/`。导出 `typeSafeAi.evaluationModel('jev-latest')`、`createTypeSafeAi()`；用 `experimental_evaluate()`；`type:'boolean'` 映射到 Noul；默认 baseURL `https://api.typesafe.ai/v1`，Key 取自 `TYPESAFE_AI_API_KEY` | — | 【仓库/代码】 |
| **OpenRouter** | **仅列名、实际不可调用**：`typesafe/jev-router` 存在（created 1790363560），但 `pricing.prompt = "-1"`、`pricing.completion = "-1"`，且 `/endpoints` 返回 **`"endpoints": []`** → 无任何可用 provider 端点 | 不适用 | 【仓库/代码】 |
| **Cloudflare AI Gateway** | **未找到 TypeSafe / Jev provider**。官方 providers 列表含 anthropic / openai / openrouter / groq … 共 24 个，**无 typesafe**。Jev 端点为 `/v1/systemone` 而非 `/v1/chat/completions`，非 OpenAI 兼容，无法走兼容路径直接接入 | 未找到 | 【官方】（确认为无） |
| **Netlify** | 未找到任何 Jev 支持证据 | 未找到 | — |
| **Python SDK** | `pip install typesafe-sdk`，要求 Python ≥ 3.10；异步 `AsyncTypeSafeClient` / 同步 `TypeSafeClient`；`base_url` 可覆写（**这是本机替代实现得以兼容的关键**） | — | 【官方】 |
| **JS/TS SDK** | `@typesafe-ai/sdk`（`TypeSafeClient`、`RetryPolicy`、类型化 `ChoiceResponse`/`ScoreResponse`/`NoulResponse`） | — | 【官方】 |
| **LangChain 官方 partner 包** | `langchain-ai/langchain/libs/partners/typesafe/`，包名 `langchain-typesafe` v0.0.1a3，**MIT**，提供 `langchain_typesafe.classifier`；配套官方博客 langchain.com/blog/building-a-harness-with-jev | — | 【仓库/代码】 |
| **其它一线集成** | DSPy `dspy/clients/typesafe.py`；LiteLLM guardrail hook；MLflow AI Gateway 常量 `TYPESAFE_API_BASE_URL`；Goose `crates/goose-providers/src/typesafe.rs`；bytedance/deer-flow 扩展 | — | 【仓库/代码】 |
| **官方 Agent Skill** | `npx skills add typesafe-ai/skills --skill typesafe-ai`（文档 /agent-skill）。**只是开发指引，不安装模型**，推理仍走云端 | — | 【官方】 |

**偏第三方但免费的 Jev 入口（对「尽量不花钱」直接相关）：** `classifier.dev` 是第三方分类服务，其 **fast tier 就是 Jev**（把上千条打包进一个请求）；其 benchmark 页自称 “Calling Jev yourself costs about $0.005 / $0.004 per thousand and needs a TypeSafe key; **this service costs nothing and needs none**”（2026-09-17/18 实测）。【第三方文章】⚠ 属第三方转售/转包，稳定性与条款均无保证。

**来源：** `https://ai-gateway.vercel.sh/v1/models` ・ `https://ai-gateway.vercel.sh/v1/models/typesafe-ai/jev` ・ https://openrouter.ai/api/v1/models ・ https://developers.cloudflare.com/ai-gateway/usage/providers/ ・ https://vercel.com/docs/ai-gateway ・ https://ai-sdk.dev/providers/ai-sdk-providers/typesafe-ai ・ https://docs.typesafe.ai/sdk ・ https://classifier.dev/benchmark

---

## ⑤ 权重、许可证与数据处理条款

| 问题 | 事实 | 档位 |
|---|---|---|
| 开放权重？ | **没有。** 官方从未发布权重/技术报告；Wikipedia 记「专有（proprietary）、未公开权重」，外部观察者猜测「可能基于某个开源 LLM 构建」。台湾文章亦确认「尚未公开 Jev 模型权重或本地部署方案」 | 【第三方文章】 |
| 架构披露 | 公司称基于 **Transformer**、纯**合成数据**训练，方法名 **RLCD（Reinforcement Learning for Calibrated Decisions）**；精确架构未公开 | 【第三方文章】 |
| 能否装进桌面应用？ | **可以，且是明文授权。** MCA §2.1 许可「include the API into one or more software applications developed and operated by Customer for the benefit of Customer's end users」（Customer Applications） | 【官方】 |
| Key 怎么放？ | **必须用户自带、不得共享。** MCA §2.4：“Customer will ensure that each Customer User **keeps the Access Credentials confidential and does not share them with anyone else**”。→ 预置一把共享 Key 分发给全体用户属违约风险 | 【官方】 |
| 明令禁止 | (a) 把 Services 作为独立服务转售/分发；(b) 用 Output 做**模型蒸馏 / 训练模仿模型**，或开发**相似或竞争产品**；(c) 反编译/逆向；(d) 修改或创作衍生作品；(f) 绕过访问限制；(j) 超出 Usage Limits；**§8.2(b) 禁止为促销额度多开账号** | 【官方】 |
| 训练数据 | 「TypeSafe will not include Customer Data in a dataset used to train ... any AI/ML models **without Customer's prior consent**」；Model 页亦写 “Jev is not trained on customer requests or responses” | 【官方】 |
| 但 Telemetry 无限制 | TypeSafe 可**永久**处理 Telemetry —— 定义含「technical logs, hashes, summary statistics and classifications, metrics, and learnings」，可「without restriction」用于改进服务 | 【官方】 |
| state 是否上传？ | **是。** 推理在云端完成，`state` 与问题必然离开本机；官方 “Data handling” 段与 ZDR 只在**企业版**提供（“zero data retention (ZDR) for enterprise customers”，销售联系） | 【官方】 |
| 留存期 | DPA Schedule I 只写「retained for as long as necessary ... in compliance with applicable laws」，**无具体天数** | 【官方】 |
| 法律文本 | DPA / MCA / Privacy Policy 三份，`typesafe.ai/legal/*`。MCА 管辖法为**加州 + 美国法**；含标准美国出口管制条款（不得为受制裁国国民/居民） | 【官方】 |
| 语言能力自述 | Model 页原文：“English is the primary training language and where accuracy is currently best. Other languages, **including CJK scripts, are handled but not equally well**; test on your own content” | 【官方】 |
| 官方自认短板 | `/model-jaggedness/jev-1.13`（最后复核 2026-09-17）列 9 类失败模式：字面理解、**不会算数/计数**、日期比较、多跳间接、大 state 噪声、对抗内容、矛盾指令、常识不变量、生成任务 —— 明确「不要把 score 用于数值插值」 | 【官方】 |

**来源：** https://typesafe.ai/legal/mca ・ https://typesafe.ai/legal/data-processing ・ https://docs.typesafe.ai/legal ・ https://docs.typesafe.ai/models ・ https://docs.typesafe.ai/model-jaggedness/jev-1.13 ・ https://en.wikipedia.org/wiki/Jev_(AI_model)

---

## ⑥ 官方对「决策循环」的建议

- **核心模式：让代码掌权、给模型窄而结构化的判断。** `/concepts/how-to-build-with-system-one` 的主题就是 “keeping code in control and giving System One narrow, structured decisions”。【官方】
- **置信度门控路由（confidence-gated routing）：** 官方原文 “Use confidence as a second axis. The answer tells you **what**; confidence tells you whether to **act**.” 即 accept / reject / escalate 三段式。【官方】
- **阈值数值：官方不提供推荐阈值。** 未在 patterns 或 confidence 文档中找到任何具体数字（如 0.8 / 0.9）。阈值需用户按动作风险自定。【官方】（确认为未给）
- **校准（calibration）：** 概率是“trained for calibrated decisions”，用 RLCD 优化；“Calibration is measured across **groups** of predictions; it **does not guarantee that an individual answer is correct**”。【官方】
- **三种配套模式：** Speculative fan-out（一次塞很多问题含假设性问题）、Confidence-gated routing、Composite scoring（把复杂判断拆成原子分，在代码里加权 —— 官方明确「权重由你控制」）、Intent routing（分类后路由到确定性逻辑 / 专用 LLM / 人）。【官方】
- **Cookbook 里可复用的官方自报收益：** Parallel questions —— 13 个 GDPR 问题批成一次调用，**便宜 12.2×、快 10.0×，答案不变**；Re-ranking —— 40 条 CLERC 法律查询，top-1 从 5% → 18%、top-10 从 38% → 62%；Classification using confidence —— SEC 年报 75 个行业组，用 confidence 决定报细分还是上级大类。【官方】
- **自一致性 cookbook：** `consistency_noul_cookbook` / `consistency_choice_cookbook` 演示「把不确定概率路由到人工复核」并保留原始数值可观测。【官方】

**来源：** https://docs.typesafe.ai/concepts/how-to-build-with-system-one ・ https://docs.typesafe.ai/patterns ・ https://docs.typesafe.ai/patterns/confidence-routing ・ https://docs.typesafe.ai/patterns/fan-out ・ https://docs.typesafe.ai/confidence ・ https://docs.typesafe.ai/cookbooks/parallel_questions

---

## ⑦ 生态名词辨析（decider / NLI / GLiClass / GliFormer / Laya 与 TypeSafe 的关系）

**结论：这几个都不是 TypeSafe 官方资产，全部是第三方独立模型或项目，靠「TypeSafe System One 兼容 API」借道。** 它们被并列的场合是本地运行时 `ollaya` 的模型清单（`ollaya-dev/ollaya` 自述 “pull and serve Laya, decider, NLI and GLiClass behind a TypeSafe-compatible API”）。【仓库/代码】

| 名词 | 是什么 | 归属 | 与 Jev 的关系 | 许可证 |
|---|---|---|---|---|
| **Laya** | 非自回归 System 1 决策引擎，单次前向、100+ 语言，带 router 选 checkpoint | **Convai Innovations**（`NandhaKishorM/laya`，`convaiinnovations/laya`） | **独立同类模型**。本地运行时 `ollaya` / `rsdecider` 提供 **wire-identical 的 `POST /v1/systemone`**，官方 SDK 改 `TYPESAFE_BASE_URL` 即可切换 | Apache-2.0 |
| **decider** | Qwen3.5 上的 decoder 决策模型，从选项字母 logits 读答案，单次前向 | **Mapika** | 独立同类模型（`decider:4b` 0.680 typed-decisions） | Apache-2.0 |
| **NLI** | 零样本自然语言推理分类器：每个选项变成一个 entailment 假设 | **Moritz Laurer** | 独立零样本分类器，非决策模型专设计；ollaya 称其「our tests 中 typed decisions 上最准的 encoder 模型」 | `nli:modernbert-large` Apache-2.0 / `nli:deberta-v3-large` MIT |
| **GLiClass** | 指令跟随零样本分类器，一次前向同时给所有选项打分 | **Knowledgator** | 独立零样本分类器 | Apache-2.0 |
| **GliFormer** | Generalist Multitask Transformer Encoders（400M） | **Knowledgator** | **`jeff` 的骨干** —— `logan-markewich/jeff`（MIT，★273）自称 “self-hosted drop-in replacement for TypeSafe's **jev**”，官方 `typesafe-sdk` 改 `TYPESAFE_BASE_URL` 即可用 | GliFormer 与 jeff 均为宽松许可（jeff MIT） |
| **kev** | Qwen3.5 上的 LoRA + pointer head | **Jared Palmer** | 独立同类模型 | Apache-2.0 |
| **jevk5** | Qwen3.5-4B fine-tune，以 GGUF 发布 | alibiserikbay | 独立同类模型（名字仿 Jev，但与 TypeSafe 无关） | Apache-2.0 |
| **winnow** | Gemma 4 上的决策模型，GGUF | EldanRing | 独立同类模型 | Apache-2.0 |
| **reflex** | Qwen3.5 上的 “Jev / System One re-creation” | kshetrajna12（MIT，★160） | **公开自称复刻**，`POST /v1/systemone` | MIT |

**⚠ 术语澄清：** 官方文档里**没有** “decider” 这个词（官方只有 Noul / Choice / Score 三个 primitive）。“decider” 是社区/第三方项目名。**TypeSafe 亦明确不背书这些项目**：`ollaya` README 自述 “Ollaya is an independent project. It is not affiliated with or endorsed by Ollama or **TypeSafe**.”【仓库/代码】

**来源：** https://github.com/ollaya-dev/ollaya ・ https://github.com/NandhaKishorM/laya ・ https://github.com/logan-markewich/jeff ・ https://github.com/Knowledgator/GLiFormer ・ https://github.com/kshetrajna12/reflex ・ https://huggingface.co/convaiinnovations/laya

---

## ⑧ 官方与第三方公布的实测数字

**官方自报（均为官网/博客口径，非第三方复现）：**【官方】

| 指标 | 数值 | 语境 |
|---|---|---|
| 端到端延迟 | **70 ms – 500 ms** | 官方博客；对比「既有 frontier LLM 3–329 秒」 |
| 速度提升 | **40×–200×**（首页另有 **193.6×**） | “two orders of magnitude faster” |
| 成本降低 | **40×–400×**（首页另有 **444.6×**） | 首页示例：TypeSafe $0.000081 / 0.114s vs LLM $0.013880 / 8.566s |
| 上下文 | 64k / 请求；state + 最长问题 32k | Model 页 |

**⚠ 官方已自我保留：** Wikipedia 转述 TechStock² 指出「445× 成本声称**仍为自测**」，且官方主动说明增益可能位于高端、承认可能存在偏差。【第三方文章】

**第三方独立测评：**

| 基准 | 内容 | Jev 数字 | 档位 |
|---|---|---|---|
| **JevBench v1.3.0**（Benchmark Heaven 自建，非 TypeSafe 关联；534 决策/系统，48 行上榜） | 分数 = chance-corrected Intelligence / Calibration / Speed / Cost 各 25% 的几何平均；**Cost 轴单位为「$ / 1,000 决策」而非 per-token** | **Jev 1.13.0 = 74.4（第 1）**；其后 SemIf 73.1、djev 73.0、Winnow-12B Q8 71.2、reflex 4B 70.3。官方口径换算 **950 输入 token/决策 → $0.0399 / 1,000 决策** | 【第三方文章】 |
| **JevBench v1.4.2 / 1.4.2.1**（更严：20% 来自 308 条新密封决策） | 95 systems / 91 ranked（v1.4.2.2） | **Jev 1.13.0 = 63.29（第 4）**；前 3 为 Imajev-4B 67.37、Plumb-4B 65.84、decider-4b v2 64.13。Jev “out-reasons decider-4b v2（Intelligence 53.1 vs 49.4）且校准更好；decider-4b v2 在 speed/cost 领先” | 【第三方文章】 |
| **reflex 官方 JevBench v1.2 成绩** | 由基准作者在 **H100** 上跑，534 决策含留出项 | reflex 4B **第 5 / 36，71.7**；Jev hard-tier 74.1% | 【仓库/代码】 |
| **Laya BENCHMARKS.md**（第三方，明示「Jev 数字为公开转述，从未在此实测 —— 无 TypeSafe API 访问」） | typed-decisions 2,000 决策 / AG News / DAIR Emotion / ECE / T4 p50 延迟 | **Laya 0.766 vs Jev 0.727**；AG News 0.953 vs 0.910；DAIR Emotion 0.600 vs 0.480；**温度拟合后 ECE 0.081 vs 0.246**；**p50（1 题，T4）32.8 ms vs 236–276 ms** | 【仓库/代码】 |
| **classifier.dev**（实测 2026-09-17/18，400 items/集，走公开 API） | 真实计费口径 | **jev-1.13: AG News 87.7% / emotion 60.5% / 2 ms/item（摊销）/ $0.005 per 1k**；「Jev + 对 <0.7 置信度重问」的 smart tier：90.0% / 62.7% | 【第三方文章】 |
| **superlinked/sie typed-decisions**（预注册，2026-09-25，160 CVE 测试记录） | 独立决策模型横向对比；**该榜未收录 Jev 行** | 可参照量级：GLiClass-large 28 ms/次；GLiFormer-large 65 ms；Qwen3-4B-Instruct 2,676 ms | 【仓库/代码】 |

**中文社区转述（热度与体感，非基准）：**【第三方文章】
- X @NFT_Chen：500 条电商工单实测 —— Jev「83 秒 / $0.01」清完，对比 DeepSeek V4.1 Flash 只做完 173 条 / $0.06
- X @FinanceYF5：50 局地铁跑酷 < $0.01；40 秒拆 724 条广告仅 $0.09
- `awesome-jev-cn`（社区整理，非官方）：官宣推文 7.3 万赞、HN 1920 分 / 504 评论、一周内新建 Jev 相关仓库 3200+
- ⚠ 该仓库自带免责声明：「与 TypeSafe AI 官方无关」；并特别提示**官方没有发行任何代币**，任何 “$JEV / Jev币” 均与官方无关

**来源：** https://typesafe.ai/blog/introducing-system-one-models-and-jev ・ https://typesafe.ai/ ・ https://github.com/fstandhartinger/jevbench ・ https://jevbench.dev ・ https://github.com/NandhaKishorM/laya/blob/main/BENCHMARKS.md ・ https://github.com/superlinked/sie/tree/main/examples/typed-decisions ・ https://github.com/CodeAlex52/awesome-jev-cn

---

## ⑨ 中国可达性

**先说结论：结构性证据表明是「Cloudflare 非中国节点 + 无国内上架」，直连稳定性不可依赖；但我无法从大陆网络实测，故整体判为推测。**

| 事实 | 档位 |
|---|---|
| `api.typesafe.ai` → **104.18.24.46 / 104.18.25.46**，属 **Cloudflare** 网段（非中国网络）；响应头 `server: cloudflare` | 【仓库/代码】（DNS + TLS 实测） |
| 站点（含 `console.typesafe.ai`）启用了 **Cloudflare Bot Management**：沙箱出口 IP 直接被 403 “Sorry, you have been blocked / unable to access typesafe.ai”（Ray ID 给出）→ 说明按 IP 信誉拦截是常态能力 | 【仓库/代码】（实测） |
| **未找到任何国内云厂商/聚合平台上架 Jev**：硅基流动 `api.siliconflow.cn/v1/models` 需鉴权（未能取得清单，未能证实或证伪）；GitHub 检索 “jev 中转 / typesafe 中转 / jev 镜像站” **均返回空** | 【仓库/代码】（未找到） |
| **无官方中国区（无 CN 域名、无人民币计价、无本地节点）**；MCA 只有标准美国出口管制条款 | 【官方】 |
| 中文社区反应：`awesome-jev-cn` 有专门的「小红书 / B站 / X中文KOL」栏目，说明中文圈有真实活跃需求；同时存在多个中文**批量注册工具**仓库（`2951461586/Jev-Register-Tool`、`Futureppo/typesafe_register`），其中前者把「**当批站点丢包率**」列为影响跑批成功率的变量 —— 这是国内访问链路不稳的间接证据 | 【第三方文章】 |
| 台湾第三方文章（ai.com.tw，2026-09-20）通篇讨论导入可行性，**未提到需要代理或存在封锁** —— 但那是台湾网络环境，不能外推至大陆 | 【第三方文章】 |

**推测（明确标注）：**【推测】
1. 大陆直连 `api.typesafe.ai` 大概率**能通但抖动大**（Cloudflare 免费/Pro 级线路在大陆的典型表现），延迟高于官方宣称的 70–500 ms，且 `429/529` 与 TLS 重置概率上升。
2. **桌面应用不能依赖这条链路做关键路径**。一旦把 Jev 做成 Switchelp 的必需依赖，大陆用户会直接撞墙。
3. `console.typesafe.ai` 的 Cloudflare Bot Management 对**机房 IP / VPN 出口 IP 误杀**风险高（本沙箱即被拦），会影响用户在 App 内引导注册的体验。
4. **若必须用官方 Jev，走 Vercel AI Gateway 与直连相比不改善大陆可达性**（Vercel 同样非国内节点），但能统一 Key 管理、加预算上限与请求日志 —— 这是运营收益，不是网络收益。

---

## ⑩ 对 Switchelp 的可用性判断（推测）

> 本节全部为【推测】，且刻意区分「事实支撑的推断」与「纯判断」。

1. **契约侧：几乎零摩擦。** Jev 的 `state + questions → answers` 与 Switchelp 已有的本机网关（127.0.0.1:18765）转发布局天然契合：网关加一路不是 OpenAI 兼容的 `/v1/systemone` 透传即可。**但要注意它不是 chat completions 协议**，不能复用现有的 OpenAI 兼容转发代码路径，得单独一条路由。【推测，基于【官方】契约】
2. **Key 归属必须做成 BYO-Key。** MCA §2.4 要求凭据保密不共享 → **Switchelp 不能内置一把公共 Key**。这与产品既有「管理用户的多个供应商与 API Key」定位完全一致：把 TypeSafe 当成又一个 provider 条目，让用户填自己的 Key。**这条不是「可以选择」，而是合规必需。**【推测，基于【官方】MCA】
3. **钱：官方侧没有免费层。** 按 JevBench 的官方口径实算 **$0.0399 / 1,000 决策**（950 输入 token/决策 @ $0.042/Mtok）—— 对个人用户近乎可忽略，对「尽量不花钱」的人来说**唯一真正的零成本路径是本地决策模型**（`ollaya` / Laya / reflex，详见 `02-open-alternatives-laya.md`）。**【推测】+【官方/第三方】数字**
4. **不该做成「假开关」的反面案例。** 团队硬规则是不做核心层没实现的控件。若做 Jev 集成，**必须真跑通 `/v1/systemone` 并在 UI 显示真实返回的 probabilities / confidence**，不能只放一个「启用 Jev 加速」的漂亮开关。Jev 的返回结构正好适合做成「答案 + 概率条 + 置信度」的可视化，**且完全不需要读用户会话内容**（state 由调用方显式构造）。【推测】
5. **最有产品价值的落点（按确定性排序）：**（a）把 TypeSafe 作为第 N 个 provider + 一条 `/v1/systemone` 透传路由；（b）用量统计里单列 Jev 的「决策数 / 输入 token / 估算 $」（Jev 输出免费，与传统 provider 的计费模型不同，**必须单独建模**）；（c）置信度门控演示 —— 官方 confidence-gated routing 是现成的、无需自创的叙事。【推测】
6. **风险清单（必须在产品层面告知用户）：** ①state 会被上传到 TypeSafe，**不可用于机密代码/数据**，ZDR 仅企业版；②限流**随时可变**（官方明示动态调整），不能当作 SLA；③early access，价格/限流/版本/条款都可能变；④英文最好、**CJK「handled but not equally well」**，中文场景须自测；⑤`jev-latest` 会漂移，官方建议**生产固定版本 ID** 并记录响应里的 `model` 字段；⑥不得用 Output 蒸馏或被视作开发竞品。【【官方】事实 +【推测】产品含义】

---

## ⑪ 未能核实事项（明确留白）

1. **大陆直连的实际可用性与延迟** —— 无法从大陆网络实测；仅有 Cloudflare 非中国节点这一结构性证据。**需要一位大陆用户跑 `curl -w '%{time_total}' https://api.typesafe.ai/v1/models` 才能定论。**
2. **官方是否真的取消了 waitlist** —— 只有中文第三方工具仓库的 2026-09-21 记录，未能用官方页面确认（`console.typesafe.ai` 被 Cloudflare 拦截，无法读取注册页）。
3. **是否存在未公开的免费试用额度 / 新账号赠送额度** —— 文档与模型页均无；MCA 只提到可选的 Promotional Credits。**未能读到 console 内部，无法证伪。**
4. **Noul 为何不带 `confidence`** —— 官方明确「Noul answers don't carry one」，但未解释原因；也没有 Noul 的阈值建议。
5. **官方推荐的具体置信度阈值** —— 未找到任何数字，patterns 与 confidence 文档只讲方法不给值。
6. **Jev 与 Laya/reflex 等的同条件正面对比** —— 所有对比都不同源：Laya 的对比是转述、JevBench 的 Laya 行与 Jev 行在不同版本榜、classifier.dev 只比了自己与 Jev。**没有任何一份由中立第三方在同协议同题集上同时跑 Jev 与 Laya 的公开数据。**
7. **Cloudflare AI Gateway 是否支持以自定义 provider 形式透传 `/v1/systemone`** —— 未找到专用 provider，也未验证通用/自定义 provider 路径的可能性。
8. **Netlify、Bedrock、Hugging Face Inference 等渠道是否支持 Jev** —— 未找到任何证据，未逐一穷举。
9. **`typesafe/jev-router`（OpenRouter 条目）到底是什么** —— 描述为「挑选最佳模型与推理强度」且 modality 写 `text+image+file+audio+video->text`、tokenizer 为 `Router`，与 Jev 文本-only 决策模型明显不符；`endpoints` 为空，**无法实测，其性质存疑**。（另注：OpenRouter 描述中出现的 `openrouter.ai/~typesafe/jev-latest` 链接未验证。）
10. **TypeSafe 的具体模型版本历史与发布日期** —— `jev-1.13.0` 是当前版本，但 1.0–1.12 是否存在、何时发布，未找到公开信息。
11. **`siliconflow` / 火山 / 智谱 等国内平台的完整模型清单** —— `api.siliconflow.cn/v1/models` 需鉴权，未取得清单；**「未找到」不等于「未上架」**。
12. **官方对 Vercel AI Gateway 上架的官方声明** —— Vercel 侧模型卡与 AI SDK provider 均确认存在，但未在 TypeSafe 官方文档中找到对 Vercel 渠道的提及。
