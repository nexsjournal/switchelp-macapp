# 05 · 决策模型接入落地方案与接口契约（Jev / 本地 Laya / 借道既有供应商）

**角色**：architect。本文件里的类型、字段名、阈值与动作矩阵是**唯一契约**，实现照抄；要改先改这份文件。
**日期**：2026-09-30 · **状态**：待评审（本文件不含任何已落地代码）· **行数上限**：560 行
**范围**：把「决策模型」接进既有的四个板块（网关、用量页、内容中心、插件中心）。**不含**：改任何 `contracts/**` 或代码文件；读会话内容的门控；自动改配置/自动切模型。

**事实来源**：调研笔记 `docs/research/jev/01…06`（下称 **R1–R6**）；`docs/architecture/01…05`；页面规格 `design/06` `07` `09`；代码事实以本文件给出的**文件与行号**为准（2026-09-30 只读核对）。引用调研数字沿用原档位（【官方】【仓库/代码】【第三方文章】【推测】），**项目自述不当作实测**。

> **2026-09-30 修订（用户决定 + 社区实测，见 [调研 08](../research/08-community-playbook-and-fun-features.md)）**
> 1. **默认后端改为本地 Laya**（§1.2 的 ②），借道既有供应商（①）降为**在线兜底**，托管 Jev（③）只在用户自带 Key 时出现。§5 的 S4 顺序相应改为 ②→①→③。
> 2. **新增 `DecisionKind::Gate`（闸门：高把握放行 / 低把握转交）**，与 `GatewayRoute`（换模型）严格区分：闸门有公开实测支撑（>0.90 档 255/255 全对，0.80 阈值放行 89% 只错 1 个）；路由有 59%–184% 的反向实测 ⇒ `GatewayRoute` 维持"默认 shadow、不自动切换"，闸门才是要上线的那个用途。
> 3. 新增三个玩法作为 P1 候选：**模型擂台**（本地裁判给多供应商回答打分）、**出站体检**（正则硬挡 + 灰区问模型）、**校准小游戏**（与 §5 S4 的校准夹具合并入口）。
> 4. 校准门槛不变但更强：官方承认校准只到群体级；社区实测里出现"最错的答案置信度最高 0.86"⇒ 阈值只能自测，界面永远写"把握/概率"。

## 0. 本方案依赖的现状（已只读核对）

| 事实 | 位置 |
| --- | --- |
| 网关请求路径 `/i/{instanceId}/c/{catalogRevision}/v1/…`；alias → 固定 `routeRevision+credentialVersion+protocolId` | `crates/switch-core/src/gateway/server.rs:326 handle_inference`；`gateway/routing.rs:73 RequestRoute` |
| 上游错误**已有**规则表：401/403→ModelPermissionDenied、404→NotFound、429→upstreamRateLimited、400–499→upstreamRejected、其余→upstreamFailed | `gateway/server.rs:1080 upstream_status_error`（流中途：`upstream_stream_error`） |
| 诊断事件仅有 20 个白名单键，是**唯一**准入清单 | `diagnostics/mod.rs:30 ALLOWED_METADATA_KEYS` |
| 用量解析：对 `total_token_usage` 逐字段差分；产出 totals / daily / by_model / by_provider / plan_window | `usage/mod.rs:140 UsageReport`、`:303 ParsedFile`、`:316 parse_rollout_file` |
| 计划额度窗口已解析（`plan_type / used_percent / window_minutes / resets_at`） | `usage/mod.rs:131 UsagePlanWindow` |
| 扩展板块四张表 + `HubStore`（JSON 载荷 + 索引列）；schema 当前 **v4** | `storage/hub.rs`、`storage/migration.rs:12` |
| 插件预览阶段已能拿到 `SKILL.md` 正文与来源 commit | `plugins/skill.rs:20 SkillDocument.body`、`plugins/source.rs:37 RepoSkill` |
| 内容条目是本地快照，含 `title` / `summary` | `content/mod.rs:226 FeedItem` |
| 写类命令返回 `ExecuteResult { operationId }`；契约类型**手工同步**（无生成器） | `src-tauri/src/commands.rs:48`；`architecture/04-data-and-contracts.md` 开头 |
| 更新包 7,698,412 B（7.34 MiB）、sidecar 707,312 B、`minimumSystemVersion: 12.0` | `target/release/bundle/macos/`、`src-tauri/tauri.conf.json` |

> **前置未实现能力（不许当既有能力用）**：`ResponseBinding`（`response_id_digest/routeRevision/credentialVersion/expiry`）只有设计、**未实现**（`docs/README.md`「与文档的已知偏差」）；`/v1/responses/compact` 返回 404；Windows 凭据 helper 是桩（网关起不来）；Windows 不产更新签名产物。凡依赖它们的都在 §5 标「前置」。

## 1. 总体架构

### 1.1 模块落点

新增核心模块 `crates/switch-core/src/decision/`；其余改动是既有模块的加字段/加分支，不建平行体系。

| 路径 | 职责 | 复用 |
| --- | --- | --- |
| `decision/mod.rs` | 公开类型、`DecisionService`（编排）、`DecisionPolicy`（阈值→动作） | 只用既有 `CoreError`；**不依赖 Tauri** |
| `decision/state.rs` | `DecisionState` 枚举、`GatewayStateFacts`、`StateBuilder`（唯一构造函数） | 字段来源是 `RequestRoute` + 请求 JSON 的元数据键 |
| `decision/question.rs` | 六个用途的问题模板（编译期常量）+ `DECISION_PROMPT_REVISION` | 模板在代码里，不进库、不改即不过期 |
| `decision/backend/{mod,systemone,byo}.rs` | `DecisionBackend` trait；`/v1/systemone` 客户端（**托管 Jev 与本地 Laya 共用**，只换 base_url 与鉴权）；借道既有供应商 | `ureq`（已有依赖）、`credentials::CredentialResolver`、`protocols::chat` |
| `decision/meter.rs` | 决策记录落库与聚合（P0-3） | `storage/hub.rs` 新增表（schema v5） |
| `decision/attribute.rs` | 上游错误归因（P0-4）：规则表 + 灰区 | 由 `gateway/server.rs` 调用 |
| `usage/insight.rs` | P0-1 离线成本洞察（纯函数 + 只读报告） | `usage/mod.rs` 的解析结果 |
| `src-tauri/src/commands.rs` | 新增 8 个命令（§3） | 沿用 `run(window, state, …)` 包装 |
| `src/features/usage/` | 成本洞察卡 + 决策计量块 | `UsagePage.tsx` 既有骨架 |

### 1.2 三个后端、同一契约

| | ① 借道用户已有供应商（默认） | ② 本地 Laya（隐私档） | ③ 托管 Jev（BYO-Key） |
| --- | --- | --- | --- |
| 线协议 | `POST {base}/chat/completions`（OpenAI 兼容，**走既有 `chat_completions.v1` 适配器**） | `POST http://127.0.0.1:11435/v1/systemone`（TypeSafe 线协议；`ollaya`、`1Panel/laya-server` 都实现它，R3 §④） | `POST https://api.typesafe.ai/v1/systemone`，**非 OpenAI 协议，必须单开一条透传路径** |
| 鉴权 | 既有 Keychain 通路 | 本地无鉴权或本地 Key | 用户自带 TypeSafe Key（MCA §2.4 要求凭据保密不共享 ⇒ **不得预置共享 Key**，R1 §⑤） |
| 结构化输出 | `response_format: json_schema`（声明支持时）／否则 `json_object` + 严格校验 | 原生 `answers/probabilities/confidence` | 原生 |
| 校准置信度 | **拿不到**（LLM token 概率≠校准概率，R6 §6.4） | 需自行温度拟合、按「问题类型×选项数」分桶（R2 §⑦） | 官方 `confidence`（choice/score）；**noul 无 `confidence`** |
| 分发体积 / 成本 | 0；$0–0.27/天（1 万次，R6 §6.4） | ONNX INT4 **262.4 MB** 或首次下载；$0 | 0；输入 $0.042/Mtok、输出免费；官方口径约 **$0.0399/1 千次**（950 tok/次，R1 §③） |
| 中国大陆可达性 | 用户自己的供应商，通常最好 | 完全离线 | Cloudflare 非中国节点，直连「能通但抖动大」⇒ **不可作关键路径**（R1 §⑨【推测】） |

**三者在上层不可区分**：`DecisionService::decide()` 只认 `DecisionBackend`。`BackendCapabilities { native_confidence, noul_confidence, max_options, local }` 决定哪些用途可用——**能力不足时该用途直接不开放**，而不是降级后假装有置信度（反面案例：R6 §6.4 的「语义不等价」）。

### 1.3 数据流（文字版）

```text
[A] 网关决策路径（P0-2 / P0-4；state 只含元数据，不读会话内容）
宿主 Codex
  → 本机网关(127.0.0.1:18765) POST /i/{inst}/c/{rev}/v1/responses
  → RequestGuard(auth) → GatewayRouter.admission(prefix, alias) → RequestRoute   ← 既有，不改
  → ★决策点（新增，handle_inference 内、取凭据之前）
       StateBuilder::build(route, payload, facts)  ← 只取白名单键（§2.3）
       ├─ 规则先短路：错误归因(status/code) / 上下文分桶 / 配额窗口 → 确定性结论
       ├─ 需要模型时才调 DecisionService（超时 200 ms，失败 fail-open）
       ├─ 输出：PolicyAction{Allow|Deny|Ask|Abstain} + 建议 alias（enforce 且已钉住时才生效）
       └─ 无论结果如何都写一条 DecisionRecord（shadow 也写）
  → 用（可能被钉住/未变的）alias 与原本的 route 继续：凭据解析 → 适配器 → 上游   ← 既有链路
  → 上游非 2xx → upstream_status_error()（既有规则表）→ ★灰区才问模型 → 归因标签

[B] 离线洞察路径（P0-1；零网络、零新采集）
~/.codex/**/rollout-*.jsonl
  → usage::collect_usage(codex_home, days, now)                ← 既有差分算法，不改
  → ★usage/insight.rs（新增，纯函数）
       反事实成本（需用户填单价）｜ 异常用量日（稳健 z）｜ 任务形态归类（不看内容）
  → UsageInsightReport → usage_insight 命令 → 用量页新卡片（全部标「估算」）

[C] 内容中心与插件中心（P1-7 / P1-8；只发公开文本，用户可关）
feed_items(title+summary) / RepoSkill.document.body(SKILL.md 公开原文)
  → DecisionState::TextBlock{ kind: FeedItem|PluginSkillDoc, text: 截断 }
  → 相关性 Noul（阈值 0.60） / 安全预检多问（≥0.80 拦、0.30–0.80 问人）
  → 失败/超时/低于阈值 ⇒ 回退时间序 / 回退「询问用户」，永不空列表、永不静默安装
```

### 1.4 明确不参与的地方

- **Bridge 不做语义决策**：它处理 `model/list`、`thread/list` 与按线程归属，不是「这一轮该用哪个模型」（R3 §④）。决策服务不改 `crates/bridge/`。
- **不改既有路由规则**：一个 alias 仍对应一个 `(providerId, modelId)`；决策**只能选已发布的 alias**，不能造新 alias、不能跨供应商（`architecture/03` §1、ADR-003/005）。
- **不做假开关**：命令不存在或后端未连通时，界面显示「未配置」而不是可点的开关（`docs/README.md` 批次 E 的既有立场）。

## 2. 接口契约（实现照抄本节）

### 2.1 核心类型

```rust
// crates/switch-core/src/decision/mod.rs —— 全部 serde(rename_all = "camelCase")
pub enum DecisionKind {          // 用途，闭枚举；每个用途一份问题模板与阈值
    GatewayRoute,        // P0-2 网关路由（只选已发布 alias）
    UpstreamAttribution, // P0-4 上游错误归因
    UsageInsight,        // P0-1 用量洞察（**本地纯规则/本地模型，永不出网**）
    FeedRelevance,       // P1-7 内容中心相关性
    PluginSafety,        // P1-8 插件安装前安全预检
    QuotaFallback,       // P1-6 配额感知回退
}
pub enum DecisionMode { Shadow, Enforce }             // 默认 Shadow；enforce 逐用途独立
pub enum DecisionBackendKind { ByoProvider, LocalLaya, HostedJev }

pub struct DecisionRequest {
    pub decision_id: String,          // ULID，调用方生成，用于回滚与对账
    pub kind: DecisionKind,
    pub state: DecisionState,         // §2.3；**唯一**可进模型的载荷
    pub questions: Vec<DecisionQuestion>,
    pub policy: ActionPolicy,         // 阈值与 fail 语义，由用途决定，不由模型决定
    pub mode: DecisionMode,
    pub timeout_ms: u64,              // 硬超时（§2.5）
}

pub enum DecisionQuestion {           // 与 R1 §2.2 的三个 primitive 一一对应
    Choice { key: String, instructions: String, options: Vec<String> }, // 选项 ≤255【官方】
    Score  { key: String, instructions: String, levels: Vec<String> },  // 2–10 级【官方】
    Noul   { key: String, instructions: String },                       // 布尔
}

pub enum AnswerValue { Choice(String), Score(u8), Noul(f64) }           // Score 是 0 基索引，配 legend；Noul 是 P(true)

pub struct DecisionAnswer {
    pub key: String,
    pub value: AnswerValue,
    pub confidence: Option<f64>,     // choice/score 才有；noul 恒 None（官方不返回）
    pub confidence_source: ConfidenceSource,
    pub probabilities: BTreeMap<String, f64>, // 已归一化（§2.4）
    pub legend: BTreeMap<String, String>,     // Score 的级别文字，原样透传
}

pub enum ConfidenceSource {
    Reported,          // 决策模型原生 confidence
    DerivedPeakedness, // noul：2×|p−0.5|（Noul 无 confidence，这是本项目推导，**必须标注**）
    ArgmaxOnly,        // 借道供应商：只有 argmax，无校准 ⇒ 不参与门控（§2.4）
}

pub struct DecisionUsage { pub input_tokens: u64, pub output_tokens: u64 } // 上游原值；缺失记 0 并由 degraded 标明，不伪造估算

pub struct DecisionOutcome {
    pub decision_id: String,
    pub kind: DecisionKind,
    pub mode: DecisionMode,
    pub backend: DecisionBackendKind,
    pub backend_model: String,        // 响应里的 model（如 jev-1.13.0）；固定版本号记进记录（R1 §⑤）
    pub answers: Vec<DecisionAnswer>,
    pub action: PolicyAction,         // 由 action_matrix 决定，**不是**模型给的
    pub decision_rule: String,        // 命中的规则标识，如 "attribution.rule.status_401"
    pub usage: DecisionUsage,
    pub estimated_cost_micros: u64,   // 本地 = 0
    pub elapsed_ms: u64,
    pub cached: bool,
    pub degraded: Option<DegradeReason>,
}

pub enum PolicyAction { Allow, Deny, Ask, Abstain }   // Abstain = 不动作、走原路径
/// 后端返回的**原始**形状；由 `DecisionService` 归一化后才是 `DecisionOutcome`
/// （`action` 由代码决定、`degraded` 由调用方填、`estimated_cost_micros` 由计量层算）。
pub struct DecisionResponse {
    pub backend_model: String,   // 响应里的 model（如 jev-1.13.0），固定版本记进记录
    pub answers: Vec<DecisionAnswer>,
    pub usage: DecisionUsage,
}
pub enum DegradeReason {
    Disabled, Timeout, Unreachable, RateLimited, CircuitOpen,
    MalformedOutput, LowConfidence, NoCalibratedConfidence, CapabilityMissing,
}

pub struct Thresholds { pub act: f64, pub ask: f64 }  // 默认 act=0.80, ask=0.30（§2.4）
pub struct ActionPolicy {
    pub thresholds: Thresholds,
    pub fail_mode: FailMode,
    pub allow_enforce: bool,          // false = 该用途永远只出建议（shadow）
}
pub enum FailMode { Open, Ask }       // 失败时：不动作／问用户。**没有 Deny**（模型不可用 ≠ 内容有害）
```

### 2.2 决策后端 trait

```rust
pub trait DecisionBackend: Send + Sync {
    fn kind(&self) -> DecisionBackendKind;
    fn capabilities(&self) -> BackendCapabilities;
    /// 同步实现；调用方负责 spawn_blocking 与超时（与 content 模块同一约定）。
    fn evaluate(&self, request: &DecisionRequest) -> Result<DecisionResponse, BackendError>;
}
pub struct BackendCapabilities {
    pub native_confidence: bool, // 决策模型原生校准概率
    pub noul_confidence: bool,   // 是否给 Noul 置信度（Jev/Laya 都是 false）
    pub max_options: usize,      // Jev 255；本地 Laya 建议 ≤20（R2 §⑦）
    pub local: bool,             // true = 不出网（计量与 UI 单列）
    pub max_state_bytes: usize,  // 由用途的裁剪预算决定（§2.3 规则 4）
}
pub enum BackendError { Unreachable, Timeout, Unauthorized, RateLimited, Http(u16), Malformed(String) }
```

- 借道供应商的**渲染规则**：`state` + `questions` 渲染成一条 `system`（含 `DECISION_PROMPT_REVISION` 与「只输出 JSON」）＋ 一条 `user`（state 的规范 JSON + 问题表）；**不把 state 放进 tool 调用参数**（既有适配器的工具链路是给宿主用的）。返回体必须是 `{"answers":{key:{"choice"|"score"|"noul"|"confidence"}}}`，缺字段或越界即 `Malformed` → 动作 `Abstain`。
- `HostedJev` 与 `LocalLaya` 共用一个客户端：请求体 `{"state":…,"model":…,"questions":{…}}`，响应读 `answers` / `usage` / `model`；**不得复用 OpenAI 兼容代码路径**（R1 §⑩.1）。

### 2.3 state 白名单（本节的清单是闭集，多一个字段都不许）

```rust
pub enum DecisionState {
    /// 网关与归因用：**只含非内容元数据**，结构体闭集，禁止 #[serde(flatten)] 与自由 Map。
    Metadata(GatewayStateFacts),
    /// 内容/插件用：只有两种来源的公开文本，闭枚举，会话内容在这里**不可表达**。
    TextBlock { kind: TextBlockKind, text: String, truncated: bool, characters: usize },
}
pub enum TextBlockKind { FeedItem, PluginSkillDoc }
```

**允许进 state 的字段（`GatewayStateFacts`）**

| 字段 | 类型 | 来源与规则 |
| --- | --- | --- |
| `schema_version` | u8 | 常量，模板升级时 +1 |
| `alias` | String | 当前请求 alias（`gs/p_a/m_1`），即 `RequestRoute.alias` |
| `catalog_revision` / `instance_id` | String | 既有前缀与 `RequestRoute` |
| `protocol_id` | String | `responses.v1` / `chat_completions.v1` |
| `provider_id` | String | `RequestRoute.provider_id`（UUID，**不含展示名**） |
| `endpoint_host` | String | 仅主机名：小写、去 port、无 path/query/userinfo（派生） |
| `context_bucket` | enum | `Le8k / K8To32 / K32To128 / Gt128`，由请求体字节数估算，**不解析正文** |
| `tool_names` | Vec<String> | 本轮声明的 function/custom 工具名，排序去重，最多 32 个 |
| `streaming` | bool | 请求是否 `stream: true` |
| `turn_index` | u32 | 同一 `chain_digest` 链上的第几轮（进程内计数） |
| `chain_digest` | Option<String> | `previous_response_id` 的 sha256 前 16 位（**只存摘要**，用于钉住与缓存） |
| `error` | Option<UpstreamErrorFacts> | `status_code`、`class`（4xx/5xx/transport）、`upstream_code`、`upstream_type`、`message_excerpt`（≤120 字符、已脱敏截断）、`retry_count` |
| `quota` | Option<QuotaFacts> | `plan_type`、`used_percent`（取整）、`window_minutes`、`resets_in_minutes`（由 `UsagePlanWindow.resets_at` 换算）、`source: "rate_limits"` |
| `session_totals` | SessionTotals | 已完成轮数、累计输入 token 的 10^k 分桶、上一次归因结论的**分类** |
| `candidates` | Vec<RouteCandidate> | `alias`、`declared_context_limit`、`declared_output_limit`、`reasoning_efforts`、`declared_modalities`（全部来自已发布 `RouteEntry`，即既有快照） |
| `pinned_alias` | Option<String> | 当前链已钉住的 alias（§2.7） |

**禁止进 state 的字段（违反即视为缺陷，不是配置项）**：prompt / instructions / input / output / 工具调用参数 / 工具结果 / 文件内容与文件名 / `cwd` 与仓库路径 / Codex rollout 原始行 / Authorization、cookie、任何 Key 或令牌 / 完整 endpoint URL（含 path、query）/ 上游响应正文（只允许结构化错误字段与 ≤120 字符脱敏摘要）/ 图片音频 PDF 的任何描述 / 账号标识（邮箱、plan 账号 id、org id）/ 其他会话的任何字段 / **任何自由文本兜底槽位**。

**构造规则（`StateBuilder::build`，唯一入口）**

1. 只从 `RequestRoute`、请求 JSON 的**元数据键**、既有快照与进程内计数取值；不接收整份请求体以外的任何来源。
2. `error.message_excerpt` 必须先过 `redact(text, secret)`（既有函数，`gateway/server.rs`），再截断 120 字符，并去掉换行。
3. `context_bucket` 只按 `request.body.len()` 分桶；**不解析 token**、不引入 tokenizer。
4. 单用途预算：`{GatewayRoute, UpstreamAttribution, QuotaFallback} ≤ 4 KB；FeedRelevance ≤ 8 KB（title+summary 截到 600 字符/条）；PluginSafety ≤ 32 KB（`SKILL.md` 正文截到 24,000 字符，超出即 `truncated=true` 并在界面写明「按截断后的内容判断」）`。这条预算与 R1 §2.4 的 32k/64k 上限（【官方】）留足两个数量级余量。
5. **守卫测试（P0-2 的验收核心）**：把一份真实 rollout 样本与一份含密钥的合成请求喂进 `StateBuilder`，断言生成的 state JSON 里不出现样本里的任何 prompt 子串、不出现 Key 子串、不含禁用键名。这条测试与实现同批落地，先失败后通过。

### 2.4 阈值与动作矩阵（阈值只写在代码里，绝不问模型）

**置信度的三种来源必须分开算**，并写进记录：

| 来源 | 算法 | 说明 |
| --- | --- | --- |
| `Reported`（choice/score） | 官方 `confidence` 原值 | 先用 `probabilities` **归一化到和为 1**（官方示例之和 ≠ 1），再记 `confidence`；**不自己重算** |
| `Reported` 缺失但给了 `probabilities` | `(n×maxp − 1)/(n − 1)`，`n` = 选项数 | 官方三选项 `(3p−1)/2` 的推广（R1 §2.3【官方】）；`n=2` 退化为 `2p−1` |
| `DerivedPeakedness`（noul） | `p ≥ 0.5 ? 2p − 1 : 1 − 2p`（峰度，范围 0–1） | Noul 不带 `confidence`（【官方】），这是本项目推导，界面上必须与官方 confidence 视觉区分 |
| `ArgmaxOnly`（借道供应商） | 无 | **不参与门控**：该后端下 `allow_enforce=false` 的用途照旧，其余用途降为建议 |

| 动作带 | 条件 | GatewayRoute | UpstreamAttribution | FeedRelevance | PluginSafety | QuotaFallback |
| --- | --- | --- | --- | --- | --- | --- |
| 高 | `c ≥ act`（默认 0.80） | enforce 且已钉住 → 切 alias；否则记录 | 采用归因标签 | 视为相关、参与排序 | **拦下**（不安装），列出命中的问题项 | 只建议（默认）／开着自动切换时按钉住规则切 |
| 中 | `ask ≤ c < act`（0.30–0.80） | 不动，写「本来会选 X」 | 采用标签但标「灰区」 | 沉底但**不删** | **询问用户**（列出原文片段与得分） | 只建议，附「重置时间」 |
| 低 | `c < ask`（0.30） | `Abstain` | 取更保守的那一档（见下） | 保持时间序，不排序 | `Ask`（同中带：**不静默安装**） | 无动作 |
| 无置信度 | `ArgmaxOnly` 或 `CapabilityMissing` | `Abstain`（该用途在借道后端下不开启） | 仅规则结论 | 不排序 | `Ask` | `Abstain` |

- 归因在低置信/无置信时**只能落在更保守的分类上**：优先 `unknown`，其次是能由确定性规则支持的分类（§2.4.1）。
- 「低置信度占比」是计量指标，**不是错误率**（§4.2）；界面文案不许写成准确率。

#### 2.4.1 归因规则表（P0-4；规则先短路，模型只处理灰区）

| 观测 | 归因 | 动作 |
| --- | --- | --- |
| 401/403，或 `error.type ∈ {authentication_error, invalid_api_key}` | `auth` | 采用；恢复入口复用 `MODEL_PERMISSION_DENIED` 文案 |
| 429 且 `error.code ∈ {insufficient_quota, billing_hard_limit, quota_exceeded}` | `quota` | 采用；复用既有「临时 cooldown」姿态，**不给未经证实的余额** |
| 429 且 `error.code = rate_limit_exceeded`（纯限速，不表示余额耗尽） | `upstream` | 采用；按既有「临时 cooldown」处理 |
| 404 且 `endpoint_host` + `protocol_id` 组合表明 base_url 少了 `/v1` 或路径重复 | `protocol_translation` | 采用；给出字段级建议（复用 `architecture/03` §4 的 base_url 提示口径） |
| 400/422 且 `message_excerpt` 命中既有已核实模式（`role 'developer' is not allowed`、`tool type 'web_search' is not supported`、`max_tokens` 字段名不支持） | `protocol_translation` | 采用 |
| 5xx / 超时 / DNS / TLS / 连接被拒 | `upstream` | 采用 |
| 其余 400/422（无结构化 `code`/`type`） | **灰区** | 问模型（超时 200 ms）；失败即 `unknown` |
| 响应非协议体（200 HTML / 登录页） | `protocol_translation` | 采用；提示 URL/代理 |
| 无法判定 | `unknown` | 只展示原始错误，**不猜** |

**一致性守卫**（必须写成单测）：模型给出的归因若与 HTTP 状态矛盾（如无 401/403 却给 `auth`），一律**降级为 `unknown`**，并把这次降级计入计量（`degraded=MalformedOutput` 的一个子类）。规则表的每条至少两份合成错误夹具；已核实模式必须带出处注释（`architecture/03` §3 的兼容表）。

### 2.5 超时、失败与熔断

| 用途 | 硬超时 | FailMode | 失败时的用户可见结果 |
| --- | --- | --- | --- |
| `GatewayRoute` | 200 ms | `Open` | 请求照原路径转发；只记一条 `degraded` 记录，**不错误提示**（失败对用户不可见是刻意的：不阻断） |
| `UpstreamAttribution` | 200 ms | `Open` | 归因显示 `unknown` + 原始错误 |
| `FeedRelevance` | 500 ms | `Open` | 回退时间序；状态行不改色（不因筛选失败把页面打成错误） |
| `PluginSafety` | 3,000 ms | `Ask` | **不安装**，弹「模型未能判断，请自行确认」并展示原文片段 |
| `QuotaFallback` | 200 ms | `Open` | 只给建议；自动切换（若用户开启）本次不执行 |
| `UsageInsight` | 不适用 | — | 纯离线规则；不调用任何后端 |

- 重试：**最多 1 次**，只对 `Unreachable/Timeout/RateLimited`，遵守 `retry-after`；4xx 一律不重试。
- 熔断：连续 3 次失败 → 该后端在 **5 分钟**内不再被调用（`CircuitOpen`），状态在设置页如实显示；熔断不影响 `FailMode` 的语义。
- **fail-open 是硬要求**（R3 §④：该生态所有可跑项目的一致做法）：决策服务永远不能让一次 Codex 请求失败。唯一的 fail-closed 姿态是 `PluginSafety` 的「询问用户」——**不安装**不等于报错。

### 2.6 shadow 模式的数据形态

- `DecisionMode::Shadow` 下：**不改变任何转发字节**；`DecisionRecord{ mode: Shadow, applied: false, action, suggested_alias }`。
- **开启 enforce 的前置**：该用途至少累计 `SHADOW_MIN_RECORDS = 200` 条 shadow 记录，且「低置信度占比 ≤ 40%」；不满足时 `decision_set_mode(kind, "enforce")` 返回 `Conflict` 并在界面写清差多少条。
- shadow 记录与 enforce 记录**同一张表**（用 `mode` 列区分），界面默认只显示 enforce 口径并在聚合行标注「含 N 条 shadow 记录（未生效）」。
- 验收硬指标：同一请求在 shadow 开/关下，上游收到的字节**逐字节一致**（§5 S2 的集成测试）。

### 2.7 会话钉住规则（避免 prompt cache 失效）

背景：按轮次换模型有**明确的反向实测**（R3 §③B：Jev 选子代理比单个 Sol-xhigh 贵 69.7%），`switchboard` 的正面做法是「选一次就钉住整个会话」（R3 §②）。

1. **钉住键** = `chain_digest`（`previous_response_id` 的摘要）。首轮没有该字段 ⇒ **允许决策但只写 shadow**：无法证明是同一会话时，不许切换。
2. 同链后续轮：`chain_digest` 命中既有 pin ⇒ **直接使用钉住的 alias**，跳过模型调用（记为 `cached=true`，不重复计在线成本）。
3. 新链（不同 `chain_digest`）⇒ 允许重新决策；旧 pin 在 **30 分钟**无引用后清理。
4. pin 只存内存（`Mutex<HashMap<String, PinEntry>>`，容量 512，LRU 淘汰）；**不落盘**：会话标识落盘没有收益、只有泄露面。若将来要跨重启保持，只落 `chain_digest`。
5. pin 不改变既有路由规则：`credential_version` 与 `catalog_revision` 的固定逻辑原样生效（`CONTINUATION_BOUND` 的实现**是前置**，见 §5 S2 注）。
6. 每用途独立 pin 表？——**不**。pin 是「这条链用哪个 alias」，与用途无关；多个用途对同一链的建议若冲突，取**优先级最高的 enforce 用途**（PluginSafety > GatewayRoute > 其余），并记一条 `Conflict` 事件。

### 2.8 决策缓存键

```text
cache_key = hex(sha256("decision.v1\n" + kind + "\n" + backend_kind + "\n" + backend_model
                       + "\n" + prompt_revision + "\n" + questions_digest + "\n" + state_digest))[..32]
state_digest     = hex(sha256(canonical_json(state)))[..32]   // 键排序、无空白
questions_digest = hex(sha256(canonical_json(questions)))[..32]
prompt_revision  = DECISION_PROMPT_REVISION（常量，改模板即整体失效；systemone 后端记 "wire.v1"）
```

| Scope | 存放 | TTL / 失效 | 用于 |
| --- | --- | --- | --- |
| `Ephemeral` | 进程内 LRU（≤256 条） | 10 分钟 | 网关/归因（同一请求重试、同链重复轮） |
| `Persistent` | `decision_records` 表（按 `cache_key` 唯一） | 内容条目 30 天；插件 **`repo@commit:dir_name` 变化即失效** | 内容筛选、插件预检 |

缓存命中：`cached=true`、`estimated_cost_micros=0`，计量里单列 `cache_hits`（§4.2）。**`state_digest` 是缓存与计量的唯一主键口径**，state 原文一律不落库。

## 3. IPC 与前端契约

命令名沿用既有下划线风格（`usage_report` / `plugins_install` / `tools_state`），全部返回 `Result<T, CoreError>`，参数用 camelCase 由 Tauri 自动转换。

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `usage_insight` | `days: i64` | `UsageInsightReport` | P0-1；`days` 收 7/30/90，非法值回落 30（复用 `clamp_range_days`） |
| `decision_status` | — | `DecisionStatus` | 三后端配置与健康 + 六用途的 `enabled / mode / shadowRecords / lowConfidenceRatio / enforceBlockedKey` |
| `decision_set_enabled` | `kind: String, enabled: bool` | `DecisionStatus` | 总开关；关即 `Disabled`（不调后端、不写记录） |
| `decision_set_mode` | `kind: String, mode: String`（`shadow`/`enforce`） | `DecisionStatus` | enforce 未达 §2.6 门槛返回 `Conflict` |
| `decision_backend_save` | `draft: DecisionBackendDraft, expectedVersion: Option<i64>` | `DecisionStatus` | `DecisionBackendDraft { kind, provider_id?, model_alias?, base_url?, credential_id?, model_name?, api_key_secret? }`（Key 只进 Keychain、不进 DB，沿用 `credentials` 通路） |
| `decision_backend_test` | `kind: String` | `DecisionProbeReport` | **发一条真实的最小决策请求**；失败必须回真实原因，禁止「配置成功」 |
| `decision_records` | `kind: Option<String>, limit: i64, cursor: Option<String>, includeShadow: bool` | `{ records, nextCursor }` | 原始记录（默认只返回 enforce） |
| `decision_meter` | `days: i64` | `DecisionMeterReport` | P0-3 聚合（§4.2） |

```rust
// 命令的入参与返回（字段名 snake_case；经 serde rename_all 后给前端的是 camelCase）
pub struct DecisionStatus {
    pub backends: Vec<DecisionBackendStatus>, // 三后端各自配置与健康
    pub kinds: Vec<DecisionKindStatus>,       // 六用途各自开关、模式与计数
    pub prompt_revision: String,
    pub circuit_open_until: Option<i64>,
}
pub struct DecisionBackendStatus {
    pub kind: DecisionBackendKind, pub configured: bool, pub backend_model: Option<String>,
    pub last_ok_at: Option<i64>, pub last_error: Option<String>, pub consecutive_failures: u32,
}
pub struct DecisionKindStatus {
    pub kind: DecisionKind, pub enabled: bool, pub mode: DecisionMode,
    pub shadow_records: u64, pub enforce_records: u64,
    pub low_confidence_ratio: Option<f64>,
    pub enforce_blocked_key: Option<String>, // 未达 §2.6 门槛时的文案键（差多少条/占比多少）
}
pub struct DecisionProbeReport {             // decision_backend_test：一条**真实**请求的结果
    pub ok: bool, pub backend_model: Option<String>, pub elapsed_ms: u64,
    pub input_tokens: u64, pub answers: Vec<DecisionAnswer>, pub error: Option<String>,
}
pub struct DecisionMeterReport {             // 与 §4.2 的表一一对应
    pub range_days: i64,
    pub decisions: u64, pub decisions_local: u64, pub decisions_online: u64,
    pub input_tokens: u64, pub input_tokens_local: u64,
    pub estimated_cost_micros: u64, pub cache_hits: u64, pub applied: u64,
    pub low_confidence: u64, pub low_confidence_ratio: Option<f64>,
    pub degraded: std::collections::BTreeMap<String, u64>, // Disabled/Timeout/CircuitOpen/…
    pub by_kind: Vec<DecisionKindMeter>,
}
pub struct DecisionKindMeter {
    pub kind: DecisionKind, pub decisions: u64, pub applied: u64,
    pub estimated_cost_micros: u64, pub low_confidence_ratio: Option<f64>,
}
```

TS 侧（**手工同步**，与既有约定一致：Rust snake_case ↔ TS camelCase，见 `architecture/04` 开头）：

- DTO 加到 `src/contracts/types.ts`（与 `UsageReport` 同一节末尾）；客户端方法加到 `src/desktop/client.ts`（接口 + 实现），转发加到 `src/desktop/transport.ts`（形如 `usageInsight: days => call<UsageInsightReport>('usage_insight', { days })`，与 `:145 usageReport` 同款）。
- 文案键加 `src/locales/{zh-CN,en}.ts`，并把 `'decision'` 与 `'insight'` 登记进 `src/i18n.test.ts` 的 `SOURCE_PREFIXES`；动态键（`decision.action.*`、`decision.kind.*`、`attribution.*`）登记进 `DYNAMIC_KEYS`——不登记会在界面出现裸键名（既有守卫会拦）。
- **事件：P0 不需要新增**。所有结果都是拉取式。仅当 P1-8 预检在真机上稳定超过 3 秒时，才新增 `decision://progress`（payload `{ decisionId, kind, phase: "started"|"done", elapsedMs }`），并只在 `transport.ts` 一处 `listen`；沿用既有「窗口重开先拉快照再订阅」的口径（`architecture/04` §5）。

### 3.1 P0-1 洞察报告的类型与算法（离线、零网络、纯函数）

```rust
pub struct UsageInsightReport {
    pub range_days: i64,
    pub price_book: PriceBookState,          // Empty ⇒ 界面不出任何金额（只出 token 差）
    pub counterfactual: Vec<CounterfactualRow>,
    pub anomalies: Vec<AnomalyDay>,
    pub shape_buckets: Vec<ShapeBucketRow>,
    pub generated_at: i64,
}
pub enum PriceBookState { Empty, Filled { entries: usize, updated_at: i64 } }
pub struct CounterfactualRow {
    pub from_model: String,                  // 来自 UsageReport.by_model 的真实模型
    pub baseline_model: String,              // 用户指定的「基准档」（默认：已发布目录里最便宜的模型，可改）
    pub input_tokens: u64, pub cached_tokens: u64, pub output_tokens: u64,
    pub estimated_savings_micros: Option<i64>, // PriceBook 为空 ⇒ None
    pub assumption_kind: String,             // 常量 "history_replayed_on_baseline"
}
pub struct AnomalyDay { pub date: String, pub total_tokens: u64, pub robust_z: f64, pub shape: AnomalyShape }
pub enum AnomalyShape { ModelConcentrated, CacheHitDropped, SessionsSpiked, Unknown }
pub struct ShapeBucketRow { pub shape: UsageShape, pub sessions: u64, pub totals: UsageTotals }
pub enum UsageShape { LongContext, ToolHeavy, ShortQa, CacheHeavy, Unknown }
```

1. **反事实**：对 `UsageReport.by_model` 的每行，把 `input/cached/output` 代入基准档单价重算（`savings = 历史价 − 基准价`）；**只在同一供应商内换档**——跨供应商会改变数据发送边界，不做。`estimated_savings_micros` 为负时如实显示为负值（不吞掉）。
2. **异常日**：对 `daily[].totals.total_tokens` 取中位数 `M` 与 MAD；`robust_z = (x − M) / (1.4826 × MAD)`；判异常必须**同时**满足 `robust_z > 3.5`、`x > 2M`、`x > 20_000`；`days < 7` 或 `MAD = 0` 时不出任何异常结论（样本不足如实说明）。不使用决策模型（R4 §U3：未找到任何成熟先例）。
3. **形态归类**：只用 `provider / model / cached_ratio = cached÷input / io_ratio = output÷input / 当日会话数` 四个量按固定阈值分桶（如 `cached_ratio ≥ 0.6 → CacheHeavy`、`input 分桶 ≥ 32k 且 io_ratio ≤ 0.02 → LongContext`、`sessions ≥ 5 且 io_ratio ≥ 0.10 → ToolHeavy`）。阈值在实现期用本机真实数据定标并写进单测，**卡头必须写「按请求形态归类，不读会话内容」**。
4. 界面必须出现的三条假设：① 假设历史请求走基准档能得到同样的输出；② 输出 token 用历史实测值代入、不重算；③ 换档会重新冷启一次 prompt cache（首次请求按全额计费）。缺任一条即视为把估算渲染成了实测（`design/07` §1.2 的既有立场、R3 §③C 的失信先例）。

## 4. 计量与数据

### 4.1 落库（`HubStore` 新增一张表，schema **v4 → v5**）

```sql
-- 与 feed_*/tool_* 同风格：JSON 载荷 + 少量索引列。迁移只能往后追加（migration.rs 规则）。
CREATE TABLE decision_records (
  decision_id      TEXT PRIMARY KEY NOT NULL,
  cache_key        TEXT NOT NULL,
  created_at       INTEGER NOT NULL,
  kind             TEXT NOT NULL,   -- 六个 DecisionKind
  mode             TEXT NOT NULL,   -- shadow | enforce
  backend          TEXT NOT NULL,   -- byo_provider | local_laya | hosted_jev
  backend_model    TEXT NOT NULL,
  local            INTEGER NOT NULL,-- 1 = 本地推理（Laya），0 = 出网
  applied          INTEGER NOT NULL,-- 是否真的改变了行为
  action           TEXT NOT NULL,   -- allow | deny | ask | abstain
  confidence       REAL,            -- NULL = 无置信度（noul 且未推导 / ArgmaxOnly）
  confidence_source TEXT NOT NULL,
  low_confidence   INTEGER NOT NULL,-- confidence < thresholds.ask，或压根没有置信度
  input_tokens     INTEGER NOT NULL,
  output_tokens    INTEGER NOT NULL,
  estimated_cost_micros INTEGER NOT NULL, -- 本地恒 0；Jev 口径 = input_tokens*42/1000
  elapsed_ms       INTEGER NOT NULL,
  cached           INTEGER NOT NULL,
  degraded         TEXT,            -- 失败原因，NULL = 正常
  payload          TEXT NOT NULL CHECK(json_valid(payload))
);
CREATE INDEX decision_records_time ON decision_records(created_at DESC);
CREATE INDEX decision_records_kind ON decision_records(kind, created_at DESC);
CREATE INDEX decision_records_cache ON decision_records(cache_key, created_at DESC);
```

- `payload` 只放**结论与摘要**：`state_digest`、`questions_digest`、命中的 `decision_rule`、建议 alias、被拦问题项的 key 与得分、`truncated` 标记。**不放 state 原文、不放 `SKILL.md` 正文、不放 title/summary**（原文各自已在 `feed_items` 与磁盘上，二次复制只增加泄露面与体积）。
- 裁剪：保留 **30 天**或 **20,000 条**（先到为准），与 `feed_*` 同一套裁剪习惯。
- 迁移的既有坑：新增表必须同步补进 `tests/sqlite_repository.rs` 里「模拟 v1 库」的 DROP 列表（`architecture/04` §7 明写），否则会报「表已存在」的假失败。

### 4.2 与既有用量统计的合并口径（必须分开列，禁止混算）

| 指标 | 定义 | 界面标签 |
| --- | --- | --- |
| 决策次数 | `count(*)`，按 `local` 分两组 | 「决策 N 次（本地 M / 在线 K）」 |
| 输入 token | `sum(input_tokens)`，同样分两组 | 「输入 token（决策）」 |
| 估算成本 | `sum(estimated_cost_micros)`；本地恒 0 | 「估算 $X」；在线与本地**并排两列**，本地列写 `$0（本地推理）` |
| 低置信度占比 | `sum(low_confidence)/count(*)`，**分母写明**（含无置信度记录） | 「低置信度 N%」+ 悬停说明「低置信度不等于错误率」 |
| 缓存命中 | `sum(cached)` | 「缓存命中 N 次（未重复计费）」 |
| 失败/降级 | 按 `degraded` 分类计数 | 「超时 N / 熔断 N / 不可达 N」 |
| 是否生效 | `sum(applied)` | shadow 期间恒 0，界面单列「影子模式」 |

**与 `UsageReport` 的关系**：两套数字**不相加**。`usage_report` 是宿主自己的 token 计费（来自 rollout），`decision_meter` 是本工具决策调用的开销；它们来自不同数据源、不同计费方。用量页并列展示，**不做合并总数**（既有立场：四个指标之间都禁止相加推算，`design/07` §2.1）。

### 4.3 界面呈现位置

- **用量页**（`src/features/usage/UsagePage.tsx`），在「本机计划额度」卡之后新增两块（都在既有 `.page` grid 内，`gap: 24px` 与卡片内边距 24px 的既有规范不变）：
  1. `成本洞察（估算）`——反事实估算、异常用量日、任务形态归类；卡头固定一行免责：「以下都是估算，不是实测节省；换档假设见详情」。
  2. `决策开销`——§4.2 的七行；未启用决策服务时显示一行说明（不是空卡）。
- **设置页**新增「决策服务」小节：后端选择 + `decision_backend_test` 的连通性结果 + 各用途的 enabled/mode/shadow 计数。**不显示**任何「节省 xx%」的承诺性文案（R3 §③C：该生态最容易失信的地方）。
- **插件中心**：预检结果在详情页（`MarketDetail`）内联展示；拦下/询问都必须能展开看到命中的问题项与原文片段。
- **内容中心**：筛选只在排序与「低相关」折叠组生效，状态行加一个小小的「已按相关性筛选（可关）」；**永不因筛选失败改列表结构**。

## 5. 分阶段实施计划（每阶段一个可独立验收的产物）

**统一门禁命令（本仓库现有）**：

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                     # 含 switch-core / gptswitch / bridge（bridge 起真子进程）
cargo run -q -p switch-core --example g0_apply_pipeline -- /tmp/g0-check   # 应用管线真实产物
pnpm typecheck && pnpm test && pnpm build
scripts/check-publish-safety.sh            # 隐私扫描（CI 也在跑）
# 平台/有副作用的用例默认 #[ignore]，验收时显式跑：
cargo test -p switch-core -- --ignored     # live_fetch(网络) / restart_host(会重开 Codex) / system_vault(Keychain)
```

| 阶段 | 产物 | 验证命令（在上面的基础上追加） | 依赖 / 前置 |
| --- | --- | --- | --- |
| **S0 契约骨架（无行为变化）** | `decision/` 类型与 trait、`StateBuilder`、问题模板、`DECISION_PROMPT_REVISION`、v5 迁移、`decision_status` | 门禁全绿；`cargo test -p switch-core decision`；新增「禁用字段守卫」单测（§2.3 规则 5）与「迁移 v4→v5」测试 | 无。**不含**任何调用与网络 |
| **S1 P0-1 用量洞察（离线）** | `usage/insight.rs` + `usage_insight` + 用量页「成本洞察（估算）」卡 | 门禁全绿；`cargo test -p switch-core usage`；本机真实数据抽样核对（参照 `design/07` §6 第 2 条的验收口径：与一份独立实现逐项对数）；`node scripts/in-page-audit.js` 两主题（`?theme=light|dark`）几何/对比度全 0 | 无。金额需用户填单价（§8 Q3） |
| **S2 P0-2 shadow + P0-3 计量** | 网关决策点（默认关）、决策记录表读写、`decision_records`/`decision_meter`、用量页「决策开销」、设置页小节 | 门禁全绿；**集成测试**：`crates/switch-core/tests/gateway_server.rs` 扩两例——① shadow 开/关时上游收到的字节逐字节一致；② 决策后端不可达时请求仍 200 且记录一条 `degraded` | 前置：**`ResponseBinding`/`CONTINUATION_BOUND` 未实现** ⇒ 钉住只能用进程内 `chain_digest`（§2.7），跨重启不保持；这一点必须在设置页写明 |
| **S3 P0-4 归因** | `decision/attribute.rs` 规则表 + 灰区 + 界面归因标签 + 一致性守卫 | 门禁全绿；`cargo test -p switch-core attribution`（每条规则 ≥2 份夹具）；守卫单测：与状态矛盾的模型结论被降级为 `unknown` | 规则部分无前置；**灰区（模型）需要 S4 的任一后端** |
| **S4 P1-5 后端三选一** | ①借道既有供应商（默认）→ ②本地 Laya（首次下载 + 复用 minisign 清单，§6）→ ③托管 Jev（`/v1/systemone` 透传 + BYO-Key） | 门禁全绿；`decision_backend_test` 在真机对每个后端各出一条真实连通性报告；**校准夹具**：50–100 条标注样本，输出混淆矩阵与 ECE，结论落 `docs/audits/`（R2 §⑦要求） | ②需 §6 的打包/下载与 **macOS 最低版本**决策（§8 Q1）；③需大陆可达性结论（§8 Q2） |
| **S5 P1-6 / P1-7 / P1-8** | 配额回退建议（`resets_at` 已在手）｜feed 相关性筛选（阈值 0.60 起、可关）｜插件安装前预检（≥0.80 拦 / 0.30–0.80 问人） | 每个用途独立验收：门禁全绿；`cargo test -p switch-core`（内容：模拟失败回退时间序；插件：`Ask` 路径不写任何文件）；人工走查三页 × 两主题 | 前两个依赖 S2 计量；P1-8 复用 `RepoSkill.document.body`（已存在）；自动切换需 S2 的钉住 + 用户显式开关 |

**每阶段的通用红线**：新增表/字段同步改 `tests/sqlite_repository.rs` 的模拟 DROP 列表；新增文案同步双语与 `i18n.test.ts`；新增命令必须进 `src-tauri/src/main.rs` 的 `generate_handler!`（`main.rs:424`），否则前端报「命令不存在」。

## 6. 打包、签名与更新影响（只有引入本地模型或 sidecar 时才需要）

现状基线：更新包 `Switchelp.app.tar.gz` **7,698,412 B = 7.34 MiB**；随包 sidecar `gptswitch-bridge-app-aarch64-apple-darwin` **707,312 B**；`externalBin: ["binaries/gptswitch-bridge-app"]`；`minimumSystemVersion: "12.0"`。

| 项 | 结论 | 依据 |
| --- | --- | --- |
| 体积 | 随包 = **7.34 MiB → 约 280 MiB**（Laya GGUF Q4_K_M 259.6 MB 或 ONNX INT4 262.4 MB），`latest.json` 单平台产物同步膨胀约 36 倍；**推荐首次使用下载**，不随包 | R6 §4.1（【仓库/代码】实测字节） |
| macOS 最低版本冲突（**点名**） | ORT ≥1.24 的 macOS arm64 wheel 从 `macosx_13_0` 跳到 `macosx_14_0`，即 **ORT 实际要求 macOS 14+**；llama.cpp 官方包用 `-DCMAKE_OSX_DEPLOYMENT_TARGET=13.3`。而 `src-tauri/tauri.conf.json` 写的是 **`minimumSystemVersion: "12.0"`**，`architecture/05` 的用户目标写的是「macOS 14+」——**三处不一致**。引入任何本地推理都会把下限钉到 **13.3（llama.cpp）或 14.0（ORT）**，必须同时改 `tauri.conf.json`、平台矩阵与 `latest.json` 的说明；`darwin-aarch64` 平台键不区分最低系统版本 ⇒ 老系统用户会照常收到更新但因无法加载模型而**静默降级**，必须在应用内做版本探测并如实提示 | R6 §4.4（【仓库/代码】wheel tag 抽样） |
| sidecar 重名坑 | 历史坑：sidecar 与同名 cargo bin 重名会导致拷包互相覆盖，本仓库已用 `gptswitch-bridge-app`（与包名区分）规避。新增 sidecar（如 `llama-server`）必须守同一约定：`binaries/<name>-<target-triple>` 且 `<name>` 不与任何 cargo bin 同名 | `docs/development/03-signing-and-release.md`、R6 §4.2 |
| 签名/公证 | Tauri bundler 会把 `externalBin` 复制进 `Contents/MacOS/` 并以可执行目标身份签名（自动开硬化运行时）。本仓库现状：**已签名未公证**（无 notarytool 凭据）⇒ 一个 280 MiB 的未公证应用，说服成本显著上升 | R6 §4.2、`03-signing-and-release.md` |
| minisign 复用 | 权重清单**复用现有密钥对**（`plugins.updater.pubkey` 已随每个已发布包固化，**绝不新生成密钥**，否则老用户更新链路失联）。清单形态：`{ name, size, sha256, minisign-sig }`，由 `scripts/make-latest-json.mjs` 同一套私钥签；校验失败即拒绝加载并回退在线路径，沿用 `update.signatureMismatch` 的「如实说明」姿态 | R6 §4.1、`architecture/06-updates.md` |
| 下载源 | HF 直链返回 302 到带过期签名的 CDN，**必须解析重定向、不能缓存目标 URL**；镜像 `hf-mirror.com` 实测 200 但非官方 ⇒ 做成可配置 + 官方源回退，默认关闭 | R6 §4.1 |
| 许可 | 运行时与权重都必须是 MIT / Apache-2.0：`ort`、`llama.cpp`、Laya 三 checkpoint（均 Apache-2.0，带 `commercial-use`）。**红线**：`openjev/openjev` 系 **CC-BY-NC-4.0 非商用**（GGUF 仓许可自相矛盾），`NanoJev` 权重**无许可证**，`ekzhang/openjev-sglang` **无许可证**，`Jev-Style v3` 训练数据条款不清——**一律不打包** | R2 §5、R6 §5 |
| Windows | 本地推理不依赖 Codex 网关，理论可落地；但**应用内更新在 Windows 不产签名产物** ⇒ 「首次使用下载权重」的现成管道不存在，且若要把决策接进网关路由，Windows 网关本身还没起来（凭据 helper 是桩）。**结论：Windows 上先只支持 ①（借道既有供应商）** | `architecture/05` §6 实现现状 |

## 7. 风险与回滚

| 风险 | 证据（档位） | 缓解 | 回滚动作 |
| --- | --- | --- | --- |
| **校准不可信**：小模型默认过度自信 | Laya 官方模型卡自述零样本 typed-decisions **0.362**，低于多数类基线 0.461；ECE 0.466 → 温度拟合后 0.081（【官方】，R2 §⑥、R4 §③）；扑克实测「最错的答案置信度最高 0.86」（【第三方文章】，R5 §4.1） | ①阈值只在代码里、逐用途可调；②**上线前必须做校准夹具**（S4）；③本地后端默认只用 argmax 场景，置信度门控先关；④界面永远写「估算/概率」不写「准确率」 | 把该用途降回 `Shadow`；或整体 `decision_set_enabled(kind,false)` |
| **CJK 表现更差** | Jev 官方：「English is the primary training language… **including CJK scripts, are handled but not equally well**」（【官方】，R1 §⑤）；Laya 早期 checkpoint 仅英文，需显式用 multilingual（【仓库/代码】，R5 §4.6） | 中文场景（feed 标题、SKILL.md 中文）**用 multilingual checkpoint 或借道既有供应商**；上线前用中文夹具单列一组 ECE | 该用途改回确定性规则（关键词/BM25/时间序） |
| **中国大陆可达性** | `api.typesafe.ai` 解析到 Cloudflare 非中国节点；机房/代理 IP 被 Bot Management 403（实测）；**无国内上架**（R1 §⑨，【推测】+【仓库/代码】） | 托管 Jev **只能作为可选后端**，不参与关键路径；默认后端是「借道既有供应商」；`decision_backend_test` 在真机给可信的失败原因 | 关掉该后端即回到用户已有供应商 |
| **反向实测：模型路由可能更贵** | suenot holdout：Jev 选子代理比单个 Sol-xhigh **贵 69.7%**；本地 Laya 比确定性选择**贵 8.8%**；jevgrep 把 Jev 成本降 59% 后合计仍贵 2–3%（【仓库/代码】，R3 §③B） | ①`GatewayRoute` 默认只 shadow，enforce 需用户显式开启 + 会话钉住；②界面**不承诺节省**；③先做确定性裁剪与规则（本方案的 P0 全是规则/离线） | 关掉 `GatewayRoute` enforce；保留 shadow 观测 |
| **许可与空壳项目** | CC-BY-NC 红线、无许可证权重、`awesome-jev` 同名高星空壳（同一作者同日批量、README-only，策展方自己警告「A listing is not an endorsement」）（R2 §5.2–5.3、R4 §③C、R5 §4.5） | 只在 §6 白名单里选运行时与权重；建立「提交数 + 测试文件数 + 是否有公开评测产物」三条筛选，**不用星数** | 移除该依赖条目，回到借道供应商 |
| **隐私面被悄悄扩大** | 生态里最火的同类应用把聊天记录整段发给云端（R5 §4.3）；本方案 P1-7/P1-8 会发出**公开文本** | `DecisionState` 闭枚举让「读会话内容」不可表达；state 原文不落库（§4.1）；发文本的两个用途可关且有 UI 说明；诊断白名单不变（如要记决策事件需新增 4 个键：`decision_id`/`decision_kind`/`decision_backend`/`decision_action`，`diagnostics/mod.rs:30`） | `decision_set_enabled(kind,false)`；本地后端（不出网） |
| **提示注入 / 对抗输入** | 提供商自己承认对抗输入能影响分类器（R1、R3 §③D）；laya input 护栏 AUC 0.840 vs 裁判 0.992、flag FPR 0.67（【仓库/代码】，R4 §2.4） | `PluginSafety` 只能做**加速器**：确定性规则（`curl \| sh`、`~/.ssh`、凭据路径）先短路，模型只处理灰区；**永不 allow 静默安装**，只有「拦」与「问」 | 关掉模型部分，只留规则 |

## 8. 开放问题（需要主 Agent 或用户决策；每条给建议默认值）

| # | 问题 | 建议默认值 | 谁定 |
| --- | --- | --- | --- |
| Q1 | 本地 Laya 是否进发行版？若进，macOS 最低版本怎么处理（ORT 14.0 / llama.cpp 13.3，与现写 12.0 冲突） | **不随包**：首次使用下载 ONNX INT4（262.4 MB）+ `ort` in-process（无第二个可执行文件）；最低版本**如实提到 14.0** 并同步 `tauri.conf.json`、平台矩阵与更新说明；Intel Mac 不支持本地推理（`ort` 无 x86_64-apple-darwin 预编译） | 用户（影响分发承诺） |
| Q2 | 托管 Jev 在大陆是否可作可用后端（唯一判据是大陆用户的一条 `curl -w '%{time_total}' https://api.typesafe.ai/v1/models`） | **先不作为默认**；只在用户自带 Key 时开放，并在配置页写「需要能稳定访问 Cloudflare」 | 用户 + 一位大陆用户实测 |
| Q3 | 反事实成本估算的单价从哪来（本项目既有立场：没有可靠单价来源就不给金额，`design/07` §1.2） | **用户自填单价**（本地 settings，逐模型一条，可留空）；`PriceBook` 为空时只出「反事实 token 差」与异常日判断，**不出金额**；不随包任何价目表 | 用户 |
| Q4 | P0-1 的「任务类型归类」是否接受**只看形态不看内容**（内容分类必然要读请求正文，与「不读会话内容」冲突） | 接受形态归类（按 provider/模型/缓存比率/输出输入比/工具声明分桶），并在卡头写明「按请求形态归类，不读会话内容」；内容分类留到 P2 且默认关闭 | 用户（影响该卡的价值预期） |
| Q5 | `GatewayRoute` 的 enforce 是否在本次范围（它有明确反向成本证据，且依赖未实现的 `CONTINUATION_BOUND`） | **本次只做 shadow**：enforce 开关存在但需 200 条 shadow + 用户显式开启 + 会话钉住；跨重启钉住等 `ResponseBinding` 实现后再说 | 主 Agent |
