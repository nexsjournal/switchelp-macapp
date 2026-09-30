# Jev / 决策模型在编码 Agent 与网关里的集成模式

**调研日期**：2026-09-30
**范围**：Jev（TypeSafe System One）及 Laya 等同类"非生成式决策模型"在编码 Agent（Codex / Claude Code / Pi / DSH）与本机网关中的集成模式，以及这些模式到 Switchelp 的映射。
**不在范围**：Jev 模型本身的训练/校准原理；非编码场景（邮件、招聘、IoT 等）的用例；对具体机构的合作建议。
**证据档位**：【官方】厂商标注 / 【仓库·代码】仓库 README、源码、评测报告 / 【第三方文章】外部引用 / 【推测】本文作者推断。README 自述数字一律标"项目自述"。
**方法**：只读。全部结论来自 GitHub API、README/源码原文与仓库元数据。未 clone、未执行任何仓库脚本。未找到可信第三方复现的项已标注。

---

## ① 一句话结论

这套生态已经收敛出**六个固定插入环节**（路由 / 压缩 / 门控 / 审查 / 记忆 / 协议），但**当前可复现的收益全部来自"把证据筛掉"而不是"把模型换小"**——把大段工具输出/搜索结果替换成少量精确片段（jevgrep −28.6%、suenot −29.9%~−41.5%）比按轮次切换模型更可靠；而**按轮次切模型有多份反向实测**（suenot：子代理选路比单个 Sol-xhigh 贵 69.7%；hermes：Jev 摘要式交接召回率反低于原文），且**开源免费的 Laya 在同一 holdout 里比确定性选择贵 8.8%**——所以对 Switchelp 的第一优先级落在"离线用量回放 + 元数据级路由 + 证据裁剪"，而不是"上决策模型"。

---

## ② 按环节分组的模式表

成熟度：**可跑**=有真实代码 + 有测试或公开评测产物；**疑似空壳**=提交数≤10 或文件数极少、无测试、无评测；**未知**=素材不足以判断。星数普遍与实质严重脱节（见 ③ 末段）。

### 2.1 路由（按轮次选模型 / 推理强度）

| 项目 | 决策发生在哪一步 | 问什么 / 哪种类型 | 决策后如何动作 | 实测收益 | 实现难度 | 成熟度（依据） |
|---|---|---|---|---|---|---|
| [0xNatoshi/jev-codex-router](https://github.com/0xNatoshi/jev-codex-router) | Codex Router(:4202) 收到 `model=jev/auto` 的每次 Responses 调用前 | **一次请求 4 个独立 Choice**：①是否强制旗舰（架构/终审/风险类）②最小够用的能力档（Luna/Terra/Sol/Astra）③最小够用思考深度 ④路由租约（one_call/tool_chain/user_turn） | 只改 `model` 与 `reasoning.effort`；SSE 原样转发；**fail-open** 到"Astra/medium"；哨兵文件做 kill switch；**配额耗尽**时才启用本地发现的兼容路由 | 自述历史仿真 **≈ −60% vs 全 Astra**（237 轮，旧策略），作者明示"不是实测 Codex 配额节省"【仓库·代码】 | 高（需 fork Codex Router + LiteLLM + 本地决策服务） | **可跑，但仓库已归档（archived=true）**；69 commits、MIT、有 CI/backtest 文档（2026-09-22 最后推送） |
| [suenot/codex-jev-router](https://github.com/suenot/codex-jev-router) | **已放弃模型路由**；改为 MCP 工具在"搜索结果进入上下文之前"筛选证据 | 现役：确定性筛选（不调 Jev）；历史：Choice 端子代理模型+effort | 返回几条带行号的精确片段给 Codex；小候选集/失败一律走确定性回退 | **审计 holdout（144 runs）**：全 16 任务 −19.8%；预设"日志/多文件"组 −29.9%；**噪声日志任务 −41.5%**；通过率 45/48→48/48。历史路由：Jev 选子代理比固定 Sol-high 便宜 27.2%，但比单个 Sol-xhigh 贵 **69.7%** | 中（MCP + rg） | **可跑**；24 commits、MIT、有独立 benchmark 仓库与 AUDIT.md；作者主动降级旧路由 |
| [miniLV/Jev-Auto-Router](https://github.com/miniLV/Jev-Auto-Router) | 本地 Responses 代理，每次"有意义的模型调用"前 | 一次 Choice 选 `(model, effort)` 精确对；候选必须是"本机实测可请求"的白名单 pair | 只改 `model` 与 `reasoning.effort`；Astra 档需"一次性准入券"；OFF/隐私拒绝/低置信/超时统一走固定基线 | 自述**仍属验证原型**："真实 caller-edge 证明资产、实机 A→B→A、配对评估结论尚未完成"，不宣称普遍节省【仓库·代码】 | 高（Responses 代理 + M6 审计 + 对照评估门禁） | **可跑（原型）**；20 commits、Apache-2.0、`npm test`/typecheck；作者明确拒绝生产承诺 |
| [ruban-24/switchboard](https://github.com/ruban-24/switchboard) | 编码 CLI 内的代理；**选一次即钉住整个会话** | System One 判断难度/够不够上下文（Choice/Noul），**本地策略**做最终选择 | 钉住 (model, effort) 以保 prompt cache；低置信走 uncertain fallback | 自述路由开销：**中位 0.55s / p95 0.67s，约 $0.62 每 1000 次分类**【仓库·代码】 | 中 | **可跑**；19 commits、Apache-2.0；**同时支持 Jev 与自建 Laya** |
| [xinyao27/jevonian](https://github.com/xinyao27/jevonian) | 本机 OpenAI/Anthropic 兼容代理，`jevonian/auto` 路由 | 一次调用同时答"哪条路由"+"想多深"；state 含**缓存切换代价**估计 | 确定性代码先过滤候选；`minConfidence` 只在账本标记低置信而**不采纳**；账本记服务模型与原因 | 未找到实测收益数字（自述强调"未知配额按中性处理"） | 中高 | **可跑**；AGPL-3.0（**许可证对闭源产品不友好**）；2026-09-29 活跃 |
| [gargpratyush/jev-router](https://github.com/gargpratyush/jev-router) | `jev-claude` 启动的 Claude Code；注入"Jev Router"到 `/model` 选择器 | Jev 打分 4 维（任务复杂度/是否需要推理/工具复杂度/上下文大小）→ Choice 选档 | 换 Claude 模型档；状态行显示"上一轮用了哪个模型 + p 值 + 上下文 %" | 未找到实测数字；自述"大对话拒绝会浪费 prompt-cache 的降级"【仓库·代码】 | 中 | **未知**；496 星但 48 commits、无测试目录、MIT |
| [miuuyy/Astra-Ares](https://github.com/miuuyy/Astra-Ares) | Codex 任务**运行中**（不是每次调用），改已选模型的 effort | Jev 选下次该想多深 + 这个 effort 持续几次生成 | 通过 config 写入 checkpoint 应用；bridge 在模型网络路径之外 | 自述"**workload cache 命中率与相对固定 effort 的节省均未测量**"【仓库·代码】 | 中 | **疑似空壳**；仅 4 commits、MIT、293 星；有 native fixture tests 但无评测 |
| [BillionsBobby/JevRouter](https://github.com/BillionsBobby/JevRouter) | 选项集合统一为"模型/子代理/技能/MCP 工具/CLI/插件" | 一次 Choice 在候选集中选；候选超限则先粗排 Top-K 再二次 Choice | 用代码强制可用性/权限/风险/确认 | 自述 Toolathlon 10 任务前 5 有序工具位置命中率 **38%→44%**（串行→分解+上下文），自述约快 5.5×、便宜约 7×；DeepSeek V4.1 Flash 对照 24%【仓库·代码】 | 中 | **可跑**；73 commits、MIT；作者注明"provider 性能声明仍是 provider 声明" |
| [kerpopule/hermes-jev-skills](https://github.com/kerpopule/hermes-jev-skills) | 每轮开始时在"你可调用的所有模型"里选够用的那个 | Choice（tier × work-kind 网格） | 分池切换；shadow 模式只记录 | 自述**约 0.4s/轮**【仓库·代码】 | 中 | **可跑**；100+ commits、918 星、MIT；含多份带日期的 SCORECARD |
| [angel291592/Intent-Router](https://github.com/angel291592/Intent-Router) | **路由之前**：把含糊请求编译成 typed `IntentSpec`，宁可 probe / 追问 / halt | 探针 + 一次追问 + 与 spec 逐条比对 | 产出 `.intent/<name>.intent.yaml`，后续会话先读它、不再重复追问 | 自述 3/3 交付物写出失败策略、5 个裸交付 0 个（opencode 交付对比）【仓库·代码】 | 中 | **可跑**；100+ commits、373 星、MIT |
| [notque/vexjoy-agent](https://github.com/notque/vexjoy-agent) | `/do` 入口把自然语言路由到专家 agent | Jev 分类意图；**dispatch 以 Jev receipt 为准** | 派发 + 评审/测试门禁 + 学习回路 | 未找到量化收益 | 高（大而全） | **可跑**；100+ commits、425 星、MIT；2026-03 创建（生态里少见的"老"仓库） |

### 2.2 上下文压缩与裁剪

| 项目 | 决策点 | 问什么 / 类型 | 动作 | 实测收益 | 难度 | 成熟度 |
|---|---|---|---|---|---|---|
| [tamaratran/fast-jev-compaction](https://github.com/tamaratran/fast-jev-compaction) | 宿主要压缩时，**替代**摘要 | 每个非钉住 tool call **两个 Noul**：①"还记得调用过它、带输入的这件事"是否仍重要 ②"结果原文是否仍需要且重跑得不到" | 保留/截断到首 N 字符+一行注释/整对删除；**绝不改写**用户与助手文本；按 token 预算分阶段装填 state；超限即抛错 | 自述提供 `reductionRatio`，调用方自设 <0.25 视为不值得；**无公开收益数字** | 中（已是 npm 库 + CC 插件） | **可跑**；30 commits、MIT、7220 星；被大量 port |
| [tamaratran/jev-pruner](https://github.com/tamaratran/jev-pruner) | Bash 输出**进入模型之前** | 评分裁剪长 stdout/stderr | 裁剪结果 + 标记被删区段，指向引擎自己保存的完整输出文件 | 自述"概率阈值是保留策略，不是实测误差保证"【仓库·代码】 | 低 | **可跑**；100+ commits、MIT、2026-09-30 活跃 |
| [leonaaardob/fast-dev-compaction](https://github.com/leonaaardob/fast-dev-compaction) | Codex 生命周期钩子（压缩前后） | 同 fast-jev-compaction | 逐字恢复上下文而非摘要 | 无 | 低 | **可跑（移植版）**；36 commits、10 星、MIT |
| [joelhooks/pi-fast-jev-compaction](https://github.com/joelhooks/pi-fast-jev-compaction) | Pi 扩展 | 同 | 裁剪陈旧工具历史，**不够用时才回退 Pi 自带摘要** | 无 | 低 | **可跑**；8 commits、14 星、MIT |
| [compozy/yoshi](https://github.com/compozy/yoshi) | CC/Codex 代理内 | Jev 判断哪些历史仍需要 | 代理层裁剪 | 自述"measured not claimed"但**未找到具体数字** | 中 | **未知**；27 星、27KB、MIT、2026-09-18 后未更新 |

### 2.3 工具调用门控 / 自动批准 / 安全

| 项目 | 决策点 | 问什么 / 类型 | 动作 | 实测收益 | 难度 | 成熟度 |
|---|---|---|---|---|---|---|
| [leepokai/jev-guard](https://github.com/leepokai/jev-guard) | **每次工具调用前 + 每次工具返回后 + 每次加载指令文件**（skill/plugin/CLAUDE.md/AGENTS.md） | 前：`risk`（4 级 Score）+ `approval`（Noul）+ `user_requested`（Noul）+ `from_untrusted`（Noul）；后：`directed`（Noul）+ `kind`（Choice: injection/canary/discussion/benign）；文件：Choice over exfiltration/covert_execution/instruction_override/canary/unrelated_side_effects/clean | 阈值全在 20 行代码里：`deny if from_untrusted≥0.7` / `deny if risk≥2.5` / `allow if (risk≥1.5 or approval≥0.75) and user_requested≥0.85` / else `ask`；只读工具跳过；按内容哈希缓存 | **价格**[第三方·Vercel 模型卡] $0.042/1M 输入、$0 输出 → 典型 ~1k token/次 ≈ **$0.00004/次调用**；**延迟实测**（作者，从台湾，含 TLS 与进程启动）**~0.75s 直连 / p50 ~580ms 经 AI Gateway**；662 个真实已装 skill 中 0 个越线（最高 0.74），植入样本 exfiltration 0.99 / covert `curl\|sh` 0.98 / canary 0.51；实测 `git push --force` 在用户明说后 ask→allow(p=0.96)，被网页注入的同一条命令被 deny(p=0.97)【仓库·代码】 | 中（0 依赖、单 hook 覆盖 8 个宿主） | **可跑（生态内质量最高）**；24 commits、MIT、npm 发布、有真机校准表 |
| [valentynkit/jev-belay](https://github.com/valentynkit/jev-belay) | Claude Code **Stop hook**：只在"有文件改动且此后没有通过的检查"时才问 | 一次调用 **4 个问题**（读 transcript 找证据） | 阻塞"未经验证的完成"；任何异常 fail-open | **实测 AUROC 0.976**（100 条真实标注 stop），仅看措辞为 0.777；只有 **17.7%**（2694 个 stop 中 477 个）会走到提问；触发时 1 次调用、输入 **中位 1222 token**、**$0.00005**【仓库·代码】 | 中 | **可跑**；37 commits、71 个测试文件、MIT；明确写了"明显做法已被测量且不work" |
| [AskTheWay/dsh-jev-interceptor](https://github.com/AskTheWay/dsh-jev-interceptor) | DSH 的 `tools/pre-execute` + `approval/request` + 会话快照保留 | 风险/不可逆/是否符合任务/注入嫌疑（Choice+Noul 扇出）；保留：每条消息 `noise/background/relevant/critical` | **从不 allow**，无异议表达为 `next()` 让下游保留否决权；证据门控自动批准（必须抓到参数证据）；never 策略在上游先执行 | 自述实测 501 输入 token ≈ **$0.00002/次**；provider 侧 p50 ~100ms、美西之外端到端 ~1s；shadow 模式给出反事实（FIFO 会丢而评分会留的是哪些）【仓库·代码】 | 中高 | **可跑**；14 commits、**63 个测试**、MIT、npm 发布、有 CI |
| [DanRWilloughby/snifftest](https://github.com/DanRWilloughby/snifftest) | 文本产出后（散文 linter，非 agent 门控） | 可数规则 + **一个**判断模型 | 报"AI 写作痕迹" | 无 | 低 | **可跑**；97 commits、57 测试文件、34 星、MIT |
| [ckorhonen/jev-lint](https://github.com/ckorhonen/jev-lint) | agent **还在写**的时候，对照团队最佳实践模糊检查 | 未找到问题清单细节 | 提前提示，不等 code review | 无 | 中 | **可跑**；30 commits、70 测试文件、MIT（语言标 HTML，存疑） |
| [BasmaAbouzied0/jev-auto-approve](https://github.com/BasmaAbouzied0/jev-auto-approve) / [jev-secret-guard](https://github.com/BasmaAbouzied0/jev-secret-guard) | CC PreToolUse hook | 单个 Noul："这条 shell 命令是否严格只读" | auto-approve ≥0.95，否则回落普通权限提示，**从不 deny**；本地硬拒清单 + 注入过滤在 Jev 之前 | 自述校准中 8 条改状态命令 **0 条被批准**；保密仓自述"已知密钥本地拦截、未知密钥交 Jev 判断并打码" | 低 | **疑似空壳**；各 3 commits、5 个文件、**0 星**、单测 1 个；同作者同日发布 |
| [codejunkie99/keel](https://github.com/codejunkie99/keel) | **宿主应用**在"全新、未钉住的任务"开始前选一次路线 | 本地 Laya（Core ML）或可选托管 Jev 选候选路线，**或明确弃权** | **宿主校验每一个选择**；被拒/过期/失效即回落普通路线；每次决策在 transcript 留一条有界 receipt（候选、结果、校验、回退、观测结果）；`keel decisions export/report` 导出 JSONL 回放用例 | 自述"构建检查不证明更好的编码结果"；roadmap 里 replay/held-out 对比**尚未实现**【仓库·代码】 | 高（Rust/GPUI 原生 macOS 应用） | **可跑（与本产品形态最像）**；10 commits 但 **115 个测试文件**（历史被压平）、MIT、Rust |

### 2.4 代码 / 仓库检索与审查

| 项目 | 决策点 | 问什么 / 类型 | 动作 | 实测收益 | 难度 | 成熟度 |
|---|---|---|---|---|---|---|
| [dzhng/jevgrep](https://github.com/dzhng/jevgrep) | Agent 要在陌生仓库找东西时，**在源码进入上下文之前** | Jev 发现相关文件与源码上下文 | 先给摘要+紧凑文件清单，再给选中源码（带行号），再给声明/调用位置；显式声明"是证据不是答案" | **实测**：10 个调优过的 Python SWE-bench 任务，Jevgrep 与基线**都解 8/10**；Sol 成本 $7.62→$5.44 = **−28.6%**（含失败尝试，不含 Jev 成本）；0.4.3 含 Jev 总成本**−25.8%** 仍 8/10；0.5.0 把 Jev 自身成本再降约 59%，但 Sol+Jev 合计**贵 2–3%**，作者明示"单次观测，不构成统计等价"【仓库·代码】 | 中 | **可跑**；100+ commits、**124 个测试文件**、MIT、1827 星 |
| [suenot/codex-jev-router](https://github.com/suenot/codex-jev-router)（证据选择部分） | 见 2.1 | 确定性优先，Laya 可选 | 返回精确片段 | 见 2.1：−29.9% / −41.5% | 中 | **可跑** |
| [thruwire/foreman](https://github.com/thruwire/foreman) | 软件工厂**监督层**：观察多个 coding agent 的工厂事件 | "这个 worker 卡住了吗""现在是否需要独立验证"（监督类窄问题） | 由 deterministic 逻辑把一个或多个 Jev 检查翻译成指令；不替 worker 选工具/文件；**职责没有聚合分数** | 自述"**Jev 在该用例上的评估准确率未经证明**，语义分数需要校准"【仓库·代码】 | 中高 | **可跑**；30 commits、24 测试文件、MIT、619 星 |
| [egma-ai/jev-code-reviewer](https://github.com/egma-ai/jev-code-reviewer) | 审**行为**而不只是 diff；PR 分级 | 把每个变更分类为 P0/P1/P2 | 默认只显示 P0；diff 用自然语言呈现，原代码一键切换；本地 CLI + agent skill + GitHub 扩展 | 无量化；自述动机"agent 突然甩出 230 个文件变更" | 中 | **可跑**；10 commits、11 测试文件、MIT |
| [NiazMorshed2007/jev-review](https://github.com/NiazMorshed2007/jev-review) | 本地 MCP server，实现切片后 / 改进后 / 交付前的频繁检查点 | Score/Choice/Noul 多维度（正确性、复杂度、可改性、模块化、测试、安全…） | 返回 1–10 分 + 0–1 confidence，**故意不给合成总分**；agent 自己诊断原因与改法；支持 `previousEvaluation` 对比 | 无量化 | 中 | **疑似空壳/未知**；仅 4 commits、231 星；有 8 个测试文件 |
| [devagrawal09/jev-review](https://github.com/devagrawal09/jev-review) | 分阶段代码审查工作流 + 本地 dashboard | 未找到细节 | 无 | 无 | 低 | **疑似空壳**；**5 commits、0 测试文件、161KB 但 643 星**；2026-09-16 建、09-17 后未更新 |
| [reticlehq/reticle](https://github.com/reticlehq/reticle) | 运行期感知：verification run 的流程路由 | "页面是否已稳定""这个发现值不值得追""这个失败要不要全量捕获" | 3.2.0 已落地的是 **Jev 驱动应用**（`reticle_verify {action:"explore", driver:"jev"}` 从 DOM 枚举的候选里选，**只选不生成**）；**路由那一层是 roadmap，尚未发布** | 自述 Jev 70–500ms；无收益数字 | 高 | **可跑**；100+ commits、**1489 个测试文件**、Apache-2.0+FSL；614KB+；但 Jev 用法目前是探索而非路由 |

### 2.5 记忆与技能选择

| 项目 | 决策点 | 问什么 / 类型 | 动作 | 实测收益 | 难度 | 成熟度 |
|---|---|---|---|---|---|---|
| [Dicklesworthstone/skillranker](https://github.com/Dicklesworthstone/skillranker) | CC `UserPromptSubmit` hook：下一步该用哪个已装 skill | 两轮 Jev：先粗比全部候选，再读入围者更丰富摘要；**每轮都含真实"以上都不是"选项** | 建议是**咨询性**的，agent 自己决定是否加载；≥254 个 skill 时先用本地检索（Quill）收窄；显式请求本地直接解析 | 自述 203 个测试文件；含 `sr calibrate` / 评估抽样 / 风险监控；未找到外部复现 | 中高（Rust CLI） | **可跑**；100+ commits、125 星、**MIT + OpenAI/Anthropic rider**（注意附加条款）；需自备密钥 |
| [shimo4228/jev-skill-router](https://github.com/shimo4228/jev-skill-router) | CC `UserPromptSubmit`：装了什么 skill 该用哪个 | 一次 Choice over 已装 skill 名录 + 是否需要 skill 的 Boolean 门 | 只有门与逐候选适配度**都过 0.30** 才建议；默认 **shadow**（只记录不注入） | **反向结论**：README 专门记录"为什么它不太可能帮到一个强模型做路由"【仓库·代码】 | 低 | **可跑**；11 commits、14 测试文件、MIT、7 星（低星但诚实） |
| [kitze/skillbox](https://github.com/kitze/skillbox) | 自托管版本化技能库中的推荐 | 有界目录 + 评分 rubric，最多 32 skill / 24,000 字节一批，两批并发，8s 截止 | 可选 Jev 推荐；**任一批失败则整个请求回退到确定性搜索并给出显式回退原因**；三家 provider 各自存 key、互不串用 | 无 | 中 | **可跑**；11 commits、19 测试文件、MIT；Provider 契约写得很细 |
| [zilliztech/memsearch](https://github.com/zilliztech/memsearch) | 记忆检索结果重排 | 可选 **远程 Jev rerank**（不下载本地模型） | 混合检索 → Jev 重排 | 自述有中英文评测文档；未取到具体数字 | 中 | **可跑**；2685 星、MIT、2026-02 创建（生态中历史悠久者） |
| [kitfunso/hippo-memory](https://github.com/kitfunso/hippo-memory) | 记忆召回重排 +"标错了就不再出现" | 可选托管 Jev reranker（默认关） | 错误记忆半衰期翻倍；本地 SQLite + MCP；CC 7 个 hook | 自述私有 300 query 开发库 **R@1 0.41→0.62**；**但同一文档记录负面结果**：3 个 graded 测试在 150 题 LongMemEval 上 **答案率并未优于免费本地 cross-encoder**；Jev 买到的是"更短上下文"（Jev 排 2 条 ≈ cross-encoder 排 5 条） | 中 | **可跑**；766 星、MIT、零运行时依赖；benchmarks 目录含失败运行与预注册 |
| [yuyang2230/jev-agent-skill](https://github.com/yuyang2230/jev-agent-skill) | Agent skill 形式 | 把 classify/screen/score/verify 卸载给 Jev（经 OpenCode Zen 免费额度） | 零依赖 `jev.py` 调用器（transient-500 重试、WAF-safe UA、GBK-pipe-safe stdin） | 含 `references/benchmark-jev-vs-mainmodel.md`（未逐个核） | 低 | **疑似空壳**；0 星、10 个文件、MIT |

### 2.6 协议 / MCP 形态

| 项目 | 形态 | 关键设计 | 难度 | 成熟度 |
|---|---|---|---|---|
| [jkudish/jev-mcp](https://github.com/jkudish/jev-mcp) | MCP server，**11 个判断工具**：verify / screen / noul / find / rerank / classify / decide / compare / extract / review / **gate** | 每次判断返回 typed 概率 + confidence，150–500ms；`jev_gate` 一次调用同时审 patch 与核对每个"测试通过"声明；fail-closed；npm 包内附 agent skill | 低 | **可跑**；78 commits、MIT、463 星 |
| [vinilana/jev-gateway](https://github.com/vinilana/jev-gateway) | **本机 LLM 网关**（127.0.0.1），按 CLI 分端口（Codex 8790 / Claude 8789…） | Agent 要决定"调哪个工具"时问 Jev；confident 则用 `tool_choice` 把 LLM 导向该工具，**不 confident 就原样透传**；`--routing off` 保留计数做基线对照；`--dashboard` 单页看 6 个网关、2s 刷新；Jev 挂了/慢了/密钥错**绝不让请求失败** | 中 | **可跑**；74 commits、21 测试文件、MIT、npm 发布 |
| [itsmostafa/system-one-connector](https://github.com/itsmostafa/system-one-connector) | MCP `evaluate` 工具（Go） | 同一 wire 形状下可切 **Jev / CLM / Laya / Liquid d1**；`TYPESAFE_BASE_URL` 指向本地服务即可跑开源模型 | 中 | **可跑**；100+ commits、MIT、337 星 |
| [tacticocc/Jevbridge](https://github.com/tacticocc/Jevbridge) | **ACP + MCP** 双适配器 | Confidence gates：peaked→execute、middling→confirm、破坏性点击→不放过；后端可切 `jev\|llm\|heuristic\|auto`；MCP 工具 `jev_decide`/`jev_gate`/`jev_computer_use` | 中 | **可跑**；9 commits、9 测试文件、MIT |
| [AskTheWay/dsh-jev-interceptor](https://github.com/AskTheWay/dsh-jev-interceptor) | DSH 插件（Cordis waterfall） | 见 2.3；另附 `docs/jev-usage-points.md`——**对 DSH 全代码库系统扫描出的 13 个已验证决策点 + 适配度分级**，并明确列出"不属于 Jev 的领域" | 中高 | **可跑**；见 2.3 |
| [alterhq/typesafe-sdk-swift](https://github.com/alterhq/typesafe-sdk-swift) | Swift 6 SDK（零依赖） | Choice/Score/Noul + 严格并发 + 可配置鉴权与重试 + 离线传输测试 | 低 | 【仓库·代码】见 awesome list 条目；**未独立核实** |

---

## ③ 实测收益汇总

**A. 有第三方或独立审计来源的**

| 场景 | 数字 | 来源 | 是否第三方复现 |
|---|---|---|---|
| 证据选择替代模型路由（Codex） | 全 16 任务 **−19.8%**；预设"日志/多文件"组 **−29.9%**；噪声日志 **−41.5%**；通过率 45/48→48/48 | [suenot holdout 报告](https://github.com/suenot/codex-jev-router-benchmarks)【仓库·代码】 | 作者自审 + 公开 per-task 数据，非第三方独立复现 |
| 仓库检索（jevgrep，10 个 SWE-bench 任务） | Sol 成本 $7.62→$5.44 = **−28.6%**；含 Jev 总成本 **−25.8%**；两者都解 8/10 | [jevgrep README](https://github.com/dzhng/jevgrep)【仓库·代码】 | 自测；作者明示"不构成统计等价" |
| Jev 单次调用价格 | **$0.042 / 1M 输入，$0 输出**（≈ $0.00004/次工具调用） | [Vercel AI Gateway 模型卡](https://vercel.com/ai-gateway/models/jev)【第三方文章】 | 是（模型卡） |
| Jev 延迟 | 厂商宣称 150ms；**实测**直连 ~0.75s（含 TLS+进程启动）/ 经网关 p50 ~580ms；gateway 分类 p50 **0.55s**、p95 0.67s | [jev-guard](https://github.com/leepokai/jev-guard)、[switchboard](https://github.com/ruban-24/switchboard)【仓库·代码】 | 各自自测，口径不同 |
| 分类开销 | 约 **$0.62 / 1000 次分类** | switchboard【仓库·代码】 | 自测 |
| 完成度门控（jev-belay） | **AUROC 0.976** vs 只看措辞 0.777；仅 17.7% 的 stop 触发提问；$0.00005/次 | [jev-belay](https://github.com/valentynkit/jev-belay)【仓库·代码】 | 自测，100 条真实标注 |
| 注入检测（hermes web screening） | 80 条真实网页植入攻击**捕获 70/79**（Hermes 自带规则 11）；**1520 条干净 chunk 中 0 条被扣**；~0.2s/条 | [hermes-jev-skills](https://github.com/kerpopule/hermes-jev-skills)【仓库·代码】 | 自测；自述"不可移植的召回" |
| 提问措辞（一句话把"要求"与"偏好"分开） | 44 条人工标注集 **16/44 → 33/44** | [Virlo 自述](https://x.com/dsqjaffa/status/2102054148925526206)，经 hermes 文档转述【第三方文章】 | **否**，厂商自报 n=44，无逐例明细 |
| 有序工具预测（JevRouter） | 位置命中率 38%→44%，对照 DeepSeek V4.1 Flash 24% | BillionsBobby【仓库·代码】 | 自测，作者注明是"有序路由预测"不是端到端完成率 |

**B. 反向 / 负面实测（对产品决策更重要）**

| 结论 | 数字 | 来源 |
|---|---|---|
| **按轮次切模型可能整体更贵** | Jev 选子代理比固定 Sol-high 便宜 27.2%，但比**单个 Sol-xhigh** 贵 **69.7%** | [suenot/codex-jev-router](https://github.com/suenot/codex-jev-router)【仓库·代码】 |
| **免费本地 Laya 未跑赢零成本确定性规则** | 在 16 任务 holdout 上，14 次真用了 Laya 的检索里 Laya **贵 8.8%** | 同上【仓库·代码】 |
| **Jev 摘要式交接反而更差** | 由 Jev keep/summarize/drop digest 写的 handoff 召回**低于**原文全文：58.7% vs 37.5%（无搜索）、75.0% vs 68.3%（一次搜索）——最终发布的是全文 | [hermes-jev-skills](https://github.com/kerpopule/hermes-jev-skills)【仓库·代码】 |
| **免费本地 reranker 与付费 Jev reranker 打平** | 150 题 LongMemEval 上答案率**无改善**；Jev 只买到更短上下文 | [hippo-memory](https://github.com/kitfunso/hippo-memory)【仓库·代码】 |
| **技能路由对强模型可能无用** | 作者在 README 专门记录"为什么它不太可能帮到一个强模型" | [shimo4228/jev-skill-router](https://github.com/shimo4228/jev-skill-router)【仓库·代码】 |
| **Jev 自身成本可以吃掉大部分收益** | jevgrep 0.5.0 把 Jev 成本降 59% 后，Sol+Jev 合计**反而贵 2–3%** | [jevgrep](https://github.com/dzhng/jevgrep)【仓库·代码】 |
| **模型路由的收益作者自己不敢宣称** | 0xNatoshi 的 −60% 是"历史仿真、非实测配额节省"；miniLV 明确"不宣称普遍节省"；Astra-Ares"cache 命中率与节省均未测量" | 三者 README【仓库·代码】 |

**C. 生态可信度警告（直接引用策展方原话）**

awesome-jev 维护者在 README 顶部写：*"A listing is not an endorsement... **Treat same-day bulk submissions with particular care.** Several repositories published together by one author, sharing a scaffold and a thin commit history, can satisfy every inclusion rule and still be unproven. Volume is not evidence of quality."*【仓库·代码】
本次核实的实际分布印证了这句话：同一作者同日发布、单次提交、共享 scaffold 的仓库确实存在（如 `prismhq/jev-router` 仅 1 commit；`BasmaAbouzied0/*` 各 3 commits、0 星）。**同时星数不可靠**：`devagrawal09/jev-review` 643 星但 5 commits、0 测试；`tamaratran/fast-jev-compaction` 7220 星但 30 commits、09-18 后未更新。**本报告一律以"提交数 + 测试文件数 + 是否有公开评测产物"为成熟度判据，不以星数为据。**

**D. 平台约束（影响实现）**【官方·经多个仓库复述】
- 请求上限 **64k token**，其中 state + 最长问题 **32k**——所有集成都必须先裁剪输入。
- 并发上限约 **8**；实践中的客户端把在飞请求压到 4 并做失败冷却。
- **英文优先**：CJK 可用但准确率更低（BillionsBobby 明确把英文 state 定为更安全默认）。
- 输出是**带校准概率 + confidence**，不是文本；多个仓库强调"把数值判断放回代码，不要放模型里"。
- 提供商**承认对抗输入可以影响分类器** → 门控类决策只能是"加速器"，不能是最后防线。

---

## ④ 映射到 Switchelp 的候选插入点与风险（本节全部为【推测】）

**已核实的本产品现状**（用于锚定插入点）：
- 网关 `crates/switch-core/src/gateway/`（mod/server/routing/auth/sse/timeouts），`DEFAULT_PORT = 18765`，仅绑 loopback；`routing.rs` 把"请求前缀 + alias"解析成固定 `RequestRoute`，请求开始即固定 `routeRevision + credentialVersion + protocolVersion`，不静默回落。
- Bridge `crates/bridge/src/lib.rs`（1024 行）：顶替 `CODEX_CLI_PATH`，起两根真 codex，合并 `model/list` 与 `thread/list`，**按线程钉住**；**两条硬约束已写进文件头注释——"不记内容"（日志只记方法名/id/参数键名/路由结论）与"不假装成功"**。
- 用量 `crates/switch-core/src/usage/mod.rs`（911 行）：扫描 `<codex_home>` 会话记录，产出 `UsageTotals{input,cached,cacheWrite,output,reasoning,total}`、`UsageDay`、`UsageModelRow`、`UsageProviderRow`，以及 **`UsagePlanWindow{plan_type,used_percent,window_minutes,resets_at}`**——即**官方计划配额窗口已被解析出来**。
- 另有 `plugins/`（安装/技能/源）、`content/`（feed）、`catalog/tools.json`。

### 插入点排序

**① 用量页离线分析（推荐第 1 位，风险最低）**
把已有的 `UsageReport`（按模型/按供应商/逐日/缓存命中/推理 token）做成**"反事实成本回放"**：对历史每一轮，用本地规则或 Laya 算"如果当时走更便宜的档会怎样"，输出"假设节省"并显式标注为估算。
- 复用：`by_model` / `by_provider` / `daily` / `cached_tokens` 已就绪；**不需要新读任何会话内容**（该页本来就是用户主动查看的聚合）。
- 参考实现：[xinyao27/jevonian](https://github.com/xinyao27/jevonian) 的账本 + 基线对比、[miniLV](https://github.com/miniLV/Jev-Auto-Router) 的 Router Compass、[keel](https://github.com/codejunkie99/keel) 的 `decisions export/report`（JSONL 回放用例）。
- 风险：低。**唯一要守的是别把"估算"渲染成"实测"**——jevgrep / 0xNatoshi / miniLV 三家都在这一点上反复自辩，说明这是该生态最容易失信的地方。

**② 网关请求路径（推荐第 2 位，只允许元数据级 state）**
在 `Admission` 通过、拿到 `RequestRoute` 之后、转发上游之前插一个决策点，允许调整 `model` 与 `reasoning.effort`。
- **可以只用元数据**：jevgrep/suenot 的价值主张本质是"减少进入上下文的字节数"，而这可以由**已请求的 alias、上下文大小分桶、工具名、退出码/错误摘要、当前是第几轮**等非内容字段驱动——[miniLV](https://github.com/miniLV/Jev-Auto-Router) 的 M2 明确只提取"白名单路由事实"，[dsh-jev-interceptor](https://github.com/AskTheWay/dsh-jev-interceptor) 也只发 head+tail 参数预览。
- **必须读会话内容的变体**（⚠ 与隐私底线冲突）：让决策模型看 prompt 或工具结果本身。0xNatoshi 的做法是"Jev 只看有界决策状态，执行模型拿到完整原文"，即两个投影分离——**这是唯一与"不记内容"兼容的读内容方式**：内容只在一瞬间的进程内存里，不落盘、不写日志。若采纳，必须在文档里同步修改"不记内容"的措辞为"不持久化内容"。
- **反向证据必须先看**：suenot 的 69.7% 与 hermes 的召回率下滑说明**按轮次换模型会因为 prompt cache 失效而亏回去**。switchboard 的做法（**选一次就钉住整个会话**）与 jevonian 的做法（把 cache 切换代价放进 state）是正面回应，应在设计里直接采用。
- 落地顺序：**先 shadow**（只记录"本该选什么"，不生效）+ `--routing off` 基线对照，这是该生态的事实标准（[dsh-jev-interceptor](https://github.com/AskTheWay/dsh-jev-interceptor)、[skillranker](https://github.com/Dicklesworthstone/skillranker)、[hrermes](https://github.com/kerpopule/hermes-jev-skills)、[shimo4228](https://github.com/shimo4228/jev-skill-router) 都有 shadow 字段）。
- **fail-open 是硬要求**：Jev 报错/超时/低置信一律走固定基线，绝不阻断 Codex 请求（jev-codex-router、jev-gateway、Astra-Ares 一致）。

**③ 宿主重载 / 重启前后（推荐第 3 位）**
`apply` 服务发布 `base_url + routeRevision + credentialVersion` 并触发宿主重载。决策模型可插在"是否值得重载/是否切换到某个候选发布"这一层——**规格是离散的、选项是闭合的、不需要读任何会话内容**，是最"干净"的 Jev 形状。
- 与之最接近的现成设计是 [0xNatoshi](https://github.com/0xNatoshi/jev-codex-router) 的**配额回退**：仅当观测到配额耗尽（429 / usage-limit）才启用本地发现的兼容路由，且**在向客户端发出可重试错误之前**先重试一次；自动翻转持续到窗口重置时刻。
- 而 `UsagePlanWindow.resets_at` **已经在本产品里被解析出来了**——两者的接口天然对得上。

**④ Bridge 按线程路由（不推荐作为决策模型插入点）**
Bridge 已经做了按线程钉住，但它处理的是 `model/list`、`thread/list` 这类**清单与线程归属**问题，不是"这一轮该用哪个模型"。把决策塞进 bridge 会同时违反它自己的"不记内容"约束，且 bridge 是 JSON-RPC 中间层、拿不到逐轮推理语义。**建议保持 bridge 不做语义决策**，只在 gateway 已经算出结论后由 bridge 读取一个已发布的候选目录。

**⑤ 插件中心与内容中心（可做，但预期收益有限）**
- 插件中心 → 技能推荐（[skillranker](https://github.com/Dicklesworthstone/skillranker) / [skillbox](https://github.com/kitze/skillbox) / [shimo4228](https://github.com/shimo4228/jev-skill-router)）。**必带 abstention（"以上都不是"）**，否则会重演 shimo4228 记录的问题。注意该生态最诚实的实测是"对强模型不太可能有用"，**建议先做 shadow 记录而不是直接推荐**。
- 内容中心 → 内容分类/去重（参考 awesome list 里大量 Choice 分类用例）。**不涉及用户会话内容**，纯站点内容，风险低；但与本产品核心价值关联度也最低。

### 必须单独标注风险的方案

| 方案 | 风险 | 说明 |
|---|---|---|
| 任何把 prompt / 工具结果原文发给决策模型的网关变体 | **高** | 直接与文件头"不记内容"冲突。即使不落盘，也把用户代码送出了本机（除非用本地 Laya）。**建议默认不做**；要做也必须"两个投影分离 + 不持久化 + 显式开关"。 |
| 门控 / 自动批准（对照 jev-guard、dsh-jev-interceptor） | **中高** | 需要读会话与工具结果；且**权限层归 Codex 所有，不归 Switchelp**。应先验证 Codex 的 hook 是否允许本产品介入，再谈模型。提供商自己也承认对抗输入能影响分类器 → 只能做加速器。 |
| 换模型导致 prompt cache 失效 | **中高** | 有明确反向实测（suenot 69.7%）。必须实现后置校验或"钉住会话"。 |
| 决策模型自身的成本 | **中** | jevgrep 0.5.0 的 Sol+Jev 合计反升 2–3% 是真实先例。**每轮都调**（≈$0.00004–0.00006/次）在小任务上可能不划算；应设"最小触发条件"（如 jev-belay 只有 17.7% 的 stop 会提问）。 |

### 关于"尽量不花钱"（Laya 路线）

- [NandhaKishorM/laya](https://github.com/NandhaKishorM/laya)：**Apache-2.0**，非自回归 System 1 决策引擎，同 Choice/Score/Noul 形状，单次前向，100+ 语言。【仓库·代码】
- [mizorewww/laya-coreml](https://github.com/mizorewww/laya-coreml)：**Apple Core ML + Neural Engine**，短决策 **P50 4.98ms / P95 5.31ms on M3 Max**，整系统能效比编译版 MLX FP16 好 2.78×；要求 Apple Silicon + macOS 15+。**与 Switchelp 的 macOS 形态高度契合，且完全离网。**【仓库·代码】
- [1Panel-dev/laya-server](https://github.com/1Panel-dev/laya-server)：自托管服务，**兼容 TypeSafe Jev 的 `/v1/systemone` 请求格式**（Apache-2.0，Docker）。→ **本产品只要按 `/v1/systemone` 写一个客户端，就能在"托管 Jev / 本地 Laya"之间切供应商，无需改代码路径。**【仓库·代码】
- [receptron/laya](https://github.com/receptron/laya)：Node/TypeScript 经 ONNX Runtime 跑 Laya（**跨平台，Windows 侧可用**）。
- **但必须知道反向证据**：suenot 的 holdout 里 **Laya 比零成本确定性规则贵 8.8%**；dsh 文档也指出 Laya"零样本更弱、上下文更短，需要领域微调"。
- **因此对"不花钱"的最优解可能根本不是决策模型**：suenot 那个 **−29.9% / −41.5%** 的收益来自**纯确定性**的证据筛选（ripgrep + 长度上限 + 路径排除），**零 API 成本、零网络**。建议 Switchelp 把"确定性裁剪"作为默认，"Laya 本地决策"作为可选增强，"托管 Jev"作为再上一层可选。
- 未找到 **Rust 官方 SDK**（`gh search repos "typesafe jev rust sdk"` 无结果）。`/v1/systemone` 是普通 JSON POST（`state` + `questions`，返回 `answers`/`probabilities`/`confidence`/`usage`），Rust 侧手写客户端成本很低；不要为它引入重依赖。

---

## ⑤ 来源清单

**策展清单（起点，非结论）**
- https://github.com/yibie/awesome-jev —— `categories/classification-routing.md`（52 条）、`categories/agent-decisions.md`（57 条）、`categories/infra-sdks-integrations.md`（96 条）；README 含"策展≠背书"与"同日批量发布"警告
- https://github.com/Anil-matcha/awesome-jev-by-typesafe、https://github.com/v-modal/awesome-jev-tools、https://github.com/cobanov/awesome-jev

**路由**
- https://github.com/0xNatoshi/jev-codex-router（MIT，69 commits，**已归档**）
- https://github.com/suenot/codex-jev-router（MIT，24 commits）+ https://github.com/suenot/codex-jev-router-benchmarks
- https://github.com/miniLV/Jev-Auto-Router（Apache-2.0，20 commits）
- https://github.com/gargpratyush/jev-router（MIT，48 commits，496 星）
- https://github.com/miuuyy/Astra-Ares（MIT，4 commits，293 星）
- https://github.com/BillionsBobby/JevRouter（MIT，73 commits，300 星）
- https://github.com/kerpopule/hermes-jev-skills（MIT，100+ commits，918 星）
- https://github.com/notque/vexjoy-agent（MIT，100+ commits，425 星）
- https://github.com/angel291592/Intent-Router（MIT，100+ commits，373 星）
- https://github.com/ruban-24/switchboard（Apache-2.0，19 commits，支持 Laya）
- https://github.com/xinyao27/jevonian（AGPL-3.0，16 星）
- https://github.com/prismhq/jev-router（MIT，**1 commit**——同日发布型）

**压缩/裁剪**
- https://github.com/tamaratran/fast-jev-compaction（MIT，30 commits，7220 星）
- https://github.com/tamaratran/jev-pruner（MIT，100+ commits）
- https://github.com/leonaaardob/fast-dev-compaction（MIT，36 commits）
- https://github.com/joelhooks/pi-fast-jev-compaction（MIT，8 commits）
- https://github.com/compozy/yoshi（MIT，27 星，未再更新）

**门控/安全/自动批准**
- https://github.com/leepokai/jev-guard（MIT，24 commits，0 依赖，npm）
- https://github.com/valentynkit/jev-belay（MIT，37 commits，71 测试文件）
- https://github.com/AskTheWay/dsh-jev-interceptor（MIT，14 commits，63 测试，npm）+ `docs/jev-usage-points.md`（13 个已验证决策点）
- https://github.com/BasmaAbouzied0/jev-auto-approve、https://github.com/BasmaAbouzied0/jev-secret-guard（各 3 commits、0 星——慎重）
- https://github.com/DanRWilloughby/snifftest（MIT，97 commits）
- https://github.com/ckorhonen/jev-lint（MIT，30 commits）
- https://github.com/codejunkie99/keel（MIT，10 commits/115 测试文件，Rust/GPUI，**形态最接近本产品**）

**检索/审查/监督**
- https://github.com/dzhng/jevgrep（MIT，100+ commits，124 测试文件）
- https://github.com/thruwire/foreman（MIT，30 commits）
- https://github.com/egma-ai/jev-code-reviewer（MIT，10 commits）
- https://github.com/NiazMorshed2007/jev-review（MIT，4 commits）
- https://github.com/devagrawal09/jev-review（MIT，**5 commits、0 测试、643 星**）
- https://github.com/reticlehq/reticle（Apache-2.0+FSL，100+ commits，1489 测试文件）

**记忆/技能**
- https://github.com/Dicklesworthstone/skillranker（MIT+rider，100+ commits，203 测试文件）
- https://github.com/shimo4228/jev-skill-router（MIT，11 commits，含反向结论）
- https://github.com/kitze/skillbox（MIT，11 commits）
- https://github.com/zilliztech/memsearch（MIT，2685 星）
- https://github.com/kitfunso/hippo-memory（MIT，766 星，含负面结果文档）
- https://github.com/yuyang2230/jev-agent-skill（MIT，0 星）

**协议/MCP/网关**
- https://github.com/jkudish/jev-mcp（MIT，78 commits，11 工具）
- https://github.com/vinilana/jev-gateway（MIT，74 commits，21 测试文件）
- https://github.com/itsmostafa/system-one-connector（MIT，100+ commits，Go，可切 Laya/CLM/d1）
- https://github.com/tacticocc/Jevbridge（MIT，9 commits，ACP+MCP）
- https://github.com/alterhq/typesafe-sdk-swift（Swift 6 SDK，未独立核实）

**Laya（开源免费替代）**
- https://github.com/NandhaKishorM/laya（Apache-2.0，28856 星）
- https://github.com/mizorewww/laya-coreml（Core ML/ANE，P50 4.98ms on M3 Max）
- https://github.com/mizorewww/laya-mlx（MLX，7–14ms）
- https://github.com/1Panel-dev/laya-server（Apache-2.0，兼容 `/v1/systemone`）
- https://github.com/receptron/laya（Node/ONNX，跨平台）
- https://github.com/zzhdbw/laya-Ascend（昇腾 NPU）

**价格/延迟/厂商文档（转引）**
- https://vercel.com/ai-gateway/models/jev（定价 $0.042/1M in，$0 out）
- https://docs.typesafe.ai/concepts/state、`/confidence`、`/concepts/use-case-map`、`/model-jaggedness/jev-1.13`
- https://x.com/dsqjaffa/status/2102054148925526206（Virlo 提问措辞对比，厂商自报）

**本产品内锚点（源码路径，仓库内相对路径）**
- `crates/switch-core/src/gateway/{mod,server,routing,auth,sse,timeouts}.rs`
- `crates/bridge/src/lib.rs`
- `crates/switch-core/src/usage/mod.rs`
- `crates/switch-core/src/{plugins,content}/`、`crates/switch-core/catalog/tools.json`

**未能核实**
- 上述任何收益数字的**独立第三方复现**：未找到。所有比例均来自项目作者自测或其自审报告。
- `alterhq/typesafe-sdk-swift`、`yuyang2230/jev-agent-skill`、`miuuyy/Astra-Ares` 的实际可运行性：仅凭元数据判断，未执行。
- 0xNatoshi/jev-codex-router **被归档的原因**：仓库无说明，README 未提及。
- 通用搜索通道（`/tmp/webtools/search.py`）本次返回 `backend=none`，WebSearch 供应商不支持，**未取得独立于 GitHub 的第三方文章**。本报告对"第三方"的标注仅限于仓库内引用他人来源的条目。
