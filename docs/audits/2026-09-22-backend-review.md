# 后端专项审核报告

日期：2026-09-22 · 审核员：后端审核员（向产品总监汇报）
范围：`crates/switch-core`、`crates/bridge`、`src-tauri/src`（只读，未改任何源码）
章程：`docs/audits/2026-09-22-product-director-charter.md`（§四 裁决标准、§5.4 任务书）
已确认基线（不在本轮重复测量）：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全绿；`cargo test -p switch-core` = 556 passed / 0 failed / 3 ignored；`cargo test -p gptswitch-bridge` = 10 passed。

**本报告所有行号均在本轮当场用 `Read`/`grep` 核过。**

---

## 〇、结论先行

- 七条命题里 **5 条判「合格/已修复」**：域边界、配置写入事务、凭据边界、网关鉴权与本地绑定、Bridge 进程与路由。
- **2 条判「不合格」**：
  - **P0-A（新发现）**：上游流在 SSE 帧中途被干净关闭（对端 FIN 而非 RST/error 帧）时，网关把**半截回复标成 `status: "completed"`** 回给宿主——典型「失败被伪装成成功」。同一族的历史 P0-2 已修复，但这条残口没被覆盖，也没有测试。
  - **P1-A（复核：部分修复）**：历史 P1-14 的「损失记账用户看不到」仍未闭环：核心侧已给 `AdaptationLoss` 加了 `message_key`，但网关只把 `feature` 名写进诊断事件，`loss.*` / `result.*` 文案键在前端字典里**不存在**，日志页直接渲染未翻译键名。
- 历史两条 P0 复核结论：**P0-2 已修复**（有 4 条针对性测试）；**P0-5 已修复**（改法与本轮历史建议不同）；**P0-7 部分修复**（阈值 24 起，20–23 位无前缀串仍漏，但当前无承载路径，不构成 P0）。
- **P1-15（Windows helper 假装成功）已修复**：helper 安装改为直接失败，失败一路上传到「拒绝应用配置」。

---

## 一、命题 1：域边界

**判定：合格（本轮新读 + 命令实测）**

- `cargo tree -p switch-core -e normal | grep -iE "tauri|wry|gtk|webkit|tao"` → **无任何输出**。`switch-core` 依赖只含 `serde/serde_json/thiserror/tokio/tracing/toml_edit/time/uuid/sha2/percent-encoding/rusqlite/zeroize/ureq/quick-xml`（`crates/switch-core/Cargo.toml:11-34`），平台相关只有 `keyring`（macOS/Windows）与 `libc`（unix）。
- 依赖方向：`src-tauri/Cargo.toml:12-13` = `switch-core` + `tauri` + updater，即 **壳 → 核心**，核心不反向依赖窗口框架。`crates/bridge/Cargo.toml:12-13` 的 `[dependencies]` **只有 `serde_json`**，`cargo tree -p gptswitch-bridge` 整棵树只有 `serde_json/itoa/memchr/serde_core/zmij` —— bridge 是**零业务依赖的独立 sidecar 二进制**（`src-tauri/tauri.conf.json:34` 的 `externalBin`），不依赖 `switch-core`、也不依赖 `tauri`。
- 结论：三层边界（核心 / 桥 / 壳）成立，`switch-core` 的「不依赖窗口框架」为真命题。

---

## 二、命题 2：配置写入的事务 / CAS / 原子替换

**判定：合格（本轮新读；P0-5 复核为已修复）**

1. **临时名唯一性（P0-5 ①）已修复。** `crates/switch-core/src/codex/config.rs:897-938` 的 `write_atomic`，临时名为 `.{name}.gptswitch.{pid}.{serial}.{nanos}.tmp`（`config.rs:907-918`），序号来自 `next_temp_serial()` 的进程内 `AtomicU64`（`config.rs:941-944`）。历史问题「临时名写死 `.{name}.gptswitch.tmp`，320 次并发写 223 次失败且失败形态是写坏」不再成立。回归测试 `config.rs:1010-1045` `concurrent_writers_never_produce_a_torn_file`：32 线程并发写 4 种含 64 KiB 负载的内容，断言落盘内容必须**等于某一次输入的完整内容**且目录里不留临时文件。
2. **权限继承**：目标已存在时复制其权限位再 rename（`config.rs:924-926`），避免 create+rename 把用户的 0600 悄悄放宽成 umask 值。
3. **CAS**：`crates/switch-core/src/application/apply.rs:446-463` —— 提交时重读快照、比对 `expected_config_hash` / `expected_config_exists`（`check_cas`），不匹配即转 `Conflict` 并拒绝；另有两道冻结校验：`apply.rs:469-474`（`current_source_hash` 变化即拒）与 `apply.rs:475-480`（目录文件 hash 变化即拒）。计划过期也拒绝（`apply.rs:434-444`）。
4. **publish 与 write_atomic 的先后与回滚（P0-5 ②）已修复，但改法与本轮历史建议不同（非新发现，属复核偏差）**：历史建议是「把 `router.publish` 移到 `write_atomic` 成功之后」。当前实现是**发布在前、写盘在后、写失败即回收**：`apply.rs:505-506` 先 publish（注释明确「发布失败必须发生在修改 Codex 之前」，发布的是冻结输入）；`apply.rs:510-524` 写盘失败则 `router.retire(&catalog_revision)` 收回，收不回（仍有在途引用）时在错误里**如实写明「未能回收」**而不假装已清理。前后顺序相反但两个洞都补上了（写前发布使「发布失败」不会污染用户配置；写后失败有内存态回滚），且 `write_atomic` 本身原子，不存在半写被 rename。判「已修复」。
5. **备份先于发布**：`apply.rs:496-503`，备份失败直接返回错误、什么都不改（`apply.rs:490-493` 注释）。
6. 目录文件同样走 `write_atomic`（`apply.rs:1030-1037`）。

---

## 三、命题 3：凭据边界

**判定：合格；诊断包脱敏部分修复，残余不构成 P0（本轮新读）**

- **上游 Key 只进系统凭据库**：`crates/switch-core/src/credentials/system.rs:42-68` 只经 `keyring::Entry`（`service` + `gptswitch/` 前缀引用，`system.rs:22-30`）；非 macOS/Windows 显式返回 `KeystoreLocked`/`keystoreUnsupported`（`system.rs:71-90`），不退回 mock 假保存。SQLite 侧**无 secret 列**：`grep -rn "secret" crates/switch-core/src/storage/sqlite.rs` 无匹配；`domain/credential.rs` 落库的只有 `masked_suffix`（`credential.rs:107`、`124`）。
- 内存侧：`credentials/resolver.rs:88-131` 解析，`ResolvedSecret` 手写 `Debug` 打星（`resolver.rs:33-42`）、`Drop` 时 `zeroize`（`resolver.rs:44-48`）；`gateway/helper.rs:9-10` 明确「上游 Key 始终留在系统凭据库，helper 不接触它」。
- **`config.toml` 只有本地 token**：受管键只有 5 个（`codex/config.rs:23-29`：`model` / `model_provider` / `model_catalog_json` / `model_context_window` / `model_reasoning_effort`）加 `model_providers.gptswitch` 子表，鉴权走 `ProviderAuth::Command` → 本机 helper 路径（`config.rs:949-959`）；回归测试 `config.rs:975-983` `command_auth_never_carries_an_upstream_secret` 断言渲染结果不含 `api_key` 与 `sk-`。helper 只 `cat` 应用数据目录里的 0600 令牌文件（`gateway/helper.rs:112-134`、`74-78`）。
- **脱敏洞（P0-7）部分修复**：`diagnostics/mod.rs:269-281` `redact_value` → `304-326` `looks_like_secret` → `339-358` `is_opaque_mixed_case_token`。新增的「无前缀现代密钥」规则为 `MIN_OPAQUE_LEN = 24`（`mod.rs:340`）、字符集 `[A-Za-z0-9-_]` 且**大小写与数字三者齐备**（`mod.rs:354-357`），并排除 `inst_/rev_/op_/plan_/gs//vendor//p_/m_` 前缀（`mod.rs:330-332`、`342-346`）。回归测试 `mod.rs:572-607`：32/34/40 位无前缀混排密钥被脱敏，本工具 id 与正常文本不受影响。
  - **残余**：长度 20–23 的无前缀串、以及大小写不齐备（例如全小写非 hex 的 24–39 位串）仍不脱敏。
  - **为什么仍判非 P0**：本轮核过网关**不把上游错误正文写进诊断**——`gateway/server.rs:509-525` 只把上游错误正文脱敏后放进响应体，诊断事件只记 `alias/provider_id/model_id/http_status/error_code`（`server.rs:511-524`）；其余 `with_metadata` 位点只见标识符与计数。因此「上游 Key 回显进诊断包」这条路径当前**没有承载点**，残余属规则覆盖面而非可利用泄漏。判 **P2**（建议把阈值降到 20 或对 ≥20 的纯小写高熵串单独一条规则 + 补 canary 用例）。
- 历史项「被跟踪测试硬编码真实供应商域名」不在后端范围，留给隐私/测试审核员。

---

## 四、命题 4：网关鉴权与本地绑定

**判定：合格（本轮新读 + 测试）**

- **只绑 127.0.0.1**：`gateway/server.rs:107-116`，`SocketAddr::from((Ipv4Addr::LOCALHOST, port))`；端口被占返回 `PortInUse` 并附可读成因（`server.rs:109-116`），不静默换端口。
- **拒绝 Origin / 预检**：`gateway/auth.rs:141-147` —— `access-control-request-method` 存在即 `Unauthorized`，`origin` 存在即 `Unauthorized`；请求头解析确实提取这两个头（`server.rs:931-945`）。另有防 DNS rebinding 的 Host 校验（`auth.rs:149-156`，`is_loopback_host` 允许 `127.0.0.1/localhost/::1/[::1]` 各种带端口形式，`auth.rs:198-224`）。
- **令牌**：`GatewayToken::generate` 用两个 UUIDv4 经 SHA-256（`auth.rs:29-41`），`matches` 为常量时间比较且单独处理长度差（`auth.rs:58-60`、`74-83`，回归测试 `auth.rs:233-244`）；`Debug` 打星（`auth.rs:63-68`，测试 `auth.rs:272-277`）；失败不区分「无令牌 / 令牌错」（测试 `auth.rs:405-418`）。
- **启动换新 token**：`src-tauri/src/main.rs:85` 每次 `start_gateway` 都 `GatewayToken::generate()`，`main.rs:99-100` 取得端口后才覆盖 helper（注释：「第二个启动失败的进程不能使已有网关失联」）。`server.rs` 的 `GatewayStatus` 只暴露 `token_fingerprint`（字段 `server.rs:1130`、赋值 `:1144`、实现 `:1150-1158`，SHA-256 前 4 字节），不吐令牌。
- 测试：`gateway/auth.rs:306-326`（Origin 与预检被拒）、`crates/switch-core/tests/gateway_server.rs:625-641`（`/health` 也要令牌）、`:346`（缺/错令牌被拒）、`:660`（超大体积先拒后读、拒绝分块请求体 `server.rs:939-943`）。

---

## 五、命题 5：协议翻译的「损失记账」

**判定：不合格（P1-A，复核：部分修复）**

**5.1 `reasoning.effort` 空集合的自相矛盾 —— 已修复（本轮新读）**

历史 P1-14 记「chat 丢弃、responses 透传」。当前**两边行为一致**：
- `protocols/chat.rs:109-131`：空集合时 `losses.push(AdaptationLoss::new("reasoning.effort", "loss.reasoningEffortNotDeclared", …))` 且不发送；档位越界同理（`chat.rs:125-131`）。
- `protocols/responses.rs:51-79`：空集合时 `object.remove("reasoning")` 并 push 同一条 loss（`responses.rs:57-63`）；未声明档位也移除（`responses.rs:71-77`）。
- 测试：`responses.rs:234` `a_model_that_declares_nothing_gets_no_reasoning_field_at_all`、`responses.rs:215-231` 未声明档位不转发。

**5.2 `losses` 只有日志、用户看不到 —— 仍存在（复核：部分修复）**

- 核心侧确实补了文案键位：`protocols/mod.rs:23-37` 的 `AdaptationLoss { feature, message_key, detail }`，全部 12 处 `AdaptationLoss::new` 都带了 `loss.*` 键（如 `chat.rs:117-121`、`chat.rs:190-194` `loss.reasoningItemDropped`、`chat.rs:381`、`responses.rs:59-63`）。
- **但没有任何消费点**：
  1. 网关只把 `loss.feature` 拼进诊断事件的元数据 `error_code`，`message_key` 被丢弃 —— `gateway/server.rs:459-481`（`prepared.losses.iter().map(|loss| loss.feature.as_str())`），事件本体是 `result.adaptationLoss`。
  2. 前端字典里**不存在** `loss.*` 也不存在 `result.*`：`grep -rn '"loss\.' src/locales/` 与 `grep -c "result\." src/locales/zh-CN.ts` 均为 0。
  3. i18n 守卫也不管这批键：`src/i18n.test.ts:31` 的 `CORE_PREFIXES` = `action/compat/credential/error/group/host/probe/reason/stage/warning`，**不含 `loss` 与 `result`**。
  4. 日志页直接把键名当文本渲染：`src/features/diagnostics/LogsPage.tsx:174`（列表行 `<span>{event.resultKey}</span>`）与 `:242`（详情 `<dd>{detail.resultKey}</dd>`）。
- 用户实际看到的是 `result.adaptationLoss` + 元数据里的 `reasoning.effort`，**看不到「该模型未声明思考档位，请求的 reasoning 字段未转发」这句话**。README 宣称的「记录为 losses 而不是假装生效」在工程上成立、在界面上不成立。判 **P1**（不改核心逻辑，只需补字典键 + 把 `message_key` 带进事件元数据 + 日志页查表）。

---

## 六、命题 6：Bridge 的进程与路由正确性

**判定：合格（本轮新读 + 测试）**

- **线程钉根**：`crates/bridge/src/lib.rs:343-391`。`Routing.direct` 先看 `pins`（`lib.rs:368-373`），命中即 `Why::ThreadPinned`；`thread/list` 合并时把两侧会话各钉到各自那根（`lib.rs:831-842`）；`thread/start|resume|fork` 成功后按返回的 threadId 钉根（`lib.rs:914-929`）。
- **只带 threadId 的续接跟随线程**：`lib.rs:866-891` —— `needs_probe` 对「带 threadId 但未见过」的冷路径（bridge 重启过 / 宿主按 id 直接打开）向托管那根发 `thread/read` 探测，成功则改判 `Managed` 并钉住；**探测失败不改路由**（回落原生，`lib.rs:871-873` 注释与实现一致）。测试 `crates/bridge/tests/multiplex.rs:307` `follows_the_thread_when_no_model_is_given`、`:354` `probes_the_managed_side_for_an_unknown_thread`。
- **子进程错误不伪装成成功**：`lib.rs:518-563` `request()` 收到 `error` 字段即记 `child-error` 日志并返回 `Err`（`lib.rs:540-548`），超时返回 `Err`（`lib.rs:551-561`）；`route_request` 把 `Err` 转成 JSON-RPC error 回宿主（`lib.rs:932-935`），**不返回空 result**。测试 `multiplex.rs:377` `surfaces_child_errors_instead_of_faking_success`。merge 路径对托管侧的失败明确降级为 `{"data": []}` 并留 `managed-list-failed` 证据（`lib.rs:794-801`、`:824-830`），属「如实记录的部分结果」而非伪造。
- **宿主关闭 stdin 后不留孤儿**：`lib.rs:686-694` stdin 读线程在 EOF 后 `drop(host_tx)` → 主循环 `for line in host_rx` 结束 → `lib.rs:718-720` `native.kill(); managed.kill()`。测试 `multiplex.rs:414` `exits_when_the_host_closes_stdin`。启动期若托管那根起不来则先杀原生再返回 127（`lib.rs:640-644`）。
- **子进程 id 合成**：`lib.rs:516-526`，bridge 自造 `b-{side}-{seq}` 并在回包时换回宿主的 id（`lib.rs:536-539`），不会让两根认错门。
- **P2（记录）**：路由状态锁中毒时静默回落 `(Side::Native, Why::Default)`（`lib.rs:866-869`），会**静默改路由**而不留痕；概率极低但属「失败不显式」。建议改为记一条 `routing-state-poisoned` 日志。

---

## 七、命题 7：错误是否被伪装成成功

**判定：不合格（P0-A，新发现）；已修复项见下**

### 7.1 已修复项（复核）

**（a）Windows 凭据 helper 假装成功（历史 P1-15）—— 已修复（本轮新读）**
`gateway/helper.rs:48-59`：`#[cfg(windows)]` 分支不再生成 `.cmd` 桩，直接返回 `CapabilityUnsupported` + `error.windowsHelperUnimplemented`，注释写明「必须在这里失败，不能『装上』」；`helper.rs:136-137` 明确「Windows 不再有 `script()`」。
失败**一路传导到拦下应用**：`src-tauri/src/main.rs:100` 的 `install_auth_helper(...)?` 使 `start_gateway` 返回 `Err` → `state.rs:94-106` 存入 `gateway = None` + `gateway_error` 原因 → `commands.rs:363-374` 在应用前拦下并给出可读原因（「本机网关未运行，现在写入会让 Codex 用不了这些模型：…」）。判「已修复」，且补上了历史缺失的「拒绝写入」这一半。

**（b）网关失败一律写可读响应（历史 P0-2）—— 已修复（本轮新读 + 4 条测试）**
- 接受循环不再吞错：`server.rs:239-258` 把所有路由分支收进一个 `match` 返回 `Result`，`server.rs:260-277` 在 `!response.started()` 时补 `write_error` 并记 `result.unansweredRequest`。模块注释（`server.rs:200-203`）把这条写成不变量。
- 流内错误不再被当「没内容」跳过：`server.rs:628-637` 检测上游 `error` 数据帧即写流内 `error` 事件并 `break 'stream`；`server.rs:588-589`、`665-669` 用 `upstream_failed` 保证**不补 `response.completed`**（「那会把半句话伪装成完整回复」）。
- 上游非 2xx 有结构化错误体（`server.rs:500-526`），错误载荷含 `code/message_key/details/retryable`（`server.rs:1005-1014`）。
- 测试：`gateway_server.rs:1104` `a_request_without_a_model_answers_instead_of_hanging_up`、`:1127` `a_deleted_provider_is_reported_as_an_error_not_a_dropped_connection`、`:976` `models_with_an_unpublished_revision_answers_with_a_readable_error`、`:495` `mid_stream_error_frame_is_surfaced_instead_of_reported_as_completed`。
- 密钥不外泄：`server.rs:1104-1120` `redact()` 把已知的上游 Key 替换成 `••••redacted` 再截断；上游错误分类见 `server.rs:1048-1062`（`upstream_status_error`，401/403→`error.modelPermissionDenied`、429→`error.upstreamRateLimited`、其它 4xx→`error.upstreamRejected`）与 `server.rs:1064-1077`（`upstream_transport_error`）；测试 `gateway_server.rs:551` `upstream_error_body_never_leaks_the_upstream_key`。

### 7.2 P0-A（新发现）：上游在 SSE 帧中途干净关闭时，半截回复被标成 `completed`

**位置**
- `crates/switch-core/src/gateway/server.rs:599-601`：读循环 `Ok(0) => break`（对端 FIN，非错误）。
- `crates/switch-core/src/gateway/server.rs:671-681`：循环结束后无条件执行 `parser.finish()` + `translator.finish()` 并收尾分块编码。
- `crates/switch-core/src/protocols/chat.rs:591-651`：`ChatStream::finish()` 无条件构造 `response.output_text.done` → `output_item.done` → **`response.completed` 且 `response_object("completed", …)`**（`chat.rs:645` + `682-690` 的 `"status": status`），没有任何「流是否正常结束」的入参。
- `crates/switch-core/src/gateway/sse.rs:52-59`：`SseParser::pending_bytes()`（`:52`）与 `has_incomplete()`（`:57`）正是为「尾部有残帧 = 流被截断」设计的；测试注释 `sse.rs:351`：「finish 提交残行，供上层判定 Incomplete」，测试 `sse.rs:344-355` `truncated_stream_reports_incomplete`。

**问题**：`has_incomplete()` / `pending_bytes()` 在整个网关侧**从未被调用**——`grep -rn "has_incomplete|pending_bytes" crates/switch-core/src/` 只命中 `sse.rs` 自身的测试。因此网关**无法区分**「上游发完收尾」与「上游在帧中途被切断」，两条路都走 `translator.finish()`，都产出 `status: "completed"`。宿主（Codex）因此把一段被截断的回复当作完整回复渲染，用户不会看到任何错误 event。

**为什么算「失败被伪装成成功」**（章程 §四 P0 口径：「产品对用户声称成功而实际失败」）：失败信号在链路上是**可得的**（残帧），代码拿到了它却丢弃了，并主动补了一个 success 终态。这正是历史 P0-2 那句注释（`server.rs:588-589`「否则截断会看起来像完整回复」）想要防的事，但当时只补上了「上游发 error 帧」这一支，**没补「上游被切断」这一支**。

**可达性（如实标注）**：需要上游/中间代理在吐完部分 token 后以**正常 FIN** 关闭连接（而非 RST、也不是超时）。典型来源：供应商侧网关超时后优雅关闭、CDN/反代把上游连接断开。本机无法构造真实供应商的该行为，**未实测**；可在 `gateway_server.rs` 用 `MockReply::Sse` 去掉尾部 `data: [DONE]\n\n` 并让 mock 正常关连接来复现（该用例当前不存在）。

**建议修法（一行判定 + 一个测试）**：在 `pipe_stream` 的循环退出后，若 `!saw_done && (parser.has_incomplete() || upstream 未给出完成事件)`，则走 `upstream_failed` 分支（写流内 `error` 事件、**不发 `response.completed`**），并在诊断里记 `result.upstreamTruncated`；补一条「上游流在帧中途结束，宿主必须收到 error 而非 completed」的回归用例。

### 7.3 P2（记录，非泄漏）

`gateway/helper.rs:88-91` 的 `revoke()` 注释说「进程退出时调用，避免令牌留在磁盘上」，它被 `gateway/mod.rs:16` 以 `revoke_auth_helper` 导出，但 **`src-tauri/src` 与 `crates/bridge/src` 里没有任何调用点**（`grep -rn "revoke" src-tauri/src/ crates/bridge/src/ crates/switch-core/src/gateway/` 只命中定义与再导出）。令牌每次启动轮换（`main.rs:85`、`helper.rs:205-216` 测试），残留的 0600 文件在下次启动前**不具备任何授权能力**，故不是泄漏路径；但「注释承诺的行为没有实现」属文档与实现不符，建议要么真在退出时调用，要么改注释。

---

## 八、与历史审计的对照

| 历史条目 | 本轮结论 | 依据 | 新增/复核 |
| --- | --- | --- | --- |
| **P0-2** 网关失败静默关连接 | **已修复** | `server.rs:260-277` 未开流兜底 `write_error`；`server.rs:631-637` 流内 error 帧；测试 `gateway_server.rs:1104/1127/976/495` | 复核（本轮亲自读代码 + 读测试名与断言） |
| **P0-5 ①** 临时名固定 / 并发写坏 | **已修复** | `config.rs:907-918` pid+serial+nanos；回归测试 `config.rs:1010-1045`（32 线程，内容完整性断言） | 复核 |
| **P0-5 ②** 先 publish 后 write 且发布不可回滚 | **已修复（改法与历史建议相反）** | `apply.rs:505-506` 仍先 publish，但 `apply.rs:510-511` 写失败即 `retire` 回滚，`517-523` 回收失败如实报错 | **本轮新读出的偏差**：不是「把 publish 挪到写后」，而是「写前发布 + 写后回滚」，两个洞均闭合 |
| **P0-7** 20–39 位无前缀密钥漏脱敏 | **部分修复** | `diagnostics/mod.rs:339-358`（阈值 24、大小写+数字齐备）；测试 `mod.rs:577-590` 覆盖 32/34/40 | 复核 + **新读出残余**：20–23 位与非混排 24–39 位仍漏；但当前无「上游 Key 进诊断」承载点（`server.rs:511-524` 只记标识符），判 P2 |
| **P1-14 前半** `reasoning.effort` 空集合时 chat 丢弃 / responses 透传 | **已修复** | `chat.rs:109-131` 与 `responses.rs:51-79` 均丢弃 + 记 loss；测试 `responses.rs:215-234` | **本轮推翻历史结论**（历史判「自相矛盾仍在」） |
| **P1-14 后半** `losses` 只有日志、无文案键、用户看不到 | **仍存在（部分修复）** | 核心已带 `message_key`（`protocols/mod.rs:23-37`），但网关只传 `feature`（`server.rs:459-481`）；`src/locales/` 无 `loss.*`/`result.*`（grep 0）；`i18n.test.ts:31` 前缀集不含二者；`LogsPage.tsx:174/242` 直渲键名 | 复核 + **本轮定位到具体断点** |
| **P1-15** Windows helper 假装成功 | **已修复** | `helper.rs:48-59` 直接 Err；`main.rs:100` → `state.rs:94-106` → `commands.rs:363-374` 拦下应用 | 复核（并补验了「失败是否真的拦住写入」这一半） |
| P1-12 单实例 / 旧目录回收 | **部分修复（非本报告命题，仅顺带）** | `apply.rs:546-575` `prune_catalog_revisions` 已接入生产路径（`apply.rs:532` 调用），注释（`apply.rs:538-543`）承认过去 `retain/release/retire` 零调用；单实例仍靠端口占用（`server.rs:109-116`） | 顺带复核，细节留测试审核员 |
| P1-13 panic 面（`expect("锁未被污染")`） | **仍存在（未修）** | `credentials/resolver.rs:77/81/102/122/151/182` 等 | 复核确认仍在（与后端相关部分） |

**本轮新增（历史未登记）**
1. **P0-A**：上游 SSE 帧中途被干净关闭 → `status: "completed"`（`server.rs:599-601` + `671-681`；`chat.rs:591-651`；`sse.rs:333-355` 的信号未被消费）。
2. **P2**：`revoke_auth_helper` 无调用点，与 `helper.rs:88-91` 注释矛盾。
3. **P2**：Bridge 路由锁中毒静默回落原生（`lib.rs:866-869`），不留痕。

---

## 九、未验证项（明确标注，不作结论）

1. **Windows 真机分支**：`helper.rs:48-59` 的 `#[cfg(windows)]` 分支、`platform::private_file_mode` 的 Windows 取值、NSIS/MSI 装机路径均**未在本机实测**（无 Windows 机器）。判「fail-fast」依据的是源码与单测，不是实机行为。
2. **他人机器上的钥匙串行为**：本轮只读到 `keyring` 的建模与错误映射（`system.rs:21-31`、`34-39`），`tests/system_vault.rs` 的 1 项 ignored（需目标系统显式验收凭据库）**未跑**。首次访问的授权弹窗、iCloud 钥匙串同步、多用户会话下的条目隔离**均未验证**。
3. **Intel Mac**：无产物、无机器；`keyring`/`ureq`/`rustls` 在 x86_64-darwin 的运行行为未验证。
4. **P0-A 的可达性**：本机未构造出「真实供应商在帧中途干净关闭」的实例（mock 侧可构造，但当前无用例）。判据是代码路径必然性，不是实测复现。
5. **真实上游质量**：协议翻译在真实供应商上的参数遵从、模态声明与上游实际能力的一致性未验证（需真 Key）。
6. 本轮**未跑** `cargo test -- --ignored`（会真的重启用户 Codex）与 `scripts/g0/*.mjs`（另有专人），按章程要求。

---

## 十、给修复轮的建议顺序（按严重度）

1. **P0-A**：`pipe_stream` 区分「正常结束」与「被截断」，被截断时走 `error` 事件、不发 `completed`；补回归用例。改动局限在 `gateway/server.rs` 一处加 `protocols/chat.rs` 一个入参。
2. **P1-A**：把 `AdaptationLoss.message_key` 带进网关诊断事件的元数据（`server.rs:459-481`），在 `src/locales/{zh-CN,en}.ts` 补 `loss.*` 与 `result.*` 键，`LogsPage` 对 `resultKey` 查表；把 `loss`/`result` 加进 `src/i18n.test.ts:31` 的 `CORE_PREFIXES` 让守卫接住。
3. **P2**：脱敏阈值补 20–23 位与纯小写高熵串；`revoke_auth_helper` 接上退出路径（或改注释）；Bridge 锁中毒留痕。
