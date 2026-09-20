# 工程实现审计：静态与动态验证（2026-09-20）

审计对象：`/Users/<user>/Code/ProjDev/gptswitch-macapp`，`HEAD = 2fda2a5`，工作区版本 `0.2.0`。
方法：只读代码审查 + 真实可执行实验（`cargo test`、`cargo clippy`、`cargo build --release`、
`pnpm build`、`pnpm test`、起真实网关后发原始 HTTP/curl 攻击、并发写入故障注入）。
未改动任何业务代码；实验用文件放在仓库外 `/tmp/audit-temp/`，跑完已从仓库移除。

结论速览：**构建与 CI 门禁是真实的（本地全部复现通过）**；工程问题集中在**网关的错误收口**
（接受连接但不应答，且不记诊断）、**配置写入的并发安全**、**提交顺序（先发布路由后写文件）**
与**若干「实现了但从未接线」的机制**（路由引用计数、实例绑定、FileJournal）。

---

## 严重度排序

| # | 结论 | 严重度 |
| --- | --- | --- |
| S1 | 网关有 4 条「接受连接但一个字节都不写」的路径，且不产生任何诊断记录 | 高 |
| S2 | `write_atomic` 的临时文件名固定 → 并发写同一路径大量失败、存在半写文件被 rename 的机制 | 高 |
| S3 | `execute_apply` 先 `router.publish` 后 `write_atomic` → 写失败后网关永久服务一个配置里不存在的目录版本 | 高 |
| S4 | 路由快照引用计数（`retain`/`release`/`retire`）在生产路径从未调用，旧目录永不回收 | 中 |
| S5 | 本机令牌与实例不绑定：`claimed_instance` 是死代码，`GatewayConfig.instance_id` 只用于展示 | 中 |
| S6 | 无进程级单实例保证，跨进程无任何锁；第二个实例网关起不来但仍可写 `~/.codex/config.toml` | 中 |
| S7 | 生产路径上的 `expect("锁未被污染")` 会把锁中毒升级成 panic；accept 循环 `let _ =` 吞掉一切 | 中 |
| S8 | SSE 解析缓冲区与连接数都没有上限；请求行长度检查在读完整个请求行之后 | 中 |
| S9 | `reasoning.effort` 在「未声明档位」时两个适配器行为相反（chat 丢弃并记 loss、responses 原样透传） | 中 |
| S10 | conversion losses 只进日志元数据、没有文案键、detail 被丢掉；前端没有可见反馈 | 中 |
| S11 | Windows 没有 fail-fast：helper 是失败桩，但配置照写、界面照报成功 | 中 |
| S12 | `FileJournal` 是死代码，与文档描述的 journal 存储不一致 | 低 |
| S13 | `PendingApplyBar` 把「重启失败的任何原因」都显示成「Codex 仍在运行」 | 低 |
| S14 | 构建/文档一致性：缺 `rust-toolchain.toml`、README 版本与产物名过期、隐私清单文件名前后不一致 | 低 |

---

## S1 网关静默失败：接受连接但不写任何响应，且不记诊断

**结论**
`handle()` 的多个分支用 `?` 把 `CoreError` 直接向上返回，绕过了唯一会写响应体的 `write_error()`；
而 accept 循环用 `let _ = gateway.handle(stream)` 把这个错误丢掉。结果是连接被接受后**零字节写入即关闭**，
既不回结构化错误，也不写一条诊断事件——界面与日志里完全看不见。这正好落在项目自己认定的高等级故障形态上
（`docs/appendix/02-traceability-and-risks.md:62`：「本机中间层是单点…故障形态是**静默挂起（accept 但不应答）**…
因此网关与 Egress 都必须有请求级超时、健康探测和『可判定失败』」）。

**证据（代码）**
- `crates/switch-core/src/gateway/server.rs:165-169`：`let _ = gateway.handle(stream);` —— 返回值被丢弃。
- `crates/switch-core/src/gateway/server.rs:236` → `handle_models()`；`server.rs:252-256`：
  `let aliases = self.router.aliases(revision).ok_or_else(|| CoreError::new(ErrorCode::RouteMismatch, …))?;`
  —— `?` 直接返回，**没有任何 write**。
- `crates/switch-core/src/gateway/server.rs:274-277`：`payload.get("model")…ok_or_else(|| CoreError::validation("请求缺少 model"))?`
  —— 同样无 write。
- `crates/switch-core/src/gateway/server.rs:310-313` / `314-320`：`repository.get_provider(...)?` / `get_credential(...)?`
  —— 同样无 write（发布目录后删掉供应商/Key 即可到达）。
- `crates/switch-core/src/gateway/server.rs:199-201`：`stream.try_clone().map_err(...)?` —— 同样无 write。

同一文件里其它分支（`server.rs:216-228`、`246-247`、`272`、`324-331`、`350`、`382`、`421`、`450`）
都是正确的 `write_error(...)`，说明这不是设计选择，而是漏收口。

**证据（实测）**
起真实网关（`/tmp/audit-temp/example_zz_audit_probe.rs.bak`，与 `crates/switch-core/tests/gateway_server.rs`
同样的组装方式），带**正确令牌**发请求：

```
### 3 GET models unknown revision
curl: (52) Empty reply from server
HTTP=000

### 11 no model field
curl: (52) Empty reply from server
HTTP=000

### 对照：12 native slug as model
{"error":{"code":"ROUTE_MISMATCH","details":["目录版本 rev_audit 不包含 alias vendor/Upstream-Model"],…}}
HTTP=404
```

同一结论用 Rust 集成测试复现（`/tmp/audit-temp/gateway_silent_fail_and_concurrent_write.rs`）：

```
running 4 tests
test unknown_catalog_revision_closes_without_any_response ... ok
test missing_model_field_closes_without_any_response ... ok
test valid_request_gets_a_response ... ok
```
（前两条断言状态行为 `<empty>`，即读到 EOF 前一个字节都没有。）

**复现步骤**
1. 把 `/tmp/audit-temp/gateway_silent_fail_and_concurrent_write.rs` 复制为
   `crates/switch-core/tests/zz_audit.rs`；
2. `cargo test -p switch-core --test zz_audit`；
3. 或运行 `/tmp/audit-temp/example_zz_audit_probe.rs.bak`（放到 `examples/` 下），
   用 `curl --noproxy '*' -H "Authorization: Bearer <token>" …/i/<inst>/c/rev_NOPE/v1/models`。

**影响**
宿主（Codex）拿到的是一次「空回复」，而不是可判定的错误码；对同一个前缀重试只会重复空回复。
运维侧最糟的部分是**日志页什么都看不到**：错误被 `let _ =` 吞掉，`result.rejectedBeforeRouting` /
`result.routeRejected` 都不会出现，用户无法定位「为什么模型点不动」。它与项目的可判定失败要求直接冲突。

**修法建议**
- `handle()` 的每个 `?` 改为先 `write_error` 再返回，或让 `handle()` 统一返回 `Result<Response, CoreError>`
  并由一个收口函数负责「要么写成功响应、要么写错误响应」。
- accept 循环把 `let _ =` 换成 `if let Err(error) = gateway.handle(stream) { diagnostics.record(...) }`
  （写失败/对端断开可降级为 Debug 级）。
- 加一条回归测试：对每个分支断言「一定有一个 HTTP 状态行」，而不是「连接被关闭」。

---

## S2 `write_atomic` 并发不安全：固定临时文件名

**结论**
`write_atomic` 用固定的 `.{file_name}.gptswitch.tmp` 作为临时文件，没有 `O_EXCL`、没有唯一后缀、
没有任何锁。两个写者写同一路径时会共用同一个临时 inode：失败的一方 `rename` 得到 `ENOENT`（报错），
更糟的是它可能继续往「已被对方 rename 成目标文件」的 inode 上写。

**证据（代码）**
`crates/switch-core/src/codex/config.rs:888-893`：
```rust
let temp = parent.join(format!(
    ".{}.gptswitch.tmp",
    path.file_name().and_then(|n| n.to_str()).unwrap_or("config.toml")
));
```
`config.rs:895` 用 `File::create(&temp)`（会截断，不是 `create_new`）。

**证据（实测）** 8 个小内容写者 + 1 个 8 MiB 写者，各自 40 轮，写同一个路径：

```
failures=225 final_bytes=8388616 matches_a_complete_write=true
failures=226 final_bytes=8388616 matches_a_complete_write=true
failures=223 final_bytes=8388616 matches_a_complete_write=true
```
三次独立运行：**320 次写里 223–226 次返回了错误**（约 70%）。本轮最终内容仍是完整的一份
（大内容写者的 `rename` 抢先成功），但机制上「半写内容被 rename 成目标文件」是可达的——
这正是 `config.rs:881` 注释承诺要避免的事。

**复现步骤**：见 S1 的临时测试第 4 条 `concurrent_atomic_writes_keep_the_file_consistent`。

**影响**
进程内 `ApplyService` 有 `commits: Mutex<()>`（`application/apply.rs:136`、`321-324`）串行化提交，
所以单实例下不易触发。但**没有跨进程保护**（见 S6），两个 Switchelp 实例、或未来的 journal/config
并发写就会命中：表现为「应用失败但说不清原因」，以及低概率的配置文件内容错乱——
被错乱的是用户 `~/.codex/config.toml`，代价最高。

**修法建议**
- 临时名唯一化：`format!(".{}.{}.{}.tmp", name, std::process::id(), uuid)` 配 `OpenOptions::new()
  .create_new(true)`；
- 写入前对目标取 advisory 锁（`flock`/`LockFileEx`），把 CAS 重读与 rename 都放进锁内；
- 或直接加单实例保证（S6），把跨进程并发从设计上排除。

---

## S3 提交顺序：先发布路由，后写配置文件

**结论**
`execute_apply` 在**写 Codex 配置之前**就把新目录版本发布进 `GatewayRouter`，且发布不可回滚。
一旦紧随其后的备份或 `write_atomic` 失败，网关会长期对外服务一个「配置里根本不存在」的目录版本，
而事务停在 `Committing`、`written_hash` 与磁盘内容不一致。

**证据（代码）** `crates/switch-core/src/application/apply.rs:379-405`：
```rust
let (text, ownership) = apply_managed(&snapshot, &prepared.managed, &state.ownership)?;
// 路由发布失败必须发生在修改 Codex 之前
self.router.publish(prepared.routes.clone())?;      // ← 381 已发布
state.operation.written_hash = Some(hash(&text));
…
self.operations.save(state.clone())?;               // ← 389 写前日志
if let Some(backups) = &self.backups { … backups.create(…)?; }   // ← 391-399 备份失败即返回
write_atomic(Path::new(&state.plan.config_path), &text)?;        // ← 400 写失败即返回
```
`router.publish` 之后没有任何补偿动作；`GatewayRouter` 也没有 `unpublish`。
后续恢复路径见 `apply.rs:489-512`（`decide_recovery`）：`written_hash != 当前文件 && expected != 当前文件`
→ `ConflictWithExternalChange`，于是事务进冲突，但**已发布的路由仍然在服务**。

**影响**
- 网关侧：`/i/<inst>/c/<rev>/...` 对新版本可路由，但 Codex 的 `base_url` 仍指向旧版本（或什么都没改），
  出现「网关有、宿主不知道」的幽灵版本，用户重启宿主也解释不通；
- 失败是「后置失败」：用户在差异弹窗点了确认，得到的是错误，却同时留下了一个已生效的路由版本。

**修法建议**
- 顺序改为「备份 → write_atomic → router.publish → 记录 AwaitingReload」；发布失败时按 S1 的思路
  走可判定失败（此时文件已是新内容，恢复策略本就是 `RecordCommitFromFile`，语义自洽）；
- 或在 `publish` 前先做一次「可写性探测」（父目录可写、磁盘有空间），把失败面收到发布之前。

---

## S4 路由快照引用计数是死代码：旧目录永不回收

**结论**
`GatewayRouter::retain` / `release` / `retire` 与 `RevisionRefs` 的整套「仍有引用就保留、
引用归零才回收」机制**只在 `routing.rs` 自己的测试里出现**，生产路径一次都没调用。
`RuntimePublication` / `ApplyService::publication()` 同样没有调用方。

**证据**
```
$ grep -rn "\.retire(\|\.retain(\|\.release(" crates/switch-core/src src-tauri/src | grep -v "fn retire\|fn retain\|fn release"
crates/switch-core/src/codex/detect.rs:200:        instances.retain(|instance| {     # Vec::retain，无关
crates/switch-core/src/diagnostics/mod.rs:298:    parts.retain(|part| !part.is_empty());  # 无关
crates/switch-core/src/gateway/routing.rs:549-570                            # 全是测试
```
`publication(` 只有 `apply.rs:169` 的定义命中。生产入口（`main.rs:246-283`）没有任何生命周期管理调用。

**影响**
- 每次「应用」都会 `publish` 一个新目录版本（`apply.rs:240-243` 让版本号随来源摘要变化，
  所以每次改动都是新版本），而旧版本永不移除：`GatewayRouter` 的内存随应用次数单调增长；
- 旧版本前缀永久可服务（`admission` 能命中旧快照），与
  `docs/architecture/02-configuration-lifecycle.md` 的回收语义不符；
- 前端「计划过期自动重试一次」（`CodexConfigPage.tsx:159-168`、`PendingApplyBar.tsx:61-75`）
  每次重试都会新建 operation + 新目录修订 + 新快照，把这个累积放大一倍；
- `docs/` 里也找不到「刻意不回收」的记录，所以这是**未接线**而不是设计决定。

**修法建议**
在 `handle_inference` 准入成功后 `retain(&route.catalog_revision, 0)`、请求结束（含 `pipe_stream` 的所有
提前返回路径，建议用 guard 结构体在 `Drop` 里 `release`）调用 `release`；新版本发布成功后对旧版本调
`retire`，返回 `false`（仍有引用）时留待请求结束再清理。`MemoryOperationStore`/SQLite 里的旧记录
与 `catalogs/<rev>/` 目录也需要同一套保留策略（当前没有 prune）。

---

## S5 本机令牌与实例不绑定

**结论**
`RequestGuard` 持有 `instance_id`，也提供了 `claimed_instance()`，但**网关路径从不使用它们做绑定校验**：
推理请求的实例取自 URL 前缀，只与「快照所属实例」比对，与令牌所属实例无关。

**证据（代码）**
- `crates/switch-core/src/gateway/auth.rs:120-127`（`RequestGuard::instance_id`）、`auth.rs:192-200`
  （`claimed_instance`）：`grep -rn claimed_instance` 只有定义与自身测试命中，生产路径零调用；
- `crates/switch-core/src/gateway/server.rs:215` 构造 guard 后只调用 `guard.check(&headers, len)`
  （`auth.rs:132-190`：方法、预检、Origin、Host、令牌、体积、content-type），**没有比对实例**；
- `crates/switch-core/src/gateway/server.rs:282-307`：实例来自
  `RuntimePublication::parse_prefix(&request.path)`，然后
  `self.router.admission(&revision, alias, &InstanceId::new(instance))` —— 比的是快照的 `instance_id`；
- 装配侧根本没有对齐：`src-tauri/src/main.rs:51` 把网关身份固定为 `AUTH_HELPER_INSTANCE = "local-main"`
  （`codex/config.rs:20`），而快照的实例来自检测结果（`apply.rs:273-286` 用 `instance.id`，
  `detect.rs:262` 是 `stable_instance_id(identity)`），两者**必然不同名**。

**影响**
- 与 README/架构文档的表述不符（`auth.rs:22-23`：「绑定到单个实例；只授权该实例的推理接口」）；
- 实际可被利用的面：一个进程若发布过多个实例的目录，任一实例的令牌都能访问其它实例的前缀。
  当前单用户单实例场景下影响有限，但这是**声明的安全边界没有实现**，且随着「一个网关服务多个实例」
  的设计意图（`server.rs:41-45` 注释明确写了这一点）会变成真问题。

**修法建议**
在 `handle_inference` 里改用 `router.admission_from_path(&request.path, alias, guard.instance_id())`
（`routing.rs:196-213` 已实现实例比对），或直接移除 `GatewayConfig.instance_id` 与
`claimed_instance()`，把语义改成「进程级令牌」。两者必须选一个，不能保持现状。

---

## S6 没有单实例保证，跨进程无锁

**结论**
仓库里没有任何单实例机制（无 `tauri-plugin-single-instance`，无锁文件），
`src-tauri/Cargo.toml` 的依赖里也没有相关 crate；第二个实例启动失败的地方只有网关端口。

**证据（代码）** `src-tauri/src/main.rs:210-217`：
```rust
let gateway = start_gateway(&directory, repository, vault, router, diagnostics.clone());
if let Err(error) = &gateway {
    // 端口被占用时不能假装在跑
    eprintln!("Switchelp 网关未启动：{} {:?}", error.message_key, error.safe_details);
}
```
失败被降级为一行 stderr；`app.manage(DesktopState::new(...))` 照常执行，
全部 IPC 命令（含 `apply_execute`）照常可用。
```
$ grep -rn "single_instance\|single-instance" src-tauri/ Cargo.toml src-tauri/Cargo.toml package.json
（无输出）
```

**影响**
两个 Switchelp 同时打开时：两者共享同一个 `~/.codex/config.toml`、同一个 SQLite 文件，
但 `ApplyService` 的互斥锁各自独立（S2 的固定临时名、S3 的发布顺序无补偿、CAS 只在各自进程内生效）。
用户完全看不出自己开了两个实例，冲突表现为随机的「应用失败 / 配置被回写」。

**修法建议**
加 `tauri-plugin-single-instance`（第二个实例只聚焦已有窗口），并在应用数据目录放一个独占锁
（`flock`）作为兜底：拿不到锁就拒绝启动 IPC，而不是只打一行日志。

---

## S7 生产路径上的 `expect("锁未被污染")` 与 accept 循环的错误吞噬

**结论**
生产代码里遍布 `Mutex::lock().expect("锁未被污染")`；任何一次在持锁期间 panic 都会让锁中毒，
此后**所有**调用点 panic。在网关连接线程里 panic 的表现同样是一次「不应答」（S1），
在 IPC 线程里被 `spawn_blocking` 的 `JoinError` 转成 `CoreError::internal`（`commands.rs:110-112`），
至少是可判定的。

**证据（生产路径，节选；测试内的 unwrap 已排除）**
```
crates/switch-core/src/storage/repository.rs:88,102,123,133,145,156,187,197,210,219,227,235,242,252,263,289,294
crates/switch-core/src/storage/operation.rs:95,105,126,136,143,150,161
crates/switch-core/src/storage/journal.rs:178,190,204,211
crates/switch-core/src/gateway/routing.rs:175,189,223,259,267,278,293,297,307,320,331
crates/switch-core/src/credentials/resolver.rs:77,81,102,122,151,182
crates/switch-core/src/credentials/memory.rs:22,26,34,52,62,69,78
crates/switch-core/src/diagnostics/mod.rs:177,180,181,195,196,225,233,237,244,245,246,254
crates/switch-core/src/diagnostics/probe.rs:151,158,220
crates/switch-core/src/application/apply.rs:61  (expect("UTC 可表示为 RFC3339"))
crates/switch-core/src/application/mod.rs:58   (同上)
crates/switch-core/src/codex/plan.rs:403       (expect("刚推入的事件必然存在"))
crates/switch-core/src/protocols/chat.rs:509   (expect("刚插入或已存在"))
```
另外还有 `let _ =` 形式的静默失败（已核对，多数无害，但两处值得改）：
- `crates/switch-core/src/gateway/server.rs:196-198`：`set_read_timeout` / `set_write_timeout` / `set_nodelay`
  全部忽略结果 —— 如果超时设置失败，连接将无限期挂住（这恰好是 S8 的慢连接场景放大器）；
- `src-tauri/src/commands.rs:509-511`：`let _ = backups().prune(DEFAULT_KEEP)` —— 保留策略静默失效；
- `crates/switch-core/src/codex/backup.rs:163`：`let _ = std::fs::remove_file(meta)` —— 残留 meta 无声；
- `crates/switch-core/src/gateway/helper.rs:67`：`revoke` 忽略删除失败（退出时令牌文件可能留下）。
其余 `let _ =`（`platform/mod.rs:441`、`codex/config.rs:607`、`catalog.rs:326`、`detect.rs:146`、
`domain/credential.rs:129`、`diagnostics/{discovery,update,probe}.rs` 的 mock 服务器、
`main.rs:104-151` 的窗口/进程操作）经核对是刻意的空实现或测试用，不算缺陷。
`panic!` / `todo!` / `unimplemented!` / `unreachable!` 在生产代码里**零命中**
（唯一的 `panic!` 在 `codex/plan.rs:768`，是测试断言）。

**修法建议**
- 统一一个 `lock_or_recover()` 帮助函数（`PoisonError::into_inner` 或映射成 `CoreError`），
  替换生产路径的 `expect("锁未被污染")`；持锁期间不做可能 panic 的操作；
- 网关连接线程包一层 `catch_unwind`，把 panic 转成 500 响应 + 诊断事件（否则又是「不应答」）。

---

## S8 SSE 解析与连接数缺少上限

**结论**
- `SseParser.pending` 是一个**无上限**的 `Vec<u8>`：`feed()` 直接 `extend_from_slice`，只有遇到 `\n`
  才 drain（`gateway/sse.rs:71-89`）。上游（或被劫持的上游）只要持续发送不含换行的字节，
  内存就单调增长；而空闲超时在**每次读成功后都会重置**（`server.rs:528-535`，`last_frame = Instant::now()`
  在读到任意字节后无条件执行），慢速滴流也能一直续命。
- 请求行长度没有检查：`read_request` 用 `read_line` 读完整行之后才检查 `MAX_HEADER_LINE`
  （`server.rs:771-781` 的检查只作用于头部行；请求行在 `server.rs:752-757` 无长度上限）。
- 每连接一个线程，无并发上限，读取超时 30 秒（`server.rs:196`）。

**证据（实测）**
```
### 18 huge header line (10MB)      # 请求行 10_000_000 字节
resp: b'HTTP/1.1 404 Not Found\r\n…{"error":{"code":"NOT_FOUND",…}}'   # 10MB 已全部进内存后才回答

### 19 partial body held (content-length 1000, send 10 bytes)
after 30.0s resp: b'HTTP/1.1 400 Bad Request\r\n…"请求体不完整"…'     # 线程被占住整整 30 秒
```
（说明有两个独立的线程占用「配置 30s 读超时 + 1000 字节声明」，每个连接便宜地占用一个线程。）

**影响**
本地 DoS 面：本机任意进程（含被诱导的浏览器页面以外的本地程序）用 N 个连接即可占住 N 个线程 × 30 秒。
恶意/异常上游则能撑大网关内存。单用户本机工具下不是远程可利用漏洞，但上游不受信时属于真实健壮性缺口。

**修法建议**
- 给 `SseParser` 加 `MAX_PENDING_BYTES`（例如 1 MiB），超限即报错并终止流；
- 请求行同样限制长度（读之前先看 `pending`/用 `take(MAX)` 或 `read_until` 带上限）；
- 连接总数加信号量（例如 32 / 64），并对空连接设置更短的「首字节」超时（现有 30s 是统一的读超时）。

---

## S9 `reasoning.effort` 未声明时两个适配器行为相反

**结论**
「模型没有声明任何档位」这一状态下：chat 适配器**不发送并记 loss**（`chat.rs:116-121`），
responses 适配器**原样透传**（`responses.rs:58-70` 的条件里 `!limits.reasoning_efforts.is_empty()`
才判定越界）。同一份目录数据在两条协议下得到相反的行为，代码注释与文案表的说法互相矛盾。

**证据（代码+测试）**
- `crates/switch-core/src/protocols/chat.rs:116-121`：
  `if limits.reasoning_efforts.is_empty() { losses.push(AdaptationLoss::new("reasoning.effort", "loss.reasoningEffortNotDeclared", …)) }`
- `crates/switch-core/src/protocols/responses.rs:51-70`：注释写「未在声明集合内的档位一律摘掉」，
  但条件是 `!limits.reasoning_efforts.is_empty() && !limits.allows_effort(&effort)` —— 空集合时不动；
  并且 `responses.rs:217-228` 的测试 `an_unconstrained_model_keeps_the_host_choice` 明确断言要透传。
- 文档侧声明「未声明就不发」：`crates/switch-core/src/protocols/mod.rs:57`（「非空表示该模型已声明档位」）、
  `src/locales/en.ts:272,283`（"Undeclared levels are never sent, only recorded as loss"）。
  于是**测试与文案表互相否定**。

**影响**
同一个未声明的 `high`，走 responses 的模型会被真的转发给上游（等于替模型虚报能力），
走 chat 的模型会被丢弃。目录里只要有供应商走 responses 就会不一致——这正是「不假装生效」原则想避免的。

**修法建议**
两处对齐到 `mod.rs:57` 的声明语义：空集合 = 未声明 → 一律不发送，并记录
`loss.reasoningEffortNotDeclared`；同步改 `responses.rs` 的测试与对应文案。
若确实想要「未声明即透传」的逃生舱，必须显式记录成文档化的例外并写进 PRD，而不是靠 `is_empty()` 的反向判断。

---

## S10 conversion losses 没有到达用户可见面

**结论**
`AdaptationLoss` 被计算出来了，但只被压成一行诊断事件的 `error_code` 元数据；人类可读的
`message_key` / `detail` 被丢弃，前端没有任何展示，文案表里也没有对应键。

**证据**
- `crates/switch-core/src/gateway/server.rs:384-406`：`result.adaptationLoss` 事件里
  `with_metadata("error_code", prepared.losses.iter().map(|loss| loss.feature…).join("+"))`
  —— 只保留 `feature`（如 `max_output_tokens+store`），**没有 `message_key`、没有 `detail`**。
- 前端：`grep -rn "loss" src --include="*.ts*"` 只命中 `en.ts:272,283` 两句编辑页提示，
  没有任何组件读取 `result.adaptationLoss`；
- `grep -rn "adaptationLoss" src/locales/*.ts` **零命中**。`LogsPage.tsx` 会渲染 `resultKey`
  （`logs.…` 机制），而 `src/i18n.ts` 的既定策略是「键缺失时返回键本身并把问题暴露出来」，
  因此日志页会显示裸键名 `result.adaptationLoss`，而不是一句人话。

**影响**
「无法表达的字段记录为 losses，不假装生效」这条承诺在**核心层成立**（这是实测确认的好消息，
`chat.rs` 的文本/工具/图片/`store`/`include`/`prompt_cache_key` 分支都真的记了 loss），
但在**产品层不可见**：用户看不到「这次请求丢了哪个参数」，只能看到一条会说不出人话的日志。

**修法建议**
- 诊断事件里带上 `losses`（`safeMetadata` 允许结构化字符串，可放 `loss.message_key` + `detail`）；
- 在 `src/locales/{zh-CN,en}.ts` 补齐 `loss.*` 与 `result.adaptationLoss` 文案键；
- 若产品期望，在连接/诊断页对「最近一次请求的 losses」给一行提示。

---

## S11 Windows 没有 fail-fast：helper 是失败桩，但配置照写、界面照报成功

**结论**
Windows 的凭据 helper 是显式的失败桩（README 亦自承），但**没有任何代码阻止在 Windows 上应用配置**，
也没有把「本平台不支持」变成计划层的 `Blocked`。用户会在 Windows 上得到「已应用并重启成功」，
然后每个请求都拿不到令牌。

**证据**
- `crates/switch-core/src/gateway/helper.rs:113-119`：
  ```rust
  #[cfg(windows)]
  fn script(_instance_id: &str) -> String {
      String::from("@echo off\r\necho gptswitch-auth-helper: not implemented for windows >&2\r\nexit /b 1\r\n")
  }
  ```
  且 `install()`（`helper.rs:36-63`）对平台无分支；`main.rs:60` 无条件调用它。
- 平台分支审计：`grep -rn "cfg(windows)\|Platform::Windows" src-tauri/src crates/switch-core/src/application`
  只命中 `main.rs:146`（`open_host_app`）——应用/计划路径没有任何平台闸门；
- 文案表里只有 `error.keystoreUnsupported`（`en.ts:333`），没有「Windows 尚不支持」类键。
- 其它平台分支的实情（供评估）：
  - `platform/mod.rs:430-444` `restrict()` 非 unix 分支是空实现（`let _ = (path, mode)`），
    Windows 上令牌文件与 helper 目录**完全不设权限**，只依赖用户目录 ACL；
  - `platform/mod.rs:414-427` `private_file_mode` / `private_dir_mode` 在 Windows 返回 `None`；
  - `platform/mod.rs:104-114` Windows 重启计划只有 `taskkill /IM … /F`（没有优雅档），
    `quit_force: None`，因此 `restart_host` 的所有平台分支里 Windows 的「强制退出」语义无法区分；
  - `platform/mod.rs:362-390` 窗口策略、`config_root_candidates`、`host_process_name`、
    `helper_file_name`（`.cmd`）、`host_executable_names`（`.exe`）都有真实实现。
- 结论：**Windows 上能做**——打开界面、检测实例、读写 SQLite/凭据库、生成目录、写 `config.toml`、
  重启宿主、导出诊断；**不能做**——宿主取不到本机令牌，因此所有推理请求不可用；
  且 token 文件没有额外的权限收紧。

**修法建议**
在 `plan_apply` / `plan_restore` 里对 `Platform::current() == Windows && !helper_supported()` 返回
`Blocked` 计划（带明确 message_key），或在 `commands::apply_execute` 前置拒绝；
实现 Windows helper 时用 PowerShell 读令牌文件（并明确 ACL），同时补齐 `restrict()` 的 Windows 实现。

---

## S12 `FileJournal` 是死代码

**结论**
`storage/journal.rs` 里的完整文件型 journal（`.jsonl`、`read_all` + `rewrite`）在生产路径没有使用，
装配用的是 SQLite：`src-tauri/src/main.rs:172-173` `SqliteOperationStore::open(&db_path)`。
`grep -rn "FileJournal" crates/switch-core/src src-tauri/src` 只命中 `journal.rs` 自身。

**影响**
- 文档描述的 journal（stage / `expected_config_hash` / `written_hash` / `backup_ref` / `finished`）
  与实际持久化（`OperationState` + `apply` 状态机）是两套模型，读者会被误导；
- `FileJournal::append` 是「读全文 → 改 → 原子重写全文」，若要启用需先解决 S2 的并发写问题。

**修法建议** 若不再计划启用，删除或在模块头注明「未接线、仅设计参考」；
若要启用，明确它与 `OperationStore` 的职责边界（谁决定恢复动作）。

---

## S13 `PendingApplyBar` 的错误归因不准

**结论**
`PendingApplyBar.confirmApply` 把所有重启失败原因都显示成「Codex 仍在运行」。

**证据** `src/app/PendingApplyBar.tsx:80-84`：
```ts
const report = await client.restartHost(await instanceIdOf(client)).catch(() => null);
if (!report || !report.quitConfirmed || !report.launchedConfirmed) {
  showToast(t('codex.restartHostStillRunning'), 'danger');
```
`!report` 覆盖了所有异常：`instanceIdOf` 在没检测到实例时返回 `''` → `restart_host('')` →
`desktop.instance("")` 返回 not found；实例没有 `app_path` 时 `commands.rs:353-356` 返回 validation 错误
（「这个实例没有可重启的应用路径；请手动重开 Codex」）——这两种情况都会被说成「Codex 仍在运行」。
同一文件 `CodexConfigPage.tsx:112-131` 的做法是对的（`restartNotice` 区分三种情况），两处不一致。

**修法建议** 复用 `CodexConfigPage` 的 `restartNotice`（提到共享模块），或至少把 `!report` 与
`quitConfirmed === false` 分开表达。

**补充（针对任务书里的重试问题）**：自动重试是**单次**、不是循环——
`CodexConfigPage.tsx:159-168` 与 `PendingApplyBar.tsx:61-75` 都是 `.catch()` 里 `planOf` 一次再 `execute`，
重试再失败会把错误抛给用户。因此**没有无限循环风险**。但存在两处状态副作用：
① 每次重试都会新建一个 operation 与一个新目录修订（配合 S4 会累积）；
② 失败的那次 operation 停留在 `Conflict`，`applied_summary()`（`apply.rs:558-596`）按
`ORDER BY rowid`（`sqlite.rs:460`）取最后一条符合条件的记录，所以「当前生效」显示的仍是新事务，语义正确。

---

## S14 构建与文档一致性（低）

| 问题 | 证据 | 影响 |
| --- | --- | --- |
| `rust-toolchain.toml` 不存在，但 CI 三处注释引用它 | `ls rust-toolchain*` 无输出；`.github/workflows/ci.yml`（core/lint 与 release 的 test 均有「与 rust-toolchain.toml 一致」注释） | 本地工具链版本可能与 CI 固定值（`dtolnay/rust-toolchain@master` + `toolchain: 1.88.0`）不同，fmt/clippy 结果漂移；应补文件或改注释 |
| 隐私清单文件名前后不一致 | `scripts/check-publish-safety.sh:6` 说明写 `~/.gptswitch-private-patterns`，实现（第 121 行）读 `$HOME/.switchelp-private-patterns`；README.md:73 与 README.zh-CN.md:60,63 也写 `gptswitch` | 按文档配置的私有特征清单**永远不会被读取**，第 ⑦ 项扫描静默失效（fail-open） |
| README 下载表版本过期 | README.md:36-38 / README.zh-CN.md:29-31 写 `0.1.3` 产物名，仓库与 `tauri.conf.json` 是 `0.2.0` | 用户下载到的与实际版本对不上；CI 的版本一致性检查只覆盖 `package.json`/`tauri.conf.json`/`Cargo.toml`，不含 README |
| `minimumSystemVersion` 未在 README 出现 | `src-tauri/tauri.conf.json` `bundle.macOS.minimumSystemVersion = "12.0"`；`grep -n "minimumSystemVersion\|12\.0" README*.md` 无命中 | 系统要求无处可查（任务书要求的「与 README 一致」实为「README 未声明」） |
| `bundle.targets: "all"` | `tauri.conf.json` `bundle.targets = "all"` | macOS 上会产生 dmg+app（release.yml 只归集 `*.dmg` 与 app zip，未收集 `*.app.tar.gz`；`all` 在 macOS 还含 `updater` 目标时可能产出额外文件） |
| 已跟踪文件版本号一致 | `package.json`/`tauri.conf.json`/`Cargo.toml` 均 `0.2.0`；`bundle.identifier = app.gptswitch.desktop` | ✅ 一致（CI test job 也会校验） |

---

## 分层与 IPC 边界（任务 1）：结论是合格的

- **`switch-core` 不依赖窗口框架**：`cargo tree -p switch-core --depth 1` 只列出
  `keyring/percent-encoding/rusqlite/serde/serde_json/sha2/thiserror/time/tokio/toml_edit/tracing/ureq/uuid/zeroize`
  （`src-tauri` 不在其中）；`grep -rn "tauri\|wry\|webview" crates/switch-core/`
  只有 `examples/g0_seed_app.rs:36` 与 `lib.rs:3` 的注释命中。**确认成立。**
- **`src-tauri` 是薄壳**：`commands.rs`（877 行）只有 DTO 映射 + `authorize()` + `spawn_blocking`；
  业务判定都在 core（如 `commands.rs:264-281` 的 inspect 只是组织 `ConfigSnapshot` 的输出）。
- **前端拿不到密钥**：`Credential`（`domain/credential.rs:68-82`）只有 `secret_ref` / `masked_suffix`，
  没有明文字段；`grep -rn "expose()\|resolve_secret" src-tauri/src/commands.rs` 只有
  `models_discover`（745-746）与 `probes_start`（815-822）内部使用，二者都不把 secret 放进返回值；
  备份预览走 `read_masked`（`commands.rs:519-528`）。**前端只在新增/替换 Key 时短暂持有明文，符合设计。**
- **命令面没有越权**：35 个命令全部经 `authorize()`（`commands.rs:84-101`）校验窗口 label（`main`）
  与 URL（`tauri://localhost` / `http(s)://tauri.localhost`，dev 下额外限 `localhost:5173` 且仅 debug 构建）。
  路径类参数只用于实例查找（`desktop.instance(id)` 来自检测结果），
  `host_restart` 的路径只从已检测实例取（`commands.rs:352-356`），渲染层传不了任意路径。
- **capabilities/main.json 不过宽**：只有 `core:default` + `allow-set-theme` / `allow-start-dragging` /
  `allow-internal-toggle-maximize`，`windows: ["main"]`。查 `src-tauri/gen/schemas/acl-manifests.json`，
  `core:default` = `path/event/window/webview/app/image/resources/menu/tray` 的 default 集，
  **不含** `fs` / `shell` / `http` / `dialog` 等插件。真正的 IPC 边界是 `authorize()`（Tauri 2 里
  自定义命令不受 capability 限制），这一层存在且合理。

---

## 构建与 CI：真实门禁

**本地实测（全部通过）**

| 命令 | 结果 |
| --- | --- |
| `pnpm build`（`tsc --noEmit && vite build`） | `EXIT=0`；`1692 modules transformed`，`dist/assets/index-*.js 445.88 kB │ gzip 132.13 kB`；无告警 |
| `cargo build -p switch-core --release` | `EXIT=0`，无 warning |
| `cargo test -p switch-core` | 全绿：325 + 20 + 24 + 21 + 7 + 13 通过、0 失败；3 个 `#[ignore]`（见未验证项） |
| `cargo clippy -p switch-core --all-targets -- -D warnings` | 干净 |
| `cargo clippy --workspace --all-targets -- -D warnings` | `EXIT=0`（含 `src-tauri`，编译 tauri 2.11.5） |
| `cargo fmt --all --check` | 干净（退出码 0） |
| `pnpm test`（vitest） | 15 个文件 / 112 个用例全部通过 |
| `scripts/check-publish-safety.sh` | 对**已跟踪**文件通过（唯一的红项来自未跟踪的本地审计文档，属脚本按设计扫描未跟踪文件） |

**《ci.yml》的门禁（PR + push main，5 个 job，每个都是真门禁，无 fail-open）**
1. `frontend`：`pnpm install --frozen-lockfile` → `pnpm typecheck` → `pnpm test` → `pnpm build`；
2. `core`：`cargo test -p switch-core`（ubuntu，Rust 1.88.0 固定）；
3. `lint`：`cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`
   （**必须 macOS runner**：`--workspace` 会编 `src-tauri`，Linux 缺 webkit2gtk）；
4. `privacy`：`scripts/check-publish-safety.sh`（含未跟踪文件）；
5. `workflows`：`raven-actions/actionlint@v2`。

**《release.yml》的门禁与 fail-open（tag `v*` 或手动）**
- `test` job 是真正的发布前闸门：`pnpm typecheck` + `pnpm test` + `cargo test -p switch-core` +
  `cargo fmt --all --check` + `cargo clippy -p switch-core --all-targets -- -D warnings`，
  再校验三处版本号一致、以及标签与版本号一致（`v$pkg`）。**这一层不含糊。**
- **fail-open 点（按设计，但确实不拦）**：
  - `publish` 的 `if` 允许两个构建 job 都是 `skipped`，且 `fail_on_unmatched_files: false`、
    产物为空时只打一条 notice（`release.yml` publish 步骤）→ **在未配置 Apple 签名的仓库里，
    打标签会稳定产出一个「没有任何安装包的绿色 Release」**；
  - `build-macos` 在 `APPLE_SIGNING_IDENTITY` 缺失时整体跳过（`gate` job 计算 `signed`）；
  - `build-windows` 条件是 `github.event_name == 'workflow_dispatch' && inputs.with_windows` →
    **tag 触发时永不构建 Windows**，README 的 Windows 条目只能靠手动流程；
  - 因此「发布门禁」实际只保证「测试通过 + 版本一致」，**不保证有产物**。
- **CI 不做的事**：不跑 `tauri build`（不产包、不校验 bundle/签名配置）、不跑 `scripts/g0/probe-full-loop.mjs`
  端到端验收（需要真 Codex 与本机 app 包）、不跑 `src-tauri` 的单测（无）、不扫描 README 的版本号。

---

## 协议与网关安全（任务 4/5）的其余实测结果（正面）

对真实网关（正确令牌）的其余攻击全部**行为正确**：

```
### 1  health                      → 200 {"served":…,"status":"ok"}
### 2  GET models (已发布版本)      → 200 列出 alias（形如 gs/<providerId>/<modelId>）
### 4  wrong token                 → 401 UNAUTHORIZED "令牌不匹配"
### 5  no token                    → 401 "缺少 Bearer 令牌"（两类文案不同但码/键一致，不泄露令牌是否存在）
### 6  Host: evil.example.com      → 401 "Host 不是本机地址：evil.example.com"（防 DNS rebinding）
### 7  Origin: https://evil…       → 401 "该入口不接受带 Origin 的请求"
### 8  OPTIONS + Access-Control-…  → 401 "该入口不接受浏览器预检请求"
### 10 {not json                   → 400 VALIDATION_FAILED "请求体不是合法 JSON"
### 12 model = vendor/Upstream-Model（原生 slug） → 404 ROUTE_MISMATCH（**按别名准入成立**，且上游未收到请求）
### 13 PUT /responses              → 400 "不支持的方法 PUT"
### 14 未知路径                     → 404
### 15 POST /realtime               → 400 CAPABILITY_UNSUPPORTED
### 16 Content-Length: 40 MiB      → 413 REQUEST_TOO_LARGE（按声明长度先拒，不读进内存）
### 17 transfer-encoding: chunked   → 400 "本机网关不接受分块请求体"
```
- **只绑 127.0.0.1**：`server.rs:108` `SocketAddr::from((Ipv4Addr::LOCALHOST, port))`。✅
- **每次启动新令牌**：`main.rs:45` `GatewayToken::generate()` + `main.rs:60` 覆盖 helper；
  `helper.rs:36-63` 幂等覆盖；令牌 64 hex（`auth.rs:48-50` `is_strong_enough`）。✅
- **常量时间比较**：`auth.rs:74-83`，且长度差单独在 `usize` 上比（修掉了历史上的截断 bug，有回归测试）。✅
- **请求级超时**：`TimeoutPolicy` 三层（连接 10s / 首事件 90s / 流空闲 180s，`timeouts.rs:22-31`）；
  正文总预算默认关闭（`server.rs:79-82` 有明确理由：ureq 的 `recv_body` 是总预算会截断长回复）。✅
- **流式处理**：上游流中途错误帧会翻译成 `error` 事件而不是伪造成 `response.completed`
  （`server.rs:544-553`、`chat.rs` 的 `stream_unwraps_an_upstream_error_carried_in_the_chunk` 测试）；
  客户端断开即停止读取上游并释放连接（`server.rs:500-503`、`566-579`）；
  分块写出每帧 flush（`Chunked::write_chunk`）。✅
- **密钥脱敏**：上游错误正文经 `redact(text, secret)`（`server.rs:434`、`960-973`），
  截断 2000 字符；`upstream_error_body_never_leaks_the_upstream_key` 有测试。✅
- **上游请求体**（实测抓到）：`{"input":[{"content":[{"text":"hi",…}],"role":"user",…}],"max_output_tokens":8192,"model":"vendor/Upstream-Model","stream":true}`
  —— `model` 换成上游精确 ID、`max_output_tokens` 被模型声明的 8192 收口。✅
- **`max_output_tokens` 收口**：`protocols/mod.rs:62-68` `clamp_output_limit`（请求更小取请求、
  更大取上限、未指定时用上限）在两个适配器都执行，且记录 loss。✅

**配置写入链路（任务 3）的其余验证**：`plan → digest CAS → atomic replace` 的主干是**真的**——
CAS 比较摘要与文件存在性（`plan.rs:285-303`，`check_cas` 把「存在性变化」也算身份变化）；
`write_atomic` 有 fsync + 父目录 sync + 继承目标权限（`config.rs:882-913`）；
未受管字段保留由既有测试覆盖并通过（`crates/switch-core/tests/config_golden.rs`：
注释/未知字段/`[projects."…"]` 类节、CRLF、内联表、带引号键、Unicode 路径全部保留；
`atomic_write_leaves_no_partial_file`；`integer_notation_is_not_an_external_change`）。
`tests/apply_service.rs` 覆盖：计划阶段不碰配置、外部改动阻断提交并保留外部内容、
提交后只到 `AwaitingReload`、回执才升 `Verified`、幂等键复用同一 operation、过期计划被 `Blocked`、
被篡改的 plan_hash 在写入前被拒、恢复把已提交事务报告为等待重载。
**中断恢复**（`apply.rs:599-694` + `plan.rs:489-512`）逐条可判定：
写前已保存 `written_hash` → 崩溃在 write 之后为 `RecordCommitFromFile`（补记完成）；
崩溃在 write 之前为 `NoAction` → 转 `Failed` 并给「请重新预览」；
文件被外部改过为 `ConflictWithExternalChange`（不覆盖外部内容）。**这三条分支都有对应测试。**

---

## 未验证项

1. **Windows 真机**：完全未验证（helper 桩之外的一切：`.cmd` 调用链、ACL、`taskkill` 重启、窗口策略）。
2. **被 `#[ignore]` 的 3 个测试**（本机未运行）：
   `crates/switch-core/tests/system_vault.rs:4`（真实系统凭据库）、
   `crates/switch-core/tests/restart_host.rs:27,72`（会真的退出并重开用户正在用的 Codex，
   本审计遵守「不碰真实宿主」的边界）。
3. **真实第三方供应商的推理质量与兼容性**：未验证（README 亦自承未验证）。
4. **Tauri 打包产物**：未跑 `pnpm exec tauri build`（只跑了 `tsc`/`vite build` 与
   `cargo build -p switch-core --release`），因此 bundle 配置、签名、公证、`bundle.targets: "all"`
   的实际产物清单未验证；`scripts/g0/probe-full-loop.mjs` 端到端验收未运行（需要真 Codex + app 包）。
5. **SSE 恶意流的端到端注入**：S8 的内存增长与线程占用是代码分析 + 「10MB 请求行」/「30s 半截 body」
   两个实测，未对「上游持续发送不含换行的字节」做真实注入。
6. **macOS 12（`minimumSystemVersion`）兼容性**：未在 12.x 机器上验证。
7. **多窗口 / 多实例并发**：S6 的跨进程冲突未做运行时验证（只做了同进程并发写实验）。
8. **`keyring` 在锁定钥匙串下的行为**：`CredentialStatus::KeystoreLocked` 的路径未在真实锁定状态下验证。
9. **前端自动重试的竞态**：只做了静态分析（单次重试、无循环），未做多标签/多窗口下的运行时验证。

---

## 附：本次审计产生的临时文件（均在仓库外）

- `/tmp/audit-temp/gateway_silent_fail_and_concurrent_write.rs`：S1 + S2 的可复现测试
  （`cp` 到 `crates/switch-core/tests/zz_audit.rs` 后 `cargo test -p switch-core --test zz_audit`）。
- `/tmp/audit-temp/example_zz_audit_probe.rs.bak`：起真实网关的探针，供 curl 攻击实验
  （打印 `PORT` / `TOKEN` / `ALIAS`，随后 sleep 900s）。
- 仓库工作区未留下任何改动（`git status` 只有其它审计者产出的未跟踪文档）。
