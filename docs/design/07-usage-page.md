# 用量页（Usage）：功能范围、契约与版面规范

版本：v1 · 日期：2026-09-25 · 状态：**已实现**（Rust 核心 + Tauri 命令 + 前端页面；解析算法在本机 337 个 Codex 会话文件上实测过）

关联：[基础规范](01-foundations.md) · [组件规范](03-components.md) · [页面与流程](04-pages-and-flows.md) · [模板与交互规则](05-patterns-and-accessibility.md) · [扩展板块](06-tool-hub-plugin-hub-and-content-center.md) · 背景调研：[AITracker 调研](../research/06-aitracker-research.md)

> 本页解决一个问题：**用户想知道自己的 Codex 把量花在哪了、以及本机计划额度还剩多少。** 数据全部来自 Codex 自己在磁盘上留下的会话记录，不联网、不上传、不新增任何采集。

---

## 1. 范围

### 1.1 v1 做什么

| 能力 | 说明 |
| --- | --- |
| 时间范围 | 近 7 天 / 近 30 天 / 近 90 天（分段控件） |
| 四个指标 | 总 Token、输入 Token、输出 Token、缓存读取 Token |
| 每日趋势 | 每日总 Token 柱状图（手绘 SVG，单序列） |
| 按模型 | 模型 / 会话数 / 输入 / 输出 / 缓存读取 / 合计 |
| 按供应商 | 供应商 / 会话数 / 合计 Token |
| 本机计划额度 | 计划类型、窗口已用百分比、窗口长度、重置时间 |
| 扫描范围 | 如实显示扫描目录、文件数与不可读文件数 |

「按供应商」这一栏是本页与通用用量工具的区别所在：Switchelp 的价值就是路由，用户需要直接看到**官方订阅**与**经本工具路由的供应商**各占多少。本机实测的供应商取值有 `OpenAI`、`gptswitch`、`opencodex`、`custom` 等多种，直接按记录原值展示，不做归并猜测。

### 1.2 v1 明确不做

| 不做 | 原因 |
| --- | --- |
| **成本 / 金额估算** | 本工具的模型是用户自定义的第三方模型，**没有可靠的单价来源**。按 [04 页面与流程](04-pages-and-flows.md)「不展示没有可靠来源的余额、成功率和 Token 节省统计」，宁可只给 token 真值，也不给估算出来的钱。以后若引入用户自填单价，再单开一期。 |
| 剩余额度 / 余额 | 本地日志里只有**计划窗口的已用百分比**（`rate_limits`），没有剩余次数或余额字段。有多少给多少，不推算。 |
| Codex 之外的 AI 工具 | 本工具只管 Codex。其他工具的日志格式不在本期范围（可参考 [AITracker 的 36 份工具定义](../research/06-aitracker-research.md)）。 |
| 「全部时间」范围 | 会让每日柱状图跨度失控（需要按周聚合）。range 固定 7/30/90，避免图表与合计口径不一致。 |
| 成本/用量的历史持久化 | 每次进入页面按需扫描本地文件；不建表、不复制数据，符合「尽量不存数据」的既有取向。 |

## 2. 数据源与解析算法

数据源：Codex 的 rollout JSONL。

- 活跃会话：`<codexHome>/sessions/**/rollout-*.jsonl`
- 归档会话：`<codexHome>/archived_sessions/**/rollout-*.jsonl`（实测本机 78 个）
- `codexHome` 取 `CODEX_HOME` 环境变量，回落到 `~/.codex`。解析后的实际目录会回给界面（「扫描范围」一行）。

有效行（实测结构）：

| 行 `type` | 取用字段 |
| --- | --- |
| `session_meta` | `payload.session_id`、`payload.cwd`、`payload.model_provider`、`payload.timestamp` |
| `turn_context` | `payload.model`（模型名来自这里，**不是** session_meta） |
| `event_msg` + `payload.type = "token_count"` | `payload.info.total_token_usage.*`、`payload.rate_limits.*` |

### 2.1 关键算法：对累计值做差分，不要累加 `last_token_usage`

`token_count` 事件里同时有 `total_token_usage`（会话内累计）与 `last_token_usage`。**本机 337 个文件实测：`last_token_usage` 会重复计数**——在 110 个文件里「累加 last」显著大于最终累计值（例：最终 135386，累加 last 得 202636）。因此在 42% 的文件上，按 `last` 聚合会把数字算大。

正确做法：**对每个字段，用相邻两次 `total_token_usage` 的差**（第一次以 0 为基准）作为该事件的增量：

```text
delta_i = total_i − total_{i−1}        （逐字段，prev 初值 0）
```

依据：本机 259 个活跃文件里，`total_token_usage` 的**每个字段都单调不减**（负差分计数全为 0；加上 78 个归档文件共 337 个，同样为 0）。累计值单调时，差分自动求和等于最终累计值，既不会重复计数，又能按每个事件的时间戳与当时的模型/供应商正确分摊到「天 / 模型 / 供应商」三个维度。

**四个指标之间不可相加推算**，这一点实测过两次，两次都不是「想当然」：

- `cached_input_tokens ⊂ input_tokens`、`reasoning_output_tokens ⊂ output_tokens`（包含关系，不是并列）。
- `total_tokens` **也不等于** `input_tokens + output_tokens`：本机 32366 个 `token_count` 事件里有 **2878 个（约 9%）两者不等**，差异多为 ±1~7。最初只抽查前 80 个文件时得到「无例外」，全量核对后该结论被推翻——所以界面与契约注释都不假设它们能互相推算，四个指标卡并列显示并写明包含关系。

### 2.2 计划额度（`rate_limits`）

`token_count` 事件的 `payload.rate_limits` 里可能带计划窗口信息。本机全量实测：337 个会话文件里的 32424 个 `token_count` 事件中，**2334 条带有可用窗口记录**（`plan_type` 与 `primary` 都非空）——其中 `plus` 2211 条（窗口 300 分钟 = 5 小时）、`free` 123 条（窗口 10080 分钟 = 7 天）：

```json
{ "plan_type": "free", "primary": { "used_percent": 32.0, "window_minutes": 10080, "resets_at": 1779107671 } }
```

取**最后一个非空**的 `rate_limits` 作为当前窗口展示（按事件时间戳取最新，不是按文件扫描顺序）。`primary` / `plan_type` 为 null 的记录（第三方供应商的会话）跳过。**一个非空记录都没有时不显示这一卡**，改为一行说明（计划额度只有官方计划账号的 Codex 会话才会写入），而不是显示 0%。

**窗口长度不一定是天**：本机实测 `plus` 是 **5 小时**窗口（300 分钟）、`free` 是 7 天窗口。所以窗口文案必须按时长分支：≥ 1440 分钟显示天，否则显示小时——写死「{days} 天」会渲染成「窗口 0 天」。

## 3. 契约（冻结）

Rust：`crates/switch-core/src/usage/mod.rs`，serde `rename_all = "camelCase"`。

```rust
pub struct UsageTotals { input_tokens, cached_tokens, cache_write_tokens,
                         output_tokens, reasoning_tokens, total_tokens: u64 }

pub struct UsageDay      { date: String, sessions: u64, totals: UsageTotals }
pub struct UsageModelRow { model: String, sessions: u64, totals: UsageTotals }
pub struct UsageProviderRow { provider: String, sessions: u64, totals: UsageTotals }

pub struct UsagePlanWindow { plan_type: String, used_percent: f64,
                             window_minutes: i64, resets_at: i64 }

pub struct UsageReport {
  source_directory: String,
  range_days: i64,
  scanned_files: u64,
  unreadable_files: u64,
  sessions: u64,
  totals: UsageTotals,
  daily: Vec<UsageDay>,          // 范围内逐日零填充，升序
  by_model: Vec<UsageModelRow>,  // 按 total 降序
  by_provider: Vec<UsageProviderRow>,
  plan_window: Option<UsagePlanWindow>,
}
```

命令：`usage_report(days: i64) -> Result<UsageReport, CoreError>`，`days` 收 7/30/90，非法值回落到 30。

TS 镜像：`src/contracts/types.ts` 的 `UsageReport` 等；客户端方法 `DesktopClient.usageReport(days)` → `invoke('usage_report', { days })`。

## 4. 页面结构与版面

模板：本页是「概览 + 数据表」的组合，按 [05 模板](05-patterns-and-accessibility.md) 取 T04 数据表 的骨架，外加一行指标卡。

```text
用量                                    ← 壳层 h1.text-page-title
在 Codex 本地会话记录上统计 Token 用量      ← 壳层 hint（page.usageHint）
──────────────────────────────────────
[近 7 天 | 近 30 天 | 近 90 天]           ← SegmentedTabs，role=tablist
┌────────┬────────┬────────┬────────┐
│ 总 Token │ 输入    │ 输出    │ 缓存读取 │  ← 指标卡 ×4，grid 等宽
└────────┴────────┴────────┴────────┘
┌─ 每日 Token 用量 ────────── 会话 N 个 ┐
│  ▁▃▅█▆▃▁  (SVG 柱状图)                │
└──────────────────────────────────────┘
┌─ 本机计划额度 ──────────────────────┐
│ 计划 free · 已用 32% · 窗口 7 天      │
│ ▓▓▓▓▓▓░░░░░░░░ 重置 2026-05-18 20:34 │
└──────────────────────────────────────┘
┌─ 按模型 ────────────────────────────┐
│ 表：模型 / 会话 / 输入 / 输出 / 缓存 / 合计│
└──────────────────────────────────────┘
┌─ 按供应商 ──────────────────────────┘
│ 表：供应商 / 会话 / 合计               │
└──────────────────────────────────────┘
扫描范围：~/.codex（337 个文件，0 个不可读）  ← 证据行
```

版面硬要求（沿用既有规范）：

- 页面容器 `.page { display: grid; gap: 24px; align-content: start }`，卡片 `.card`（`--border-subtle` 描边、`--radius-card` 圆角、`--bg-surface` 底、24px 内边距四边相等）。
- 指标卡一行四列：`grid-template-columns: repeat(4, minmax(0, 1fr))`；**网格单元要两层才等宽等高**（外层 `li` flex，内层 `flex: 1`），同排文案压在一行内。
- 窄窗口（≤960px）指标卡降到两列。
- 趋势图为**单序列**柱状图（总 Token），填充 `var(--accent)`，`role="img"` + `aria-label`，每根柱 `<title>` 给出「日期 · Token 数」；不引入任何图表依赖。
- 额度进度条用 `var(--accent)` 填充、`--border-subtle` 做轨道；已用百分比取整数显示（**不显示小数**，避免无意义的精度）。
- 数字用 `--font-mono`（该栈已含中文回退，见 [基础规范](01-foundations.md)）。
- **数值列的表头必须跟数字同向对齐**：单元格右对齐，表头也要右对齐（`text-align: right`），否则「输入」两个字贴在列左、数字贴在列右，中间空一大段（实测同一列右边缘最多差 156px）。表头仍用正文字体，不跟着换成等宽栈——它是标签不是数字。
- 交互反馈只走 Toast；页面级状态（加载/空/错误）用内联条。

状态矩阵：

| 状态 | 表现 |
| --- | --- |
| 加载中 | 骨架/内联「正在扫描本地会话…」，不闪空态 |
| 无任何会话 | EmptyState：说明扫描目录 + 「用 Codex 跑一次对话后这里就会有数据」 |
| 有会话但范围内无数据 | 正常渲染指标卡（全 0）+ 内联提示「近 N 天没有记录，试试更长的时间范围」 |
| 有文件不可读 | 正常渲染 + 证据行如实写「N 个不可读」 |
| 扫描失败 | 内联错误条 + 重试按钮，Toast 报错 |

## 5. 规范遵循清单（2026-09-25 逐条核对）

| 条目 | 结果 |
| --- | --- |
| 分段控件用 `SegmentedTabs`（role=tab，不是 button） | ✅ 实测 `role="tab"` ×3，`aria-selected` 随点击变化 |
| 表头 `th[scope=col]` 必须写 | ✅ 审计脚本 `columnHeadersWithoutScope: 0`（两张表 12 个表头） |
| 无正的 `tabindex`；焦点环交给全局 `:focus-visible` | ✅ `positiveTabindex: 0`；全局 `:focus-visible` 规则存在 |
| i18n 两套 key 完全一致并登记守卫 | ✅ `usage` 前缀进 `SOURCE_PREFIXES`，`nav.usage` 进 `DYNAMIC_KEYS`；`src/i18n.test.ts` 与 `src/i18n.locale.test.tsx` 均通过 |
| 数字只来自 `UsageReport`，界面不出现硬编码数字 | ✅ 组件里没有任何字面量数字，紧凑/完整两种记法都由 `usagePolicy.ts` 从报告值算出 |
| 网格卡片两层才等宽等高 | ✅ 1440（4 列）与 960（2 列）下四张卡都是 110 高、等宽 |
| 徽标与状态色只走 policy 映射、不在组件里内联判断 | **不适用**：本页没有徽标。唯一的语义色是证据行里「N 个不可读」的琥珀色，条件是 `unreadableFiles > 0`（在组件），颜色在样式表里（`--status-warning`），没有第二个状态需要映射 |
| hover 底色用 `::before` 外扩 | **不适用**：本页没有可 hover 的整行元素（表格行不可点），按钮沿用全局 hover |

**刻意不做**的两条，理由见 1.2：不显示成本（无可靠单价来源）；不把四个指标加总（真实数据上 `total ≠ input + output`）。

## 6. 验收

1. `cargo fmt --check`、`cargo clippy`、`cargo test`、`npm run typecheck`、`npm run test`、`npm run build` 全绿。
2. 本机 337 个真实文件上，页面合计 = 逐文件差分求和（去重后口径），与抽样手工核对一致；有专门单测覆盖「`last_token_usage` 重复计数时结果仍正确」这一条。
3. 浏览器实测（冷启动 `?theme=light|dark`）跑 `scripts/in-page-audit.js`：`overflow` / `overlap` / `clipped` 为 0，无 `positiveTabindex`，表头均有 `scope`，指标卡与表头文字对比度过 AA；结果写入 `docs/audits/`。
4. 最小窗口 960×640 下指标卡降为两列且不溢出。

**验收结果（2026-09-25）**：以上四条全部达成，证据在 [用量页审计与真实数据核对](../audits/2026-09-25-usage-page-design-conformance.md)。两套主题在 1440×900 与 960×640 下几何/对比度/点击目标/语义全为 0；解析结果与一份独立 Python 实现在本机 337 个真实文件上逐项一致（337 文件 / 210 会话 / 六个字段 / 五个供应商分组全同）。

## 7. 已知边界与后续项

| 边界 | 说明 |
| --- | --- |
| **共存模式下托管 profile 的用量可能不计入** | `resolve_codex_home()` 按 `CODEX_HOME` → `~/.codex` 解析单个根目录。共存模式的托管 home 在应用数据目录下，本进程 `CODEX_HOME` 没指过去时那部分用量不会进统计。页面上的「扫描范围」会如实显示实际扫的目录，所以现象可见，但**数字会偏小**。要真正修掉需要同时扫两个 home（检测到的实例各自的 `config_root`），属独立一期。 |
| 只有 Codex | 别的 AI 工具日志格式不在本期范围（可参考 [AITracker 的 36 份工具定义](../research/06-aitracker-research.md)）。 |
| 「全部时间」范围 | 会让每日柱状图跨度失控（需要按周聚合），range 固定 7/30/90。 |
| 成本/金额 | 见 1.2：没有可靠单价来源就不给数字。若以后引入用户自填单价，再单开一期。 |
| `SegmentedTabs` 的 tabindex | 组内三个页签都可 Tab 到（非 roving tabindex），是共用组件的既有行为，四个页面一致；要改需单独立项。 |
