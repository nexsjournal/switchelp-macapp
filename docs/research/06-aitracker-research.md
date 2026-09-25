# AITracker 调研：它追踪什么，以及它能不能「全网找免费 token / 额度」

日期：2026-09-25 · 对象：[estelwalks/aitracker](https://github.com/estelwalks/aitracker) · 方法：`git clone --depth 1` 后**静态阅读全部源码**（1074 个 TS/TSX 文件中的关键路径）+ GitHub API 元数据。**未安装、未运行该应用，未执行其任何安装脚本，未改动被检查仓库。**

关联：[参考项目调研](01-reference-projects.md) · [证据索引](../appendix/01-source-index.md)

---

## 结论先行

**问题一：这个 tracker 追踪什么？**
它追踪**你自己在 36 个 AI 编程工具里已经消耗掉的东西**——token 数与折算成本、使用趋势、按工具/项目/模型/行为的分布，外加从这些本地记录里再派生的 Skills、知识与长期记忆。数据源全是这些工具在**本机磁盘上留下的日志/数据库**，加上它自己的一份 SQLite 库（默认 `~/.aitracker`）。

**问题二：它能不能全网探索到可以免费使用的 token 或额度？**
**不能，而且这个项目里根本不存在这类能力的任何代码。** 我针对这件事做了定向否证（`scrape` / `crawl` / `harvest` / `free key` / `trial` / provider 注册表 / 搜索引擎 API 等关键词全库检索，并逐条枚举了全部出网调用），结论是确定的：

- 它**不是**「额度猎手」，而是「用量记账器」。两者方向相反——一个统计你已经花掉的，一个去找你还没花的。
- 它**连你自己的剩余额度都不统计**。全库没有任何 rate limit / 剩余配额 / 重置时间 / 5 小时窗口的读取逻辑（唯一命中的 `window` 全是日期范围的时间窗和 zstd 解压窗口）。它算的是「已消耗」，不是「还剩多少」。
- 它**不抓网页**。依赖表里没有 cheerio / puppeteer / axios / got 这类抓取或 HTTP 库，`playwright` 仅供 e2e 测试；全库出网调用可以完整枚举（见第 2 节），没有任何一条是去「找免费资源」的。

所以如果目标是想复刻一个「自动发现全网可白嫖 token/额度」的工具，**这个仓库没有任何可借鉴的实现**——它的价值在另一头：**36 个 AI 工具本地日志格式的适配清单**（第 1.2 节），那是这个项目最硬的资产。

**那「额度」这条路还有没有别的走法？有，但要先把三种「免费」分开。** 这一问的后续调研放在 [第 9 节](#9-补充调研2026-09-25合规获得免费-token--额度的可行方案)：其中一条实测发现值得先记在这里——**你自己的计划额度其实就在本机 Codex 日志里**（`rate_limits` 字段），AITracker 没读它，但它是可读的真实数据。

---

## 0. 快照与证据边界

| 项目 | 记录 |
| --- | --- |
| 仓库 | `github.com/estelwalks/aitracker` |
| 快照 commit | `9dafa381b83b59c3f717ecce9b1b121b21977596`（main，2026-09-22，`fix(usage): keep quick conversations distinct…`） |
| 版本 | `package.json` `1.0.4`，主版本 v1.0.0 发布于 2026-09-04，最新 Release v1.0.4（2026-09-11） |
| 技术栈 | Electron + TanStack Start + React + TypeScript；Node ≥ 24；`type: module` |
| 代码规模 | 1074 个 `.ts/.tsx/.mjs`；依赖 53 个运行时 + 20 个开发依赖；**316 个测试文件**；CI 三条 workflow（`ci.yml` / `release.yml` / `publish-npm.yml`） |
| 许可证 | 基于 GPL-3.0 的**自定义许可证，含两条额外限制**（见第 5 节） |
| 我的方法 | 只读静态阅读：源码、工具定义 JSON、价格规则包、隐私与许可证文本、GitHub API |
| **未验证** | 未安装/未运行应用；未实际观察界面；未验证 36 个 reader 在真实工具版本上的解析正确率；未验证 `ai.trusttools.cn` 服务的实际行为（只据代码与 PRIVACY.md 推断） |

---

## 1. 它追踪什么

### 1.1 追踪链路

`src/lib/tool-registry` 下是 36 份**声明式工具定义 JSON**，每份描述：探测位置、用量日志路径与 reader 名、Skills 根目录、会话恢复命令、是否可作安装目标。`manifest.json` 用固定顺序显式列出它们（注释明确「no runtime directory scanning」，不做运行时目录扫描）。

链路是：**定义 JSON → 本地扫描器按路径找文件 → 按 reader 解析成规范化的用量记录 → 落到本地 SQLite → 聚合成读模型给界面**。

关键点：**用量数据来自「事后读磁盘」，不是拦截流量。** `src/modules/monitoring` 里的“监控”指的是**应用自己后台任务的心跳**（说明见 2.2），全库没有进程枚举或流量嗅探——唯一的 `pgrep`/`Get-Process` 出现在 `electron/update-relaunch.ts`，用来重启 AITracker 自己。

以 Codex 为例，`codex.tool.json` 声明读 `~/.codex/sessions/**/rollout-*.jsonl` 与 `~/.codex/archived_sessions`，reader 为 `codex-rollout-v1`。Claude Code 读 `~/.claude/projects/**/*.jsonl`（`claude-rollout-v1`）。**这和本项目的做法同源**：都是把工具自己写的日志当数据源。

### 1.2 36 个工具与读取路径（证据表）

下表的路径与 reader 名是从各 `.tool.json` 直接提取的，`base` 缩写含义：`home`=用户主目录、`appData`/`appDataRoaming`/`configHome`/`dataHome`/`userProfile`=各平台的配置或应用数据目录。

| 工具 | 使用记录读取路径 | reader |
| --- | --- | --- |
| Claude Code | `~/.claude/projects/**/*.jsonl` | `claude-rollout-v1` |
| Codex | `~/.codex/sessions/**/rollout-*.jsonl`、`~/.codex/archived_sessions/…` | `codex-rollout-v1` |
| Cursor | `Cursor/User/globalStorage/**/*usage*.json`、`~/.cursor/**/*usage*.jsonl`、`tokscale/cursor-cache/**/*.json` | `generic` |
| Kiro | `Kiro/User/globalStorage/kiro.kiroagent/dev_data/devdata.sqlite` | `generic-sqlite` |
| Gemini CLI | `~/.gemini/tmp/**/chats/session-*.json` | `gemini-session-v1` |
| OpenCode | `opencode/storage/message/**/*.json`、`~/.opencode/**/*.jsonl` | `generic` |
| OpenClaw | `~/.openclaw/agents/*/sessions/**/*.jsonl*`、会话 sqlite 归档 | `openclaw-session-v1` |
| Every Code | `~/.code/sessions/**/rollout-*.jsonl` | `every-code-rollout-v1` |
| Hermes Agent | `~/.hermes/state.db`、`~/.hermes/profiles/*/state.db` | `generic-sqlite` |
| GitHub Copilot | `github-copilot/**/*usage*.jsonl`、`*.json` | `generic` |
| Kimi Code | `~/.kimi/sessions/**/*.jsonl`、`~/.kimi/logs/**/*.jsonl`、`~/.kimi-code/…` | `generic` |
| oh-my-pi | `~/.omp/agent/sessions/**/*.jsonl`、`~/.oh-my-pi/…` | `omp-session-v1` |
| CodeBuddy | `~/.codebuddy/projects/**/*.jsonl` | `codebuddy-log-v1` |
| WorkBuddy | `~/.workbuddy/projects/**/*.jsonl` | `workbuddy-native` |
| Grok Build | `~/.grok/sessions/**/updates.jsonl` | `grok-turn-v1` |
| Kilo CLI | （无用量路径） | — |
| Kilo Code | 20 条路径：VS Code / Cursor / Windsurf / VSCodium / Trae / Trae CN / CodeBuddy 等的 `…/kilocode.kilo-code/tasks/**/ui_messages.json` | `kilocode-task-v1` |
| Antigravity | `~/.gemini/antigravity*/**/.system_generated/logs/transcript.jsonl` | `antigravity-transcript-v1` |
| pi | `~/.pi/agent/sessions/**/*.jsonl` | `pi-session-v1` |
| Craft Agents | `~/.craft-agent/workspaces/**/session.jsonl` | `generic-jsonl` |
| Roo Code | `Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks/**/*.json` | `generic` |
| Zed Agent | `Zed/threads/threads.db` | `zed-threads-v1` |
| Goose | `goose/sessions/sessions.db` | `generic-sqlite` |
| Droid | `~/.factory/sessions/**/*.settings.json` | `droid-settings-v1` |
| Mimo Code | `mimocode/mimocode.db` | `generic-sqlite` |
| ZCode | `~/.zcode/cli/db/db.sqlite` | `generic-sqlite` |
| AnythingLLM Desktop | `anythingllm-desktop/storage/anythingllm.db` | `generic-sqlite` |
| DeepSeek Harness | `~/.dsh/sessions/**/session.jsonl(.zstd)`（含 zstd 或裸 jsonl） | `dsh-session-v1` |
| AiPy | `aipy-pro/aipy`（目录） | `generic-sqlite` |
| Cline | `Code/User/globalStorage/saoudrizwan.claude-dev/tasks/**/*.json` | `generic-json` |
| Qwen CLI | `~/.qwen/projects/**/*.jsonl` | `generic-jsonl` |
| Command Code | `~/.commandcode/projects/**/*.jsonl` | `generic-jsonl` |
| Proma | `~/.proma/agent-sessions/*.jsonl` | `generic-jsonl` |
| Qoder CN | `QoderCN/SharedClientCache/cache/db/local.db` | `generic-sqlite` |
| Reasonix | `~/.reasonix/sessions/**/*.jsonl` | `generic-jsonl` |
| Cherry Studio | `CherryStudio/(Data/Agents/)?…/.claude/projects/**/*.jsonl` | `generic-jsonl` |

两个可读的信号：

1. **读取层是「专用 reader + 通用兜底」两层**。命名 reader（`claude-rollout-v1`、`codex-rollout-v1`、`zed-threads-v1` 等）针对特定格式精解；`generic` / `generic-jsonl` / `generic-json` / `generic-sqlite` 是应对未知变体的通用解析。**这 36 份定义本身就是一份「AI 工具本地日志格式地图」**，是本项目最值得单独抄走的部分。
2. **覆盖面明显偏向中国与新兴工具**（Kimi、CodeBuddy、WorkBuddy、Qoder CN、Qwen、Reasonix、Proma、AiPy、Mimo、ZCode…），且维护者把 `~/.zcode` 也纳入了——说明作者熟悉中文 AI 工具生态。

### 1.3 「追踪」的字段与该领域定义

- **用量**：token 量 + **按离线价格包折算的成本**（见第 3 节）、按时间范围/工具/项目/模型的分布、环比（`computeMoM`）。
- **行为分类**：`_shared/usage-taxonomy.json` 定义 `debug-command / execution / browser / agent / planning / text-qa` 六类优先级，用命令关键词（`diff`/`grep`/`log`/`status`/`test`/`lint`）判定。
- **上下文维度**：`tools / skills / commands / mcp / toolOutputs`——即一次会话里消耗被什么占用（`codex-context-v1`、`claude-context-v1` 等 reader 负责）。
- **派生资产**：把选中的会话「蒸馏」成可复用资产（persona / memory / skill / prompt / brief），存 SQLite 的 knowledge 库；Skill 清单与分发（读写各工具的 skills 根目录）；会话列表与「打开对话」（调用该工具自己的 resume 命令）。
- **README 宣称的 8 项能力**：AI 用量分析、AI 工具分析、Skills 管理、配置管理、Skills 蒸馏、知识库、长期记忆、本地优先。

---

## 2. 「全网找免费 token / 额度」：逐项否证

### 2.1 全部出网调用清单（完整枚举）

所有出网都经过一个共享封装 `src/lib/http/external-request.server.ts` 的 `fetchExternal`，它只负责统一 User-Agent（`AITracker/<version> (Electron; +repo)`）。调用方穷举如下：

| 目标 | 用途 | 发送内容 |
| --- | --- | --- |
| `api.github.com/repos/estelwalks/aitracker/releases` | 版本更新检查 | 无业务数据 |
| `https://ai.trusttools.cn/api` | Skills 市场列表/下载、汇率刷新 | 只有目录参数（搜索词、标签、分页、排序）与「USD 兑 CNY/JPY/KRW 汇率」请求 |
| 用户自己配置的模型端点 | 蒸馏、增强洞察、报告摘要、安全扫描 | 该操作选中的文本；**用用户自己的 API Key** |

端点常量在 `src/lib/app-config.ts:56`（`MARKET_API_BASE`）。除上述之外，源码里出现的 `api.deepseek.com` / `api.openai.com` / `api.anthropic.com` **全部只是「用户可配置的模型 profile 预设端点」**，不是它自己调用的托管服务；`docs/`、`www.*` 等一批厂商域名则只出现在工具定义里当展示/安装链接。

**没有第四条出网路径。** 没有遥测、没有崩溃上报、没有分析 SDK——依赖表里没有 Sentry / analytics / crash-reporting 任何一类（53 个运行时依赖是 Radix / TanStack / 字体 / zod / UI 库）。

### 2.2 明确不存在的四类能力

| 被问到的能力 | 结论 | 证据 |
| --- | --- | --- |
| 抓取网页 / 爬虫找免费资源 | **不存在** | 关键词全库零命中；无 cheerio/puppeteer/axios/got；`playwright` 仅 devDependencies 供 e2e |
| 免费 Key / 试用额度 / 额度池的发现与聚合 | **不存在** | 无 provider 注册表、无搜索引擎 API、无站点解析；`trial`/`free key` 等零命中 |
| 读取「剩余额度 / rate limit / 重置时间」 | **不存在** | 全库无 rate-limit 读取逻辑；`quota` 命中的是它**自己**的蒸馏次数上限（`distillation/quota.ts`，默认 20 次/天，仅对真实模型调用计数） |
| 拦截进程流量 / 嗅探 prompt | **不存在** | `monitoring` 指的是应用自身后台任务心跳（`usage`/`skills`/`sessions`/`security`…）写 SQLite；无进程枚举、无代理、无抓包 |

补充一处**准确的修正**：`src/lib/security/daily-limit.ts` 里定义了 `DAILY_SCAN_LIMIT = 10`（安全扫描每天 10 次）和 `consumeDailyScan()`，但**这两个函数在源码里没有任何生产调用点**——只有它自己的 `daily-limit.test.ts` 引用。也就是说这个限流器目前是**未接线的死代码**，界面上不要指望它真的拦你。

---

## 3. 成本是怎么算出来的（离线价格包）

这是判断「它会不会为了实现计费而联网」的关键，也是它设计上最干净的一处：

- 模型单价是**离线规则包**，不是联网抓的。`src/lib/pricing/pricing-definitions.generated.ts`（由 `scripts/generate-pricing-imports.mjs` 生成，文件头写明 DO NOT EDIT）声明了 6 个 pack：`defaults`、`openai`、`anthropic`、`google`、`china-providers`、`tool-routing`，对应 `src/lib/pricing/rules/*.rules.json`。
- `dynamic.server.ts` 的注释直说：**「Model prices are offline rule packs (resolve.ts); this module only loads display-currency exchange rates.」** 唯一联网的是**汇率**，只为把美元成本显示成人民币/日元/韩元。
- 汇率读取是 **cache-only 优先**：README 与代码都说明「页面 loader 永不阻塞在网络」上，缓存过期也先用旧值，网络刷新由后台任务（`exchange.refresh`）做；失败则回落到内置基准汇率并标注 "fallback"。

**推论**：就算完全断网，它的成本数字依然能算——这既是本地优先的证据，也说明它**不需要**、因而也**没有**去发现任何外部资源的动机或代码。

---

## 4. 隐私与安全姿态（含一处要注意的例外）

`PRIVACY.md` 与代码基本自洽，姿态是「本地优先、功能触发式联网、无账号、无遥测」：

- **本地**：用量分析、Skills、规则、知识、记忆、偏好都在本地 SQLite（默认 `~/.aitracker`），不需要注册账号。
- **读取边界**：扫描器只存**规范化元数据与聚合**；原始对话内容不进渲染层读模型。用户在「数据源」页手选的 `tool_data_roots` 只用于指向本地目录，**绝不进摘要、快照、导出或任何网络请求**。
- **密钥**：存在本地加密 secret store，渲染层只拿 `apiKeyMasked`；安全扫描报告生成前会用 `sanitizeReport()` 把 API Key 从报告里擦掉。

**需要注意的两处例外**（不是缺陷，是使用/评估时要清楚的边界）：

1. **Skills 市场与汇率走维护者自营的中文服务 `https://ai.trusttools.cn/api`**。这是全部出网里唯一的「非 GitHub、非用户自配端点」目标，也是把「本地优先」打折扣的唯一一处。据 `PRIVACY.md` 与代码，它只收目录参数与汇率查询，但这意味着**该功能可用性、可用地区与隐私取决于该服务的运营方**，且它是闭源的外部依赖。
2. **「完整模式」安全扫描会把 Skill 文件内容发给你自己配置的模型供应商**。`electron/security-scanner-service.ts` 按有无模型配置决定 `quick`/`full`：quick 用本地静态规则引擎（`@estelwalks/agent-threat-scanner` 的 `mode:"quick"`，无需 Key、无需联网）；full 则把采集到的文件送到用户配置的端点——**花的是用户自己的 Key**，不是官方托管服务。采集范围被严格限制在 Skills 类文件（`SKILL.md`/`AGENTS.md`/`package.json` 及代码/文档扩展名，单文件 1MB、单 Skill 128 文件、总量上限），**不读聊天记录、数据库或个人文档**。

一处**工程细节值得记一笔**：`node-security-engine.server.ts:16` 用 `String.fromCharCode(115,99,97,110,83,107,105,108,108)`（即 `"scanSkill"`）拼出动态 import 的入口名，配合 `/* @vite-ignore */`。这看着像混淆，实质是**规避打包器的静态解析**以让该依赖在 Electron/SSR 两种图里正常动态加载。动机可以理解，代价是**这类调用无法被静态安全审计发现**——评估第三方 Electron 应用时，这是一类需要额外留意的模式。

---

## 5. 许可证：GPL-3.0 + 两条额外限制（复用前必读）

`LICENSE` 标题即写明 **"GPL-3.0-based License with Additional Restrictions"**，正文含两条：

1. **不得在未经版权方事先许可的情况下，把本项目作为 SaaS 提供给第三方。**
2. **不得在未经版权方事先许可的情况下，把本项目或其代码整合进用于销售或赠送的商业产品。**（需要商业授权时，联系方式按该仓库 LICENSE 第 2 条给的地址申请——这里不转载对方的邮箱）

对本项目的直接含义：**「借鉴设计思路」与「复制代码」是两件事**。36 个工具的定义 JSON 与 reader 解析逻辑如果直接搬进 Switchelp，会落在第 2 条上——Switchelp 是待发布的商业产品，需要先取得商业许可，否则应只参考其**字段与路径事实**（这些是工具的公开文件格式，事实本身不受版权保护），自行实现解析。GPL-3.0 本身的传染性对「整合进商业产品」同样构成约束。

---

## 6. 项目健康度

| 指标 | 值 | 读法 |
| --- | --- | --- |
| 创建时间 | 2026-08-28 | **极新，不到一个月** |
| 最近推送 | 2026-09-22 | 活跃 |
| Star / Fork | 114 / 17 | 早期小规模但有真实关注 |
| Commit | 577（main） | 一个月内 577 次提交，节奏很猛 |
| Release | v1.0.0（09-04）→ v1.0.4（09-11） | 已能出正式安装包 |
| 测试 | 316 个测试文件 + 3 条 CI workflow | 工程化程度**明显高于**其项目年龄 |
| 平台 | macOS / Windows 支持；Linux 的启动器不支持 | README 明确 Linux 不在支持范围 |

综合判断：**这不是玩具项目，是一个作者很熟练、工程纪律好的早期产品。** 但它一个月龄 + 577 提交意味着接口和数据结构仍在快速变动，若要跟踪其进展，应固定 commit 而不是跟 main。

---

## 7. 对 Switchelp 的参考意义

按价值排序，只有三条真正值得抄，且都能在不碰许可证的前提下转化为对 Switchelp 有用的事实：

1. **36 份工具定义 JSON = 现成的「本地日志格式地图」。** Switchelp 若要展示「用了多少」，路径与格式事实可直接据此对照（例如 Codex 的 `~/.codex/sessions/**/rollout-*.jsonl`、Claude Code 的 `~/.claude/projects/**/*.jsonl`）。**参考事实，自行实现解析**，规避许可证问题。
2. **`reader` 的两层设计（专用 reader + `generic-*` 兜底）是应对格式漂移的正确架构**。AI 工具日志格式变化频繁，Switchelp 如果做同类读取，值得照这个抽象分层，而不是为每个工具写死一条解析链。
3. **离线价格包 + 汇率单独刷新**，是「成本显示」不必联网的正确解。Switchelp 若有成本展示需求，可直接采用这个分工，避免把价格表做成联网依赖。

**方向上要明确区分**：AITracker 是**度量（你花了多少）**，Switchelp 是**路由与切换（用哪个供应商、Key 怎么生效）**——两者的数据可以互补，但**不重叠**，且「找免费额度」既不是前者的能力，也不应成为后者的目标：那类做法依赖绕过供应商的计费与配额策略，既不稳定也有合规风险，本项目不采用。

---

## 8. 未验证项与调研边界

- 未安装、未运行 AITracker，因此**所有界面结论均为源码推断**，未做视觉核对。
- 未在真实机器上验证 36 个 reader 对当前各工具版本的解析正确率；表格中的路径是定义文件所声明，不是解析成功的证据。
- `ai.trusttools.cn` 为闭源外部服务，其真实行为仅据代码与 `PRIVACY.md` 推断，**未实测**。
- 未验证「蒸馏」「安全扫描 full 模式」在真实供应商上的实际行为与成本。
- GitHub 的 Star/Fork 数与 Release 列表取自 API（2026-09-25 当日）；克隆为 `--depth 1`，故无历史与 tag，改动节奏依 README/Release 列表描述。
- **第 9 节另有边界**：WebSearch 在该环境下不可用，9.2 / 9.5 的来源全部是直接抓取官方文档与 GitHub API；抓不到的一律标注未核实（9.8 有清单）。免费额度是快照信息，随时会变。

---

## 9. 补充调研（2026-09-25）：合规获得免费 token / 额度的可行方案

第 2 节的结论是「AITracker 不能，也没有这条路」。本节回答**那还有什么路**。

### 9.0 先把三种「免费」分开，否则一定会谈歪

| 类别 | 是什么 | 可行性 |
| --- | --- | --- |
| **A. 你自己的计划额度** | 你已经买了官方计划（free / plus / pro），还剩多少、多久重置 | ✅ **本机就能读到**，见 9.1。这是唯一「真实、稳定、不违规」的额度来源 |
| **B. 厂商公开发布的免费额度** | 官方文档里写明的免费档、免费模型、免费试用金 | ✅ 合规可用，**前提是照它的条款、用你自己的账号与 Key**，见 9.2 |
| **C. 白嫖 / 共享 / 抓来的 Key** | 论坛或仓库里流出的 Key、批量注册试号、绕过限流 | ❌ **不做**。违规且高风险，见 9.6 |

本产品与本节的全部结论只落在 A 与 B。**C 不是「更激进的做法」，是另一个东西**——它的成本不在费用上，在账号、法律与安全上。

### 9.1 A 类：你的额度其实已经在本机日志里（本次实测发现）

这是本次调研里最有产品价值的一条，且与 AITracker 无关：

Codex 的 rollout JSONL 里，`token_count` 事件除用量外还带一个 **`rate_limits`** 字段。我全量扫了本机 **337 个会话文件里的 32424 个 `token_count` 事件**，其中 **2334 条带有可用的窗口记录**（`plan_type` 与 `primary` 都非空），形如：

```json
{ "limit_id": "codex",
  "primary": { "used_percent": 32.0, "window_minutes": 10080, "resets_at": 1779107671 },
  "plan_type": "free" }
```

按 `plan_type` 与窗口长度分布（同一次全量统计）：

| plan_type | 条数 | `window_minutes` |
| --- | --- | --- |
| `plus` | 2211 | 300（**5 小时**窗口） |
| `free` | 123 | 10080（**7 天**窗口） |

**也就是说「我的计划窗口用了多少、什么时候重置」是可以从本机文件里直接读出来的真实数据**，不需要任何联网、不需要任何 Key、不涉及任何第三方。第三方供应商的会话（如经本工具路由的模型）这些字段为 `null`，跳过即可，不推算。

顺带一个实现上的要点：**窗口长度不一定是天**。plus 账号实测是 5 小时窗口，只按天渲染会变成「窗口 0 天」——两种都要处理。

AITracker 没有用这个字段（第 2.2 节已证），本项目的用量页则把它做成了「本机计划额度」一张卡。**这是「额度」这件事唯一干净的解**：不猜、不抓、不外联，只把本机已有的真值显示出来。

### 9.2 B 类：厂商官方免费档（2026-09-25 核实）

以下为**逐条抓官方文档核实过**的（本次 WebSearch 不可用，全部靠直接抓取官方页面；凡抓不到的一律标未核实，不猜）：

| 提供商 | 免费内容 | 说明 |
| --- | --- | --- |
| **OpenRouter** | `:free` 模型变体；未买过额度时 **50 次/天**，累计购买 ≥10 credits 后 **1000 次/天**，20 次/分 | 官方有可读的额度接口（`GET /api/v1/key` 里的 `free_model_daily_requests`），OpenAI 兼容。**是本清单里「免费档 + 官方额度接口」配套最完整的** [文档](https://openrouter.ai/docs/api-reference/limits) |
| **Cloudflare Workers AI** | **10000 Neurons/天**（Free 与 Paid Workers 计划都有） | 提供 OpenAI 兼容入口 `/ai/v1` [定价](https://developers.cloudflare.com/workers-ai/platform/pricing/) · [兼容说明](https://developers.cloudflare.com/workers-ai/configuration/open-ai-compatibility/) |
| **Cohere** | 试用 Key：**1000 次调用/月**，chat 模型 20 次/分 | [速率限制](https://docs.cohere.com/docs/rate-limits) |
| **Hugging Face Inference Providers** | Free 账号 **$0.10/月**、PRO $2.00/月 额度 | OpenAI 兼容 `https://router.huggingface.co/v1` [定价](https://huggingface.co/docs/inference-providers/pricing) |
| **智谱 BigModel** | 多个明确标注「免费」的模型（GLM-4.7-Flash / GLM-4.5-Flash / GLM-4V-Flash 等） | [模型总览](https://docs.bigmodel.cn/cn/guide/start/model-overview) |
| **阿里云百炼（Qwen）** | 新用户**按模型各 100 万 token**、有效期 90 天、仅北京地域、不可跨模型合并 | [新用户免费额度](https://help.aliyun.com/zh/model-studio/new-free-quota) |
| **Fireworks** | $1 免费额度 | [定价](https://fireworks.ai/pricing) |
| **NVIDIA NIM** | 开发者计划的 NIM API，面向原型验证免费 | [NIM](https://developer.nvidia.com/nim) |
| **Google AI Studio / Gemini API** | 有 Free 档（无需绑卡，RPD 太平洋时间午夜重置） | **但限流页面已不再公布固定免费数字**，只指向 AI Studio 查看自己的限额 → 具体数字**未核实** [限流](https://ai.google.dev/gemini-api/docs/rate-limits) |
| **DeepSeek** | **没有免费档** | 定价页只有按 token 计费 [定价](https://api-docs.deepseek.com/quick_start/pricing) |
| **Moonshot / Kimi** | 文档中**没有**免费额度或免费模型 | 同时提供 OpenAI 与 Anthropic 兼容端点 [文档](https://platform.kimi.com/docs/guide/start-using-kimi-api) |

**一条重要变更，能让很多过期攻略失效**：**GitHub Models 已于 2026-07-30 完全退役**（模型广场、推理 API、BYOK 全部下线，文档改为引导去 Azure AI Foundry）。2024–2025 年「用 GitHub Models 白嫖」的说法现在全部是错的。[文档](https://docs.github.com/en/github-models/about-github-models)

**未核实（抓取被挡或页面无档位信息，不要照抄进产品）**：Groq（console 对自动化抓取返回 403）、Mistral（定价页现在讲的是 $10/月 Free plan，不再是老的手机号验证免费档）、Cerebras、SambaNova。

### 9.3 真正的「无限免费」只有本机模型

想不限量、又不碰任何条款，答案只有一个：**自己的机器**。

- **Ollama** 暴露 OpenAI 兼容子集 `http://localhost:11434/v1/`（`/v1/chat/completions`、`/v1/models`、`/v1/embeddings`、`/v1/responses`），本地 Key「需要但被忽略」。 [文档](https://docs.ollama.com/api/openai-compatibility)
- LM Studio / llama.cpp（`llama-server`）/ vLLM 也提供同类 OpenAI 兼容端点（本次未逐个复验）。

**老实话**：它「无限」是指在**不被计量**这层意义上无限，不是「免费拿到前沿模型」。吞吐与能力受你自己的 GPU / 内存限制，消费级笔记本上实际能跑的是小模型或重度量化模型；大模型要么显存不够，要么慢到没法交互。对 Switchelp 这类工具，本机模型的价值是「离线兜底与零成本试验」，不是「替代付费 API」。

### 9.4 面向开发者/学生的免费计划

- **Google AI Pro 学生版**：美国 18 岁以上在校生免费 1 年，需资格验证 + 支付方式，兑换截止 **2026-12-31**，到期自动续费 $19.99/月。注意这是 **Gemini 应用订阅，不是 API 额度**。 [页面](https://gemini.google/students/)
- **GitHub Student Developer Pack**：含 Copilot Student、**$100 Azure 额度**、Codespaces 等。 [页面](https://education.github.com/pack)
- 云厂商创业额度计划、Kaggle / Colab 等本次未复验，**按未核实处理**。

### 9.5 目录型项目：可以「照单查」，但都有保质期

- **cheahjs/free-llm-api-resources** 是这件事的长期参考仓库，**但 2026-09-25 通过 GitHub API 取该仓库返回 404**（改名还是删除**未核实**）。仍在维护的同类/衍生：`jtig37/free-llm-api-resources`、`nherx/free-llm-api-resources`、`CYBIRD-D/FREE-LLM-API-Provider`，以及星标较高的 [abbosaliboev/free-ai-bible](https://github.com/abbosaliboev/free-ai-bible)。
- 它们**是「目录」而不是「通道」**：只汇总官方公开信息，不持有 Key、不代理请求。可以拿来做人工核对的起点，但它们自己都写着「限额随时会变」，**精度不足以直接喂给产品**。
- [Artificial Analysis](https://artificialanalysis.ai/) 做模型与价格的独立对比，[Helicone](https://www.helicone.ai/) 是网关/可观测工具（7 天试用），都不是免费额度目录。

### 9.6 C 类：为什么不做（事实，不是道德说教）

- **条款**：OpenAI ToS 明确禁止共享账号凭据、程序化抽取数据、转售服务 [ToS](https://openai.com/policies/terms-of-use/)；OpenRouter 也直接说明多开账号/多造 Key 没用，因为「容量是全局治理的」 [文档](https://openrouter.ai/docs/api-reference/limits)。批量注册试号违反几乎所有提供商的 ToS 与反滥用政策。
- **你用的可能正是别人被偷的凭据**：GitGuardian 统计 **2024 年公开 GitHub 仓库新增硬编码密钥 23,770,171 条**（同比 +25%），且 **2022 年泄露的密钥到 2025 年仍有 70% 有效** [报告](https://www.gitguardian.com/state-of-secrets-sprawl-report-2025)。这类 Key 往往同时躺在未知第三方手里，也可能本来就来自信息窃取木马。
- **工程后果**：任务跑到一半 Key 被吊销、账号被封、账单与欺诈责任落到你的出口 IP 上；而接收并使用他人凭据本身就有法律风险，与主观是否知情无关。

结论：**它不是一条「捷径」，而是一条随时会断、且可能把用户拖进法律与安全问题的路**，不该进任何产品的设计文档。

### 9.7 产品上怎么做：两种架构，选 (a)

| 方案 | 做法 | 取舍 |
| --- | --- | --- |
| **(a) 随包/可刷新的本地目录 + 用户自己的 Key** | 应用内置一份带版本号、可后台更新的 JSON（端点、`:free` 模型 id、文档里写的限额、官方文档链接），Key 由用户自己填进系统凭据库 | ✅ 不托管用户密钥、不担代理责任、不碰 ToS、可离线、成本低<br>❌ 会过期（GitHub Models 在 2026-07-30 下线就是活例），要有人维护约十几家；必须明写「限额以各提供商自己的文档为准」 |
| **(b) 云端聚合代理** | 后端持 Key 并转发请求（AITracker 用的那类市场服务也是这个形态） | ✅ 永远最新、用户一个凭据就能用<br>❌ 运营方要合法取得并管理每一把 Key、还要留在各家 ToS 内（转售/代理常被禁止），一次政策变更或封禁潮就崩；用户提示词过第三方（隐私成本）；且这种形态天然吸引 9.6 那类用法 |

对 Switchelp 这种「管理 Codex / OpenAI 兼容供应商」的本地工具，**(a) 是站得住的默认**：目录随包发、可刷新，Key 必须用户自己的，每个条目都附上「限额看官方文档」与 ToS 链接，让过期只是**一次数据更新**而不是一次发版。若以后要做 (b)，必须是**用户显式同意的可选项**，不能是主路径。

### 9.8 本节未核实清单

- Google Gemini 免费档的具体 RPM/RPD 数字；Groq、Mistral、Cerebras、SambaNova 的免费档细节（页面不可自动抓取）。
- 云厂商创业额度、Kaggle/Colab 的现行条款。
- LM Studio / llama.cpp / vLLM 的兼容端点细节（只按既有认知记录，未逐个复验）。
- 免费额度**随时会变**：上表所有数字都是 2026-09-25 的快照，进产品前必须重新核对。

---

## 附：证据索引（关键文件）

| 结论 | 定位 |
| --- | --- |
| 工具定义与顺序 | `src/lib/tool-registry/definitions/manifest.json`，36 份 `*.tool.json` |
| 单工具用量路径示例 | `definitions/codex.tool.json`（`capabilities.usage`）、`definitions/claude-code.tool.json` |
| 出网统一封装 | `src/lib/http/external-request.server.ts` |
| 市场/汇率服务地址 | `src/lib/app-config.ts:56`（`MARKET_API_BASE`） |
| 离线价格包 | `src/lib/pricing/pricing-definitions.generated.ts`、`rules/*.rules.json`、`dynamic.server.ts`（注释：价格是离线包） |
| 自身蒸馏配额（非免费额度） | `src/modules/distillation/quota.ts`（默认 20/天） |
| 未接线的扫描限流 | `src/lib/security/daily-limit.ts`（`DAILY_SCAN_LIMIT = 10`，无生产调用点） |
| 安全扫描采集边界 | `electron/security-scanner-service.ts`、`modules/security-assessment/adapters/local-skill-monitor.server.ts` |
| 动态 import 入口名混淆 | `modules/security-assessment/adapters/node-security-engine.server.ts:16` |
| 隐私声明 | `PRIVACY.md` |
| 许可证额外限制 | `LICENSE`（第 1、2 条） |
