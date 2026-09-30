# Jev / 决策模型在「分类 · 打分排序 · 内容筛选 · 安全护栏 · 用量统计」上的用例调研

**调研日期**：2026-09-30
**调研对象**：TypeSafe Jev（System One：输入 state + 类型化问题 Choice / Score / Noul → 输出带校准概率的决策，不生成文本）及其开源替代 Laya
**范围声明**：
- 只读调研，未 clone、未执行任何仓库脚本、未安装依赖。
- 所有 GitHub 元数据（star / 提交数 / 最近提交 / 许可证 / 测试文件数）均为 **2026-09-30 通过 GitHub REST API 读取**的快照；star 数取整。
- 项目自述的性能、成本、准确率数字一律标注「项目自述」，未独立复现。
- 档位标签：【官方】= TypeSafe / Laya 官方文档或 README；【仓库/代码】= 项目仓库 README、代码或 benchmark 产物；【第三方文章】= 非项目方的公开文章/榜单；【推测】= 本报告基于前两者的推断。
- 本调研**未**验证任何数字的第三方复现；凡查不到的一律写「未找到」。

---

## ① 一句话结论

决策模型在**本产品的落地点只有两类是真正划算的**：(a) 对**短文本块**做批量相关性/风险判断（feed 条目筛选、错误元数据分类、内容审核），成本量级 $0.00002–$0.001/次、延迟 5–260 ms，且**规则先行、模型只处理灰区**是该领域所有可跑项目的共同形态；(b) 用**开源 Laya 在本地**替代在线 Jev 以把边际成本压到 0（Apache-2.0，M5 Pro 上 32 ms / 2.1 GiB）。而「用决策模型做用量归因、模型路由省钱、挡 prompt injection」这三件事**已有明确的反面实测证据**，应劝退或降级为规则实现。

**必须先纠正的三个前提错误**：
1. **`16sulphur/laya-prompt-guard` 不存在**（`gh api` 返回 404；用户 16SULPHUR 名下无此仓库）【仓库/代码】。
2. **`kyotofin/tax-doc-classifier` 不是「261 种表单 100% 准确」**：仓库简介这么写，但 README 的实测表显示 261 表单语料上严格错误率 **5.05%（38/753，全部为低于 0.95 置信度而被拒答，非答错）**；「0 错」只成立于 314 页已填表语料（仅 15 种表单）【仓库/代码】。
3. **`angel291592/Intent-Router` 并未使用决策模型**：仓库 97 个路径中无任何 Jev/Laya/backend 文件，其 Jev/Laya 后端是 README 中标注「planned」的 L2 付费层，默认 L0 是无依赖的 prompt-only【仓库/代码】。

**生态成熟度总警告**：本次触及项目绝大多数创建于 2026-09-15 之后（本文写作时存在 ≤15 天），star 数（Laya 28,868 / awesome-jev 2,000）与该年龄明显不相称；`yibie/awesome-jev` 自己就在 README 里警告「同日批量提交、共享脚手架、提交历史单薄，能满足收录规则但不代表可用」【仓库/代码】。**不要用 star 数当成熟度**。

---

## ② 按用途分组的用例

### 2.1 内容相关性排序与筛选

| 项目 | 决策内容 / 问题类型 | 输入 state | 输出怎么用 | 实测收益 | 可跑 | 成熟度评级与依据 |
|---|---|---|---|---|---|---|
| **superagents-lab/jev-search**（Search1API 官方 demo，域名 jev.s1.dev）【仓库/代码】 | 两步：先用 Noul/Choice 判断意图→选源、时间范围、查询词；再对**每条结果的标题+摘要**打 Noul 相关度 | 第 1 步：用户自然语言 query；第 2 步：单条 result 的 title+snippet | 按相关度排序，URL 去重合并，低分结果另组展示；「相关度百分比是模型判断，不是经验证的准确率」（原文） | 无成本/准确率数字；工程参数可抄：每引擎 15 s deadline、整请求 30 s、引擎缓存 10 min–6 h、Jev provider 故障仅对 402/429/5xx 降级、400/401 不重试 | 是（16 个测试文件，mock provider，无需 key） | **可跑**：59 提交，MIT，最近提交 2026-09-20 |
| **hotchpotch/jev-reranker** | 对每个检索到的文档一条 Noul「是否与问题相关且可作为回答证据」 | 查询 + 单文档 chunk | 按概率排序，可选按可配置阈值过滤 | 无数字 | 是（9 个测试文件） | **可跑**：30 提交，MIT，2026-09-21 |
| **romeromarcelo/jev-retrieval** | 查询→grep 式 `path:start-end`；Noul 判定候选窗口 + 一条 listwise Choice 排序 | 本地 BM25 候选的 100/20 行窗口 | 已给出**可直接抄的阈值**：代码 0.90、文档 0.60；一条 listwise Choice/lane 排序 | 自述在 HAKARI-Bench NanoRTEB reranking 榜 90 个模型中排第 2 | 是（Rust CLI + Claude Code skill） | **可跑**：7 提交，Apache-2.0，2026-09-23；样本项目 |
| **kylemclaren/jevsearch** | 一次请求问 3 类：每页一条 Noul「访客会不会乐意落到这页」+ 一条 Choice 选最佳 + 一条 Noul「是否存在答案」 | 关键词命中的前 20 页 | 代码里重排/丢弃命中 | 自述 109 页 TypeSafe 文档上 Hit@1 **83% vs 纯关键词 41%** | 是（25 提交） | **可跑**：MIT，2026-09-23 |
| **kylemclaren/jevpdf** | 每行一条 Noul「这一行是否回答了查询」（16 行/请求，共享页面文本作 state） | PDF 单页文本 + 查询 | 按概率排序，**≥0.55** 高亮 | 无数字 | 是 | 未单独取元数据（同作者 jevsearch 同批） |
| **kitze/unclutter / realZachi/typesafe-adblock / davertor/jev-slop-guard** | 逐 DOM 元素 / 逐条社媒帖子做二分类（杂物 / 广告 / slop） | 单个元素或帖子的文本 | 隐藏/模糊/加标 | jev-slop-guard 默认阈值 **0.7**，3 并发上限，每帖缓存一条判定，加「显示原文」逃生门 | 是（unclutter 6 issue；slop-guard 76 提交） | **可跑**（unclutter 341★ MIT；slop-guard 3★ 但 76 提交） |

**「给每条 feed 打相关/不相关 + 置信度」的阈值策略（跨项目归纳）**【仓库/代码】：阈值全部在**代码侧**，且散落区间为 **0.55（jevpdf）→ 0.60（jev-retrieval 文档）→ 0.70（slop-guard、Sniff Test、mastra 审核）→ 0.80（JevGate）→ 0.90（jev-retrieval 代码）→ 0.95（tax-doc-classifier 拒答）**。无任何项目把阈值交给模型。低置信的处置也一致：**不动作、交人工/交 review**（jev-logtriage「低置信进 review，什么也不执行」）。

### 2.2 文档分类与切分

| 项目 | 决策内容 / 问题类型 | 输入 state | 输出怎么用 | 实测收益 | 成熟度 |
|---|---|---|---|---|---|
| **jerryjliu/docjev**（LlamaIndex 作者）【仓库/代码】 | ① 整篇文档 → 一个 Choice（自然语言类别规则）；② 包内边界 → 每页一个边界判定 | 本地 LiteParse 抽出的页面文本（可选 LlamaParse 云 OCR） | 类别 + 概率 + review flags；切分输出有序类别与页区间；**边界分落在阈值 ±0.1 内触发 review**（`--boundary-review-margin 0` 可关） | 40 份真实 PDF（8 类）：分类 **40/40 vs GPT-5.6 Luna 40/40**；切分 **7/8 vs Luna 8/8**；Jev 中位延迟 **138.6 ms 分类 / 209.6 ms 切分**，Luna 为 794.3 / 1352.3 ms（**5.73× / 6.45×**）；本次决策成本 **$0.011663 vs $0.046894**。作者明确声明：「这些是 40 份小样本的描述性结果，不构成一般化切分准确率」 | **可跑**：Apache-2.0，16 测试文件，10 提交，2026-09-26；README 自述「完全由 Codex 维护」 |
| **kyotofin/tax-doc-classifier**【仓库/代码】 | 每页一个 Choice（261 个 IRS 表单 + 7 种 page kind，230 个一级选项）；5 种公司/外国表额外二次请求 | `pdftotext` 抽出的 {header, body, footer} | `formConfidence`（各步最小值），**≥0.95 才 `gated=true` 可自动动作，否则回退到现有流程** | 314 页已填表 **0 严格错误，$0.36**；753 页空白表 **0 答错、38 页（5.05%）低于置信门槛被拒**，$0.86。对 Sonnet 分类器：**$0.00115/页 vs $0.039/页（34× 便宜）、~0.5 s vs ~3.3 s（6× 快）** | **可跑但薄**：Apache-2.0，**仅 3 提交**，仅 1 个 eval 相关文件，最近提交 2026-09-20 |
| **PostHog/jeeves**【仓库/代码】 | 不是 Jev 的用法，而是**训练一个会推理的 Jev 类模型**（Qwen3.5-9B + LoRA + pointer head，SFT + CISPO） | 同 Jev API（state + questions），可 `return_reasoning` | 兼容 Jev 的 `/v1/systemone`，**SDK 是 Jev Python SDK 的 drop-in 替身** | 自述 Test overall **0.889 vs Jev 0.857**；JevBench **0.935 vs 0.866**；JevBench hard 0.865 vs 0.730；**ECE 0.037 vs 0.049**。代价：1×H100（FP8 需 Hopper），不思考 ~0.3 s、思考中位 3.3 s / p90 17.1 s | **可跑但重**：MIT，20 提交，**创建于 2026-09-29（前一天）**，**0 个测试文件**，HF 模型 0 下载 0 赞，**无本地消费级部署路径** |
| **AgriciDaniel/jev-seo**【仓库/代码】 | 站点审计：每页问页型、搜索意图、重要度、helpfulness、specificity、trust、citability、标题/描述匹配、页面间互竞 | 爬取的页面 | 与 52 条 Google Search Central 规则结合成 52 规则评分 + 优先级修复清单 | 自述标准模式**约 1 美分/Site**（`--full` 加 DataForSEO 约 $0.30） | **疑似空壳风险高**：MIT 但 **仅 4 提交且全部在创建当天（2026-09-22）**，仓库 6.6 MB（大部分应为 PDF/图片产物） |

> **注意**：awesome 列表收录的 `AkashPriyadarshii/jev-seo` 与上面的 `AgriciDaniel/jev-seo` 是**两个不同项目**，检索时勿混。

### 2.3 去重 / 聚类 / 标注

| 项目 | 决策内容 / 问题类型 | 输入 state | 输出怎么用 | 实测收益 | 成熟度 |
|---|---|---|---|---|---|
| **keltokhy/jlink**（记录链接/去重最完整案例）【仓库/代码】 | 对每一对记录问「按这条自然语言规则，它们是不是同一实体」→ Noul | **两条记录 + 自然语言匹配规则**（规则是方法的一部分，可写进论文附录） | 输出概率 + margin；可设阈值、要求胜过次优、把不确定性带入估计；可抽样人工标注反推 precision/recall/校准表 | 自述 **171,354 对共 $2.95、约 250 对/秒、约 0.2 s/对**（≈ **$0.0000172/对**）；无标签下在 12 个标准基准的 11 个上击败 LinkTransformer 零样本模型 | **可跑**：MIT，30 个测试文件，Python/CLI/Stata/R，支持 `.csv/.dta/.parquet`；**`--api laya` 可完全本地不出机器** |
| **AkashPriyadarshii/jev-curate** | 对合成 JSONL/Parquet 行做 Noul 筛选 + 校准置信度 | 单行记录 | 通过的记录与拒绝流式写盘 | 无数字 | 未单独核实 |
| **cephalization/jev-triage** | 每个 issue 固定一组类型化问题：kind、severity、urgency、duplicate、next step | 单个 issue 文本 | 多人分诊看板；**每次人工纠正都被保留并在后续运行中回喂给模型** | 无数字 | 未单独核实 |
| **JevSpan / jevextract** | 命名实体识别 / 信息抽取：code 先提候选 span，Jev 用 Choice+Noul 逐 span 判定与验证 | 按标点切分的文本窗口 | 阈值以上保留，临界值标 review | JevSpan 自述 12 个中英 NER 基准平均 **73.7 严格 F1 vs Qwen3.8-27B 直接抽取 72.1**；jevextract 自述比 LangExtract 在 Gemini 3.5 Flash 上**便宜 10–26×** 但 F1 更低（84.2 vs 88.5） | 未单独核实 |
| **jev-align (Sutro)** | 对 CSV/Parquet/JSONL 行做 Choice/Score/Boolean，模糊样本交人工 | 单行记录 | 被接受的人工标签用来**优化保存下来的判定定义（GEPA）** | 无数字 | 未单独核实 |

> **去重的关键结论**：决策模型能做去重，但**只在"需要语义规则"时才赚**（jlink 的核心卖点就是字符串距离做不到的 IBM↔International Business Machines、母子公司区分）。对本产品的 feed 去重（同事件多来源），**MinHash/SimHash + URL 规范化是更便宜且足够的方案**（见 ④ 劝退项）。

### 2.4 安全护栏

**这是本调研最重要的纠偏区。**

| 项目 | 决策内容 / 问题类型 | 输入 state | 输出怎么用 | 实测收益 | 成熟度 |
|---|---|---|---|---|---|
| **NandhaKishorM/laya 的 `guard_questions()`**【官方】 | 5 问一次前向：jailbreak(Noul)、prompt_injection(Noul)、**sensitive_data(Noul，「是否含凭据/个人数据/敏感信息」)**、harm_severity(Score 4 档)、topic(Choice 6 类) | `{"prompt": ...}` 单段文本 | 预设只给概率，**门限与动作在代码里** | Laya 官方英文任务榜：**prompt-injections 0.698（held out, n=116）**，多语版 0.578【官方】 | **可跑**：Laya 全库 123 个测试文件、Apache-2.0、28.8k★ 但仅创建 12 天 |
| **javimp2003/laya-guardrails**（**对本产品最关键的负面证据**）【仓库/代码】 | input / tool_call / output 三阶段护栏，laya-pt-es-typed（西/葡语微调）vs 同机 Qwen3-4B-Instruct vLLM 裁判 | 单条 prompt / tool call / 回答 | 阈值在 dev 上拟合、test 上出数 | **tool_call 校验：AUC 0.958（裁判 0.833），p50 5–6 ms vs 138–181 ms（23–30×），吞吐 ~166 vs ~98 req/s，显存 2.3 GB vs 13.7 GB，100% 拦下 wrong_args 与 unrequested_action 且 0 假阳性**；output AUC 0.931 vs 0.910。**但 input(jailbreak/危害)：AUC 0.840 vs 裁判 0.992，且把 65% 的正常消息也标为可疑（flag FPR 0.67），只 block 时仅拦下 25–50%** | **可跑但小**：Apache-2.0，自述测试集仅 24–52 例、CI 存在；作者自己写「laya-pt-es-typed **不是安全模型**，其 jailbreak 信号是 zero-shot，这就是 input 结果差的原因」 |
| **CodeAlive-AI/mastra-jev-moderation** | 一次请求：Boolean「这条消息必须被拦吗」+ 类别 Choice | 单条消息 | **≥0.7 中止本轮对话**，带 deadline + 熔断，**fail open** | 自述生产拦截 **9/9 敌意、0/49 真实消息**，中位 **~0.4 s**，比 LLM 审核器**便宜约 4×** | **疑似空壳**：MIT 但 **仅 4 提交、0 测试文件**、11 KB、创建当天即停（2026-09-18） |
| **brainstormity/Jev-Moderation-Bot** | 逐条消息打分：钓鱼、垃圾、社工 → **四级升级阶梯** | 单条消息 | 分级处置；被赦免的消息作为「已验证安全先例」回注上下文 | 无数字 | **可跑**：MIT，13 提交，48★，Python |
| **4rays/profanity-checker** | **两问**：字面脏话(Noul) + 语音/形近伪装（`a55h0le`、`mike_hunt`）(Noul) | 文本或用户名 | 代码里做 `max()` 策略、阈值、JSON/OpenAPI 输出，Cloudflare Worker 可被其他 Worker 通过 service binding 调用 | 无数字 | **薄**：MIT，3 提交，1★，134 KB |
| **backmeupplz/jev_antispam_bot** | 每条消息问 Jev | 单条 Telegram 消息 | 最小 grammY 反垃圾机器人 | 无数字（自述 10 个测试文件） | **可跑**：MIT，12★，最近提交 2026-09-28 |
| **BasmaAbouzied0/jev-secret-guard**（编码侧，作为"通用做法"的模板）【仓库/代码】 | Claude Code PreToolUse hook：**已知密钥格式本地正则拦**，只有**未知高熵串才脱敏后**发 Jev 问 Noul「是不是真凭据」 | 掩码后的字符串 | **≥0.80 block；0.30–0.80 或在 Jev 不可用时一律问人工** | 自述发布校准：**6/6 秘密被拦，0/6 良性串被拦** | **薄**：MIT，**0★、6 KB、6 分钟内 2 次提交（2026-09-27）** |
| **leepokai/jev-guard** | prompt-injection 与危险动作护栏，面向 Claude Code / Codex / Pi / ACP | 工具调用 | Jev 决定拦什么 | 无数字 | 未单独核实 |
| **klauswg/jev-suite（jev-proof / jev-fidelity）** | 逐条事实 Noul+Choice：编辑是否保真（preserved/equivalent/drift/lost）、赞助片段是否合规；**代码侧保留 0.70 置信门** | 视频字幕/维基修订 diff | 低于门限**降级为人工复核而不是放行** | 自述 jev-proof 90 样本门控准确率 **0.922、0/15 注入翻转**；jev-fidelity 55 样本 **91/92 门控判断正确、0/20 注入翻转** | **可跑**：MIT，34★，Java，最近提交 2026-09-23 |
| **eugeniughelbur/jev-engineering** | **确定性规则优先 + 一次类型化 Jev 调用**兜 SDK 工具调用 | 工具调用 | 自述可复跑的 **300 次注入测试**，公布「拦到什么、什么溜过去了」 | 自述 300 次注入测试 | **可跑**：MIT，39 提交，2026-09-27 |

> **Yaml 未找到项**：用户点名的 `16sulphur/laya-prompt-guard` **不存在**（404）。我找不到任何名为「laya-prompt-guard」的仓库。**通用敏感信息检测**除 Laya 的 `sensitive_data` Noul 与编码侧的 jev-secret-guard 外，**未找到**非编码场景的独立通用项目。

### 2.5 自然语言入口 / 意图路由

| 项目 | 决策内容 / 问题类型 | 输入 state | 输出怎么用 | 实测收益 | 成熟度 |
|---|---|---|---|---|---|
| **anishfn/shapeshift**（与本产品「自然语言查询用量」最接近的形态）【仓库/代码】 | **一次调用问 14 个类型化问题**（哪张卡片 + 是否视频通话 + 是否紧急 …） | 用户**正在键入**的输入框文本（debounce） | 「Jev 决定，代码计算」：日期/金额/单位/数学全走确定性 parser。**默认完全离线**（内置关键词分类器），有 key 才切到在线 | 无数字，但有**可直接抄的 UI 状态机**：卡片只有「挑战者连赢两次」或「非常确定」才切换；信号徽章用 on/off 迟滞带；离线/限流/不可达时**静默回退离线模式** | **可跑**：MIT，18 提交，752★，7 测试文件，Next.js；`NEXT_PUBLIC_USE_MOCK=true` 可强制离线 |
| **angel291592/Intent-Router** | 把模糊请求收敛成 `IntentSpec`：probe（读 deps/routes/git 历史）→ 只问文件答不了的那一个问题 → typecheck → emit；拒绝在意图不足时输出 | 仓库文件 / 工单系统 / 文档 | 产出 `.intent/<name>.intent.yaml`，后续会话先读它、不重复提问；交付后逐条约束核对实现 | 自述 evals：8/10、8/8、6/6 子集，**0 幻觉引用**（「一条编造的引用就整轮判失败」）；交付对比 bare 5.0/6 (N=5) vs 有 skill 6.0/6 (N=3) | **可跑，但 Jev 是未实现的 planned 层**：MIT，100+ 提交，CI 徽章，`evals/` 53 个文件、`skills/`；**97 个路径里没有任何 Jev/Laya/backend 文件**；README 自述分层 L0 默认 prompt-only（$0）→ L1/L2 「planned」，L2 才用 Jev/Laya，标 **~$0.0004/decision（未实现）** |
| **monteduro/killmyidea** | 一次请求 10 问：8 个 0–4 分 indie-hacker 问题 + 类别 + 是否可理解 | 一段创业想法描述 | 每项 ×25 → 加权平均 → 清晰度门 → KILL/FIX/SHIP | 自述有 `npm run benchmark`（合成平衡用例，输出分数/判决分布、类别准确率、清晰度门命中、重复打分区间） | **可跑**：TypeScript，10 提交，**无许可证**，244★；UI 有完整「How Jev decided」面板（原答案、概率、置信度、平均、判决、延迟、token 用量） |
| **realZachi/pg-jev**（自然语言问 Postgres）【仓库/代码】 | `jev(row, '条件')` 布尔谓词；另 `jev_prob` / `jev_score` / `jev_choice` / `jev_confidence` / `jev_eval` | **整行按表别名**作为 state，`jev.batch_size` 默认 **20 行/请求** | 概率、阈值、缓存、`jev_stats()` 报请求数/token/估算成本/缓存命中 | **2,000 行：首跑 ≈3.5 s / 100 请求 / ≈296k 输入 token / ≈$0.012；二跑 ≈50 ms（按行内容缓存）；`LIMIT 3` ≈0.6 s**。**关键校准数据：批量 1–20 行 100% 正确，40 行 92–98%，80 行 77–94%**；单行 435 token vs 批量 20 时 175 token/行（摊掉 ~270 token 请求开销，批 20 只比批 40 贵 4%） | **可跑**：PostgreSQL License，PGXN 已发布，12 个测试文件（pg_regress，自带 mock API），支持 Docker；8 提交；**需超级用户 + plpython3u，Supabase/Neon/RDS 装不了** |
| **droidrun/mobile-jev** | 手机端逐步决策（点哪、输什么） | 设备屏幕状态 | 驱动 Mobilerun 手机 | 自述 demo：Uber 录屏 **9 个动作约 21 秒**；**明确写「未演示完成下单」** | **疑似空壳/demo**：MIT 但 **仅 1 次提交**（2026-09-17 一次性上传），仓库 28 MB（媒体资产） |
| **afshinm/laya-mps** | 本地跑 Laya typed-decisions（客服/发票/安全事件/agent trace 微调版） | — | 本地 HTTP 服务 + 演示 | 自述 **M5 Pro 中位 ~32 ms，~2.1 GiB RAM**，另有 **~0.74 GiB 低内存模式**；首次运行需 4 GB 磁盘、模型 ~843 MB | **可跑**：MIT，22★，2026-09-21 |

### 2.6 用量 / 成本分析的决策化

**这一节的核心是反面证据。**

| 项目 | 决策内容 / 问题类型 | 输入 state | 输出怎么用 | 实测收益 | 成熟度 |
|---|---|---|---|---|---|
| **suenot/codex-jev-router**（含官方 benchmark 仓库 + AUDIT.md）【仓库/代码】 | ① **已弃用**的路线：对短任务摘要问 Choice/Noul 选 Codex 子代理模型与推理档；② **当前推荐**的路线：**确定性证据选择**（本地 MCP 搜工作区、只把几条精确引用给 Codex），Jev 不再是默认 | 有界的工作区搜索结果 | 返回精炼引用而非大段原始日志 | 旧路由路线自述：**Jev 选的子代理比固定 Sol-high 便宜 27.2%，但比"单个 Sol-xhigh agent"贵 69.7%**。新确定性路线 holdout（144 次完整 Codex 运行）：全部 16 任务 **$2.060703 → $1.653006（-19.8%）**，eligible 日志/多文件任务 **-29.9%**，噪声日志任务 **-41.5%**，strict passes **45/48 → 48/48**。**同一报告实测：本地 Laya 选择器比确定性选择贵 8.8%**，明确写「Jev 仍是可选实验项，在我们的 holdout 中**没有**比确定性选择更省钱」 | **可跑且有审计**：MIT，7 个测试文件；**只有 4★**（被 awesome 列表推荐但实际冷门） |
| **jcressler/jev-codex-token-saver** | 同一干预点：**在大工具结果进入 Codex 上下文之前**拦住，本地扫出有界候选 → Jev 类型化证据选择 → 只回几条精确摘录；缺事实时**只允许一次**定向补救 | 本地工作区候选包（小包**绕过 Jev**） | 返回摘录；认证/网络/格式/超时失败做**一次尝试后明确标注的本地回退，无隐藏重试** | 无数字；明确声明「不是原生 compaction 的替代，不重写历史，不声称每个任务都省 token」 | **可跑**：MIT，18 个测试文件，7★，Codex 插件 |
| **miuuyy/Astra-Ares** | Jev 读**有界**任务上下文，选 GPT-6 下一次的 reasoning effort + 该 effort 持续几代 | **有界上下文**：最近 **6** 次工具调用、每次结果 **1,000 local token**（头尾截断）、整请求 **28,000 token 硬上限**、超限显式停止 | Codex 原生应用 effort 变更（不破坏 prefix cache） | **作者明确写「workload 缓存命中率与相对固定 effort 的节省尚未测量」**。有 `decisions.jsonl` 决策日志 | **优先做参考实现**：自述 experimental，仅 macOS Apple Silicon 实测通过，**需要单独打补丁的 Codex CLI + ~10 GB 编译空间**，293★ |
| **xinyao27/jevonian** | 一次 Jev 调用同时选 model route 与 thinking level（`jevonian/auto`）；**确定性代码先按配置/协议/quota 健康/上下文窗口/thinking 下限过滤候选** | 会话状态、配额健康、候选能力、缓存切换代价 | 本地 ledger 记录模型、token、成本、理由；`minConfidence` 只把低置信**标记**在 ledger 与响应头，不擅自接受 | 无节省数字 | 自述 **v0**；无独立元数据核实 |
| **Laya `router_questions()` 预设**【官方】 | **4 问一次前向**：`difficulty`（Score 4 档：trivial/easy/moderate/hard）、**`domain`（Choice 6 类：code / math_or_logic / writing / factual_lookup / data_analysis / chitchat）**、`needs_tools`(Noul)、`is_sensitive`(Noul) | `{"request": ...}` 单段请求文本 | 路由或**用量按域拆分** | 无数字；**这是本次找到的唯一现成的「请求按任务类型分类」类型化 schema**，可直接用于用量拆分 | **可跑**：`laya/presets.py`，Apache-2.0 |
| **jev-logtriage / jev-risk-check-provider** 等 | 见 ③ | | | jev-risk-check-provider 自述 540 次调用在阈值 65–75 上 **99.76% 通过、0 假阳性**，**~$0.00005/decision、p50 ~400 ms**；1,500 例对抗红队 100% | 未单独核实 |

> **未找到**：任何把 Jev/Laya 用于「token 用量异常检测」或「成本归因」的成熟项目。`ingon1026/jev-anomaly` 是**图像**异常检测（Jev-Omni 12B + llama.cpp + MVTec AD），与本问题无关【仓库/代码】。

---

## ③ 量化数字汇总

### 成本
| 数字 | 含义 | 来源 | 档位 |
|---|---|---|---|
| **$42 / 十亿输入 token（= $0.042 / 1M）**，输出 token 免费 | TypeSafe 官方定价 | TypeSafe models page，经 `walidboulanouar/awesome-jev-use-cases` 与 Laya README 双处转述一致 | 【官方】（经第三方转载） |
| **$0**（自托管） | Laya 本地推理边际成本 | Laya README 对比表 | 【官方】 |
| **$0.00115 / 页** vs Sonnet $0.039 / 页 | IRS 税表分类 | tax-doc-classifier README | 【仓库/代码】（项目自述） |
| **$0.0000172 / 对**（171,354 对共 $2.95） | 记录链接去重 | jlink README | 【仓库/代码】（项目自述） |
| **≈$0.012 / 2,000 行**（≈296k 输入 token） | SQL 语义筛选 | pg-jev README | 【仓库/代码】（项目自述） |
| **$0.011663 vs $0.046894**（40 文档全流程） | 文档分类/切分 vs Luna | docjev benchmark | 【仓库/代码】（项目自述） |
| **$0.0008 / 视频**；约 1 美分 / 站点；8 美分 / 1,018 篇论文 | jev-skip；AgriciDaniel/jev-seo；@nutlope | 各自 README / X 帖 | 【仓库/代码】【第三方文章】（项目自述） |
| **~$0.00005 / decision，p50 ~400 ms** | 风险检查 | jev-risk-check-provider README | 【仓库/代码】（项目自述） |
| **~$0.0004 / decision（未实现）；~$0.0001 / decision（未实现）** | Intent-Router 计划中的 L2（Jev/Laya）与 L1 层 | Intent-Router README | 【仓库/代码】（**计划值，非实测**） |

### 延迟
| 数字 | 含义 | 来源 | 档位 |
|---|---|---|---|
| **32.8 ms**（1 问，T4）/ **7.2 ms/问**（批 10）/ **103–332 问/秒**（单 T4） | Laya | Laya README 实测表 | 【官方】 |
| **193–464 ms（CPU，preload 后）** | Laya CPU 部署 | Laya README | 【官方】 |
| **~32 ms，~2.1 GiB RAM**（M5 Pro）；~0.74 GiB 低内存模式 | Laya 本地 macOS | afshinm/laya-mps README | 【仓库/代码】（项目自述） |
| **5–6 ms p50**，~166 req/s | laya 护栏单次检查（NVIDIA L4） | laya-guardrails README | 【仓库/代码】（项目自述，样本小） |
| **236–276 ms p50**（Jev，独立测量）| 对照 Laya 快 6–7× | AbdelStark/jev-benchmarks、nibzard/decision-model-benchmark（经 Laya README 引用） | 【第三方文章】 |
| **70–500 ms 端到端**；比 frontier LLM 快 40–200× | Jev 官方 | TypeSafe 发布帖（经 awesome 列表转述） | 【官方】（自述） |
| **138.6 ms 分类 / 209.6 ms 切分**（中位，Jev 1.13.0） | docjev，40 文档 | docjev benchmark | 【仓库/代码】 |
| **~0.4 s 中位** | mastra 审核一次判定 | mastra README | 【仓库/代码】（项目自述） |
| **~0.2 s / 对**，250 对/秒 | jlink | jlink README | 【仓库/代码】（项目自述） |
| **~500 µs** | jev-harness「测试失败分诊」 | awesome-jev 分类表 | 【仓库/代码】（项目自述） |

### 准确率 / 校准 / 阈值
| 数字 | 含义 | 来源 | 档位 |
|---|---|---|---|
| **批量 ≤20 行 100% 正确；40 行 92–98%；80 行 77–94%** | **单请求内 item 数与准确率的取舍**（本报告最可复用的一条） | pg-jev README，基于结构化列真值，每档 400 行 | 【仓库/代码】 |
| 并行提问 **12.2× 便宜、10.0× 快** | 一次请求问多题 vs 每问一次 | TypeSafe cookbook（经 awesome 转述） | 【官方】（自述） |
| ECE **0.466 → 0.081**（laya）、**0.314 → 0.106**（multilingual） | 温度标定后（每「问题类型×选项数」一个温度） | Laya README | 【官方】 |
| 零样本 typed-decisions **0.362 / 0.352**，随机 0.318，多数类 0.461 | **Laya 基座低于多数类基线**；微调后 0.766 | Laya README + `laya-typed-decisions` HF card | 【官方】 |
| **>20 选项时 Laya 明显弱于 Jev**：Banking77 Jev 0.870(72 标签) vs Laya 0.425(77 标签) | 选项共享固定 `head_max_len` 预算 | Laya README「Where Jev leads」 | 【官方】（自述） |
| prompt-injections **0.698**（n=116，held out） | Laya 英文基座 | Laya README 英文任务表 | 【官方】 |
| input 护栏 AUC **0.840 vs 0.992**；flag FPR **0.67** | **Laya 挡输入侧 jailbreak 不行** | laya-guardrails（24 例 test，CI 宽） | 【仓库/代码】（项目自述，样本小） |
| tool_call 护栏 AUC **0.958 vs 0.833**，0 假阳性 | Laya 校验工具调用可行 | laya-guardrails | 【仓库/代码】（同上） |
| 阈值谱：**0.55 / 0.60 / 0.70 / 0.80 / 0.90 / 0.95** | 各项目代码侧门限 | 见 2.1 归纳 | 【仓库/代码】 |
| Jeeves **0.889 / 0.935**（Test / JevBench）vs Jev **0.857 / 0.866**，ECE 0.037 vs 0.049 | 会推理的 Jev 类模型 | PostHog/jeeves README | 【仓库/代码】（项目自述） |
| 成本：**Jev 子代理比单 agent 贵 69.7%**；确定性选择 **-19.8% ~ -41.5%** | Codex 成本优化 | suenot/codex-jev-router holdout 报告 | 【仓库/代码】（项目自述 + 公开报告） |

### 未找到
- 「每千次决策成本」的统一权威口径：**未找到**。只有上面这些按场景的收入式数字，量级 **$0.001–$0.05 / 千次**（自托管 Laya 为 $0）。
- CPU 上每次决策延迟的**独立第三方**测量：**未找到**。只有 Laya 自述 193–464 ms（CPU preload 后）与 omp-laya-judge 自述「CPU 上约 0.3 s」。

---

## ④ 映射到 Switchelp 五板块（**以下均为【推测】**，除标注来源的实测引用）

统一三维：**隐私**（是否读用户会话） / **本地 vs 在线** / **降级行为**。

### 4.1 内容中心（RSS + GitHub 抓取，每源 20 条）

| # | 候选用法 | 决策内容 / 问题类型 | state | 输出怎么用 | 隐私 | 本地/在线 | 降级 |
|---|---|---|---|---|---|---|---|
| C1 | **feed 相关性打分排序**（推荐） | 每条一条 Noul「这条是否与我的关注点相关」+ 可选 Score 重要度 | `{条目 title + summary, 关注点自然语言描述}`。**不要塞全文**——TypeSafe 官方明确「state 中无关内容会降低准确率」 | p 排序；阈值建议从 **0.60** 起（jev-retrieval 文档档）；保留原始时间序作并列次序 | **低**：只发标题+摘要，不碰会话 | **可本地**（Laya + laya-mps，M5 Pro ~32 ms/2.1 GiB，Apache-2.0）；在线 Jev 约 $0.042/1M | 关键词/BM25 或纯时间序；**永不因模型失败而空列表**（jev-search 的做法：失败源显示警告而非 0 结果） |
| C2 | **条目详细度分流**：Score 3 档（扫一眼 / 值得读 / 深度读） | Score | 同上 | UI 上分区展示；不删条目 | 低 | 同 C1 | 全部归「扫一眼」 |
| C3 | **同事件多来源合并** | — | — | — | — | — | **劝退，用规则**：URL 规范化 + 标题 MinHash/SimHash + 时间窗。jlink 类语义链接的收益来自「字符串距离做不到的实体同一性」，多来源资讯标题不属此列 |
| C4 | **抓取失败诊断** | Choice：抓取失败原因（源变更 / 限流 / 网络 / 解析失败） | 失败元数据（HTTP 码、源类型、失败次数、上次成功时间戳） | 决定退避策略与是否禁用源 | 无 | 在线 | 已有退避逻辑，模型仅作辅助提示 |

### 4.2 插件中心（从公开源安装技能/插件）

| # | 候选用法 | 决策内容 / 问题类型 | state | 输出怎么用 | 隐私 | 本地/在线 | 降级 |
|---|---|---|---|---|---|---|---|
| P1 | **安装前安全预检**（推荐，且与 jev-axi / pi-verdict 的形态一致） | 一次请求多问：Noul「是否要求执行破坏性命令」、Noul「是否要求上传本地数据/凭据」、Noul「是否与声明功能不符」、Choice「风险等级」 | **单个 SKILL.md 全文**（短文本，天然适合） | **确定性规则先短路**（已知危险命令、`curl | sh`、密钥格式 → 直接拒）；只有灰区才看模型概率；**≥0.80 拦，0.30–0.80 或模型不可用 → 询问用户**（这是 jev-secret-guard 已发布的处置策略） | **低**：只发公开 SKILL.md，不发用户会话 | 可本地（Laya guard 预设的 `sensitive_data` Noul 现成） | **fail-closed 到「询问用户」**，不静默安装（pi-verdict：error/timeout → deny） |
| P2 | **技能与当前需求匹配** | Noul「这个技能是否适合当前任务」，可加 Choice 候选排序 | 技能名+描述 + 用户当前意图 | 只做**排序与徽章**，不自动安装 | 中：需用户当前意图文本 | 本地 | **shadow 模式起步**（jev-skill-router：默认只记日志不注入；gate 与 per-candidate fit 双双过 0.30 才建议） |
| P3 | 来源可信度评级 | — | — | — | — | — | **劝退，用规则**：stars / 许可证 / 最近提交 / 是否有测试 / 是否单日批量提交。本次调研本身就是证据——规则（提交数、许可证）比"看起来专业"更能识破空壳（见 mastra 4 提交 0 测试、droidrun 1 提交 28 MB） |

### 4.3 用量统计（解析本机 Codex 会话记录）

| # | 候选用法 | 决策内容 / 问题类型 | state | 输出怎么用 | 隐私 | 本地/在线 | 降级 |
|---|---|---|---|---|---|---|---|
| U1 | **请求按任务域拆分用量**（唯一我认为值得做的） | Laya `router_questions()` 的 `domain` Choice（code / math_or_logic / writing / factual_lookup / data_analysis / chitchat）+ `difficulty` Score 4 档【官方，schema 现成】 | 单条请求文本（**这就是隐私红线**） | 用量按域出饼图；对高 token 低难度的域给「可换更便宜模型」的提示 | **高**：必须读请求文本。**默认关闭**；开启后仅本地 Laya 推理；**只落盘分类结果与聚合，不落盘原文**；提供「只统计不分类」的降级开关 | **仅本地**（Laya） | 关闭分类，退回纯 token/时间聚合 |
| U2 | **该不该切更便宜模型 / 模型路由** | Choice 在候选 (model, effort) 上选 | 会话状态、配额健康、缓存切换代价 | 建议而非自动执行 | 高 | 本地 | — | 
| U3 | **用量异常检测** | — | — | — | — | — | **劝退**：先用分位数/EWMA/同比等统计规则；**未找到**任何 Jev/Laya 做用量异常检测的成熟案例 |
| U4 | **有界上下文的设计约束（照抄，不用模型）** | — | Astra-Ares：最近 **6** 次工具调用、每次结果 **1,000 token**、整请求 **28,000 token 硬上限** | 若未来真要把会话摘要发给任何模型，这是可复用的预算护栏 | — | — | — |

> **U2 的劝退证据（必须写进产品决策）**：suenot/codex-jev-router 的 holdout 实测显示，**「用决策模型做子代理/模型路由」比「一个 xhigh 单 agent 跑到底」贵 69.7%**；真正省钱的是「在工具结果进上下文之前做确定性证据裁剪」（-19.8% ~ -41.5%）。**而且本地 Laya 选择器比确定性选择还贵 8.8%**。对本产品（管理 Codex 多供应商的桌面壳），**"省 token" 的正确抓手是网关侧的上下文裁剪与请求去重，不是决策模型路由**。

### 4.4 免费额度（聚合各供应商免费额度信息）

| # | 候选用法 | 结论 |
|---|---|---|
| F1 | 从额度页自然语言中抽取「每月 X 次免费」结构化字段 | **劝退**：正则/结构化解析更可靠；决策模型不生成文本也**不做算术**（TypeSafe 官方明列「不是计算器，计数与日期比较要放进代码」） |
| F2 | Noul「这条额度信息是否仍有效 / 是否限时活动」 | **劝退**：产品已有「时间戳兜底」机制，规则足够且更可解释 |
| F3 | 额度耗尽风险的 Score 提示 | **弱建议**：纯规则（剩余量 / 速率 / 历史消耗）即可，模型无增益；**未找到**同类先例 |
| F4 | 供应商额度页改版后的解析失败诊断 | 与 C4 同形态，可作为 C4 的一个类目复用，不必独立建设 |

> **本板块整体劝退。** 聚合 + 展示 + 时间戳是纯工程问题。

### 4.5 网关诊断（上游 400 需区分协议翻译问题 vs 鉴权问题）

| # | 候选用法 | 决策内容 / 问题类型 | state | 输出怎么用 | 隐私 | 本地/在线 | 降级 |
|---|---|---|---|---|---|---|---|
| G1 | **400 归因**（**最高价值、隐私风险最低**） | 一次请求：Choice「{auth / protocol_translation / quota_or_rate / upstream_server / client_bad_request / unknown}」+ Noul「是否可安全重试」+ Score「用户需介入程度」 | **仅错误元数据**：HTTP 状态、供应商名、上游 error.code/type、error.message 片段（截断，脱敏）、请求路径、是否流式、是否含 tool call、重试次数。**绝不含会话内容** | UI 给出归因 + 建议动作；「协议翻译」类给出字段级提示；重试决策由代码执行 | **极低**（只发错误元数据） | **强烈建议本地 Laya**（5–6 ms/次量级，出错时高频调用不吃成本）；在线 Jev 亦可（$0.042/1M，错误文本很短） | **规则表优先**（401/403→auth、429→rate、5xx→upstream、超时→network）；模型只处理规则表未覆盖的灰区；模型失败 → 归「unknown」并展示原始错误，**绝不猜** |
| G2 | **协议不兼容特征判别**：Noul「这个错误是否由 OpenAI↔Anthropic 字段/角色/shape 不兼容导致」 | Noul + Choice（具体不兼容的字段类别） | 请求/响应结构摘要（**只发 schema 与字段名，不发值**） | 生成修复提示；引导用户在网关侧改配置 | 极低 | 本地优先 | 归入「未知协议错误」，只展示原始报文 |
| G3 | **供应商健康度评分**（哪个 key 该被降权/停用） | — | — | — | — | — | **劝退，用规则**：成功率、p95 延迟、429 率滚动窗口即可；G1 的归因结果反而应作为**规则**的输入而非另一层模型 |

---

## ⑤ 来源清单

**起点 / 榜单**
- `https://github.com/yibie/awesome-jev`（2,000★，无许可证，README 含「Curation is not endorsement」警告与逐类目文件）— 【仓库/代码】
- `https://github.com/yibie/awesome-jev/blob/main/categories/scoring-ranking.md`、`content-moderation.md`、`data-labeling-curation.md`、`verification-guardrails.md`、`adaptive-realtime-ui.md`、`classification-routing.md`
- `https://github.com/walidboulanouar/awesome-jev-use-cases`（CC0-1.0，含「Reported cost and latency」「Limits of Jev 1.13」两节，本报告大量成本/延迟原始出处）— 【仓库/代码】
- `https://github.com/kydlikebtc/awesome-jev`（NOASSERTION，587★）、`https://github.com/logicrw/awesome-jev-projects`（MIT，626★）

**Laya（开源替代）**
- `https://github.com/NandhaKishorM/laya`（Apache-2.0，28,868★，创建 2026-09-18，100+ 提交，tag 至 v0.3.22，123 个测试文件）— 【官方】
- `https://github.com/NandhaKishorM/laya/blob/main/laya/presets.py`（`guard_questions` / `moderation_questions` / `router_questions` / `triage_questions` 的确切 schema）— 【官方】
- `https://huggingface.co/convaiinnovations/laya`、`/laya-multilingual`、`/laya-typed-decisions`（均 apache-2.0）— 【官方】
- `https://github.com/afshinm/laya-mps`（MIT，M5 Pro 32 ms / 2.1 GiB，含 0.74 GiB 模式）— 【仓库/代码】
- `https://github.com/mizorewww/laya-mlx`（Apache-2.0，6,643★）、`https://github.com/mizorewww/laya-coreml`（Apache-2.0，1,530★）、`https://github.com/receptron/laya`（MIT，ONNX Runtime for Node/TS，633★）、`https://github.com/tc3oliver/laya-apple`（Apache-2.0，MLX+ANE）— 【仓库/代码】
- `https://github.com/javimp2003/laya-guardrails`（Apache-2.0；**反面证据来源**）— 【仓库/代码】
- `https://github.com/PS061188/laya-guard`、`https://github.com/dockndevai/laya-guard`、`https://github.com/morre95/Laya-GuardRails-Harness`（均 <2★，未采纳）— 【仓库/代码】

**平台与清单中核实的项目**
- `https://github.com/superagents-lab/jev-search`（MIT，491★，59 提交，16 测试）— 【仓库/代码】
- `https://github.com/PostHog/jeeves`（MIT，315★，20 提交，**0 测试文件**，建于 2026-09-29）— 【仓库/代码】
- `https://github.com/jerryjliu/docjev`（Apache-2.0，482★，10 提交，16 测试）— 【仓库/代码】
- `https://github.com/kyotofin/tax-doc-classifier`（Apache-2.0，483★，**3 提交**）— 【仓库/代码】
- `https://github.com/AgriciDaniel/jev-seo`（MIT，238★，**4 提交且全在创建当天**）— 【仓库/代码】
- `https://github.com/realZachi/pg-jev`（PostgreSQL License，381★，8 提交，12 测试）— 【仓库/代码】
- `https://github.com/suenot/codex-jev-router`（MIT，**4★**，7 测试）及 `https://github.com/suenot/codex-jev-router-benchmarks`（holdout 报告）— 【仓库/代码】
- `https://github.com/jcressler/jev-codex-token-saver`（MIT，7★，18 测试）— 【仓库/代码】
- `https://github.com/miuuyy/Astra-Ares`（293★，MIT）— 【仓库/代码】
- `https://github.com/angel291592/Intent-Router`（MIT，374★，100+ 提交，`evals/` 53 文件；**Jev 层未实现**）— 【仓库/代码】
- `https://github.com/anishfn/shapeshift`（MIT，752★，18 提交，7 测试）— 【仓库/代码】
- `https://github.com/keltokhy/jlink`（MIT，7★，30 测试）、`https://github.com/hotchpotch/jev-reranker`（MIT，36★）、`https://github.com/romeromarcelo/jev-retrieval`（Apache-2.0，4★）、`https://github.com/kylemclaren/jevsearch`（MIT，9★）
- `https://github.com/CodeAlive-AI/mastra-jev-moderation`（MIT，**4 提交 0 测试**）、`https://github.com/brainstormity/Jev-Moderation-Bot`（MIT，48★）、`https://github.com/4rays/profanity-checker`（MIT，3 提交）、`https://github.com/backmeupplz/jev_antispam_bot`（MIT，12★）、`https://github.com/davertor/jev-slop-guard`（MIT，76 提交）、`https://github.com/kitze/unclutter`（MIT，341★）、`https://github.com/realZachi/typesafe-adblock`（MIT，88★）
- `https://github.com/BasmaAbouzied0/jev-secret-guard`（MIT，**0★，6 KB，2 次提交**）、`https://github.com/leepokai/jev-guard`、`https://github.com/eugeniughelbur/jev-engineering`（MIT，39 提交）、`https://github.com/klauswg/jev-suite`（MIT，34★）
- `https://github.com/monteduro/killmyidea`（**无许可证**，244★，10 提交）、`https://github.com/droidrun/mobile-jev`（MIT，425★，**1 提交，28 MB**）
- `https://github.com/xinyao27/jevonian`、`https://github.com/malevrigns/agent-jev`（元数据读取时 API 返回 EOF，**未核实**）、`https://github.com/ingon1026/jev-anomaly`（**图像**异常检测，不相关）

**未找到 / 不存在（明确记录）**
- `https://github.com/16sulphur/laya-prompt-guard` → **HTTP 404，仓库不存在**；用户 `16SULPHUR` 名下亦无同名仓库
- 通用（非编码场景）敏感信息检测的独立决策模型项目 → **未找到**
- 「每千次决策成本」统一权威口径 → **未找到**
- Jev/Laya 用于 token 用量异常检测的成熟案例 → **未找到**
