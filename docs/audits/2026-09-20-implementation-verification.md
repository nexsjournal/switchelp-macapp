# 实现验证审计：Switchelp 端到端链路（真机实测）

- 审计时间：2026-09-20 19:00–19:20（本地时间，UTC+8）
- 审计对象：`/Users/<user>/Code/ProjDev/gptswitch-macapp`（工作区版本 0.2.0，HEAD = `2fda2a5`）
- 宿主：`/Applications/ChatGPT.app`（Codex Desktop `26.915.31945`）+ `codex-cli 0.155.0-alpha.9.2`
- 方法：只看真实文件、真实进程、真实网络请求与真实系统日志；每条结论附「命令 → 原始输出 → 解读」
- 结论分类：**实测通过** / **实测失败** / **无法验证（附原因）**

## 0. 审计行为声明（对宿主状态的改动与还原）

| 行为 | 影响 | 是否还原 |
| --- | --- | --- |
| 备份 `~/.codex/config.toml` 到 `/tmp/config.toml.backup-before-audit` | 只读复制 | 不需要还原 |
| 启动一次已构建的发行版 app（`target/release/bundle/macos/Switchelp.app/.../gptswitch`，pid 28798）测网关 | 该 app 启动时轮换了 `gateway-token`（19:01:21）并覆写 helper；**没有**改写 `config.toml` | 已 kill，端口已释放 |
| 用真实上游发一次极小推理请求（1 条消息，回复 2 字） | 消耗极少量上游额度 | 不可撤销（数据已产生） |
| 跑 `scripts/g0/*.mjs` 三个探针 | 全部使用隔离 `CODEX_HOME` / `GPTSWITCH_TEST_DATA_DIR`；`probe-full-loop` 临时写入并删除 1 条 Keychain 条目 | 已核对无残留 |
| 新增 1 个后台 app 进程（pid 36698，debug 构建，隔离数据目录）复现缺陷 | 只写 `/tmp` | 已 kill 并删除 `/tmp/gptswitch-*` |

还原核对：

```
$ shasum -a 256 ~/.codex/config.toml /tmp/config.toml.backup-before-audit
0dc8fb41682a8cc0f54a8d37298c4c9e6450200d8bd6aa5ea8bd044a78236f46  /Users/<user>/.codex/config.toml
0dc8fb41682a8cc0f54a8d37298c4c9e6450200d8bd6aa5ea8bd044a78236f46  /tmp/config.toml.backup-before-audit

$ lsof -nP -iTCP:18765        # 审计结束后
（无输出 = 端口未监听，与审计开始时一致）
```

```
$ DB=".../app.gptswitch.desktop/metadata.sqlite"
$ echo "providers=$(sqlite3 "$DB" 'select count(*) from providers;') models=$(sqlite3 "$DB" 'select count(*) from models;') operations=$(sqlite3 "$DB" 'select count(*) from operations;') creds=$(sqlite3 "$DB" 'select count(*) from credentials;')"
providers=1 models=1 operations=6 creds=1     # 与审计开始时相同，真实库未被污染
```

审计**没有**在真实 `~/.codex/config.toml` 上做任何破坏性实验，**没有**修改任何业务代码，只新增本报告文件。

---

## 1. 宿主现状（config.toml 受管字段 + app 数据目录 + SQLite 事务）

### 1.1 受管字段

```
$ cat ~/.codex/config.toml | grep -nE "^(model|model_provider|model_catalog_json|model_reasoning_effort)"
5:model_reasoning_effort = "high"
7:model = "gs/57745d8a-08f4-4e72-8b64-1dc95f4699be/14f3fca6-1920-4f29-b4d0-5e32ccde5397"
8:model_provider = "gptswitch"
9:model_catalog_json = "/Users/<user>/Library/Application Support/app.gptswitch.desktop/catalogs/rev_c9a0dbf7ff24a147/models.json"

$ sed -n '/\[model_providers.gptswitch\]/,/refresh_interval_ms/p' ~/.codex/config.toml
[model_providers.gptswitch]
name = "Switchelp"
base_url = "http://127.0.0.1:18765/i/inst_feea2e927725590c/c/rev_c9a0dbf7ff24a147/v1"
wire_api = "responses"

[model_providers.gptswitch.auth]
command = "/Users/<user>/Library/Application Support/app.gptswitch.desktop/bin/gptswitch-auth-helper"
args = ["--instance", "local-main"]
timeout_ms = 5000
refresh_interval_ms = 300000
```

**解读（实测通过）**：受管字段共 4 类——`model`、`model_provider`、`model_catalog_json`、`model_providers.gptswitch{,.auth}`。配置形态与 README「How it works」链路图完全一致：宿主 → 本机网关 `127.0.0.1:18765/i/<instance>/c/<rev>/v1` → helper 取本地令牌。`base_url` 里的 `rev_c9a0dbf7ff24a147` 与 `model_catalog_json` 指向同一目录版本，二者一致（这是路由能命中的前提）。`model_reasoning_effort = "high"` 仍在配置里，但该模型目录声明的思考档位为空（见 2.2），即一个**声明与宿主默认值不匹配**的状态（实测未导致失败，见 3.5）。

### 1.2 app 数据目录结构

```
$ ls -la "/Users/<user>/Library/Application Support/app.gptswitch.desktop/"
drwx------@  9 lex  staff    288 Sep 20 16:48 .
drwx------@  3 lex  staff     96 Sep 20 16:46 bin
drwxr-xr-x@  5 lex  staff    160 Sep 20 18:24 catalogs
drwx------@  6 lex  staff    192 Sep 20 18:39 backups
-rw-------@  1 lex  staff     64 Sep 20 18:42 gateway-token
-rw-------@  1 lex  staff  57344 Sep 20 16:46 metadata.sqlite

$ ls -la .../bin/
-rwx------@ 1 lex  staff  511 Sep 20 18:42 gptswitch-auth-helper     # 一个 /bin/sh 脚本

$ ls .../catalogs/
rev_173105a5b1190f77   rev_c63a6e34074cc5e5   rev_c9a0dbf7ff24a147
```

**解读（实测通过）**：数据目录 `0700`、`metadata.sqlite` `0600`、`gateway-token` `0600`、helper `0700`，与 README「Security boundaries」表一致。目录树里**没有** `diagnostics` 文件——诊断日志是进程内 `Arc<DiagnosticLog>`（`src-tauri/src/main.rs:172`），不落盘，因此无法从磁盘复核历史诊断。三个 `rev_*` 目录都是 1088 字节的单文件 `models.json`，即每次应用都会新建一个不可变目录版本（旧版本保留，供旧前缀继续服务）。

### 1.3 SQLite 表结构与事务流水

```
$ sqlite3 .../metadata.sqlite ".tables"
credentials  models  operations  providers  revisions

$ sqlite3 .../metadata.sqlite ".schema models"
CREATE TABLE models (
    id TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE RESTRICT,
    upstream_id TEXT NOT NULL COLLATE BINARY,
    protocol TEXT NOT NULL,
    alias TEXT NOT NULL UNIQUE COLLATE BINARY,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    UNIQUE(provider_id, upstream_id, protocol)
);
# providers / credentials / revisions / operations 结构同构：id + payload(json_valid)
```

**解读（实测通过）**：`UpstreamId`/`Alias` 双列 + `alias` 全局唯一约束，说明「别名准入」是**库级约束**而不是约定；`credentials.payload` 里没有明文字段，只有 `secretRef`（见第 4 节）。

```
$ sqlite3 .../metadata.sqlite "select count(*) from operations;"
6

$ # 按 operation.stage 直方图（payload 内嵌 operation 对象）
total 6
stage histogram: Counter({'prepared': 3, 'awaiting_reload': 2, 'verified': 1})

kind     id       stage            rev                     events  首次(UTC)                末次(UTC)
apply    fcd00cfa awaiting_reload  rev_c63a6e34074cc5e5    4       2026-09-20T08:48:46.3Z   2026-09-20T08:48:50.2Z
apply    0114b8bf prepared         rev_173105a5b1190f77    2       2026-09-20T09:08:52.3Z   2026-09-20T09:08:52.3Z
restore  dcb0cf45 prepared         restore                 2       2026-09-20T09:48:49.1Z   2026-09-20T09:48:49.1Z
restore  dcd55b37 verified         restore                 5       2026-09-20T10:20:17.2Z   2026-09-20T10:20:21.6Z
apply    5c99185d prepared         rev_c9a0dbf7ff24a147    2       2026-09-20T10:24:08.8Z   2026-09-20T10:24:08.8Z
apply    684fedce awaiting_reload  rev_c9a0dbf7ff24a147    4       2026-09-20T10:39:01.0Z   2026-09-20T10:39:02.2Z

$ # 最后一条 apply（684fedce）的完整阶段迁移
1 validating      2026-09-20T10:39:01.019Z  stage.validating
2 prepared        2026-09-20T10:39:01.019Z  stage.prepared
3 committing      2026-09-20T10:39:02.220Z  stage.committing
4 awaiting_reload 2026-09-20T10:39:02.234Z  stage.awaitingReload
```

**解读（实测通过，但需纠正一个提法）**：

- 任务里要求统计的 `committed` / `conflict` / `rolled_back` 三种 stage **本机一条都没有**。`ApplyStage` 枚举（`crates/switch-core/src/codex/plan.rs:20`）里也没有 `committed` 或 `rolled_back` 这两个名字——对应的是瞬态 `Committing`（不落库为终态）、终态 `Conflict`、以及 `RollingBack → Restored`。因此「历史事务 stage 分布」的准确写法是：**Prepared 3 / AwaitingReload 2 / Verified 1，Conflict 0 / RollingBack 0 / Failed 0**。
- 2 条 `AwaitingReload` 从未推进到 `Verified`，说明「应用成功即停等待重载、绝不宣称宿主已加载」这条设计在真机上被遵守（README 的对应声明成立）。
- 最后一条 apply 的 `committing → awaiting_reload` 只隔 14ms，且配置写入发生在 `10:39:02.220Z`——这个时间点在第 7 节的重启证据里是关键锚点。
- 6 条里 3 条停在 `Prepared` 且从未执行（`prepared` 阶段的 `check_execution` 返回 `None`），属于用户反复预演后未提交的正常残留，不是失败。

---

## 2. 目录文件与宿主模型缓存

### 2.1 `catalogs/rev_c9a0dbf7ff24a147/models.json`（当前生效版本）

```
$ cat .../catalogs/rev_c9a0dbf7ff24a147/models.json
{
  "models": [
    {
      "slug": "gs/57745d8a-08f4-4e72-8b64-1dc95f4699be/14f3fca6-1920-4f29-b4d0-5e32ccde5397",
      "display_name": "deepseek-v4.1",
      "description": "由 Switchelp 管理；上游模型 ID：deepseek-v4.1",
      "default_reasoning_level": null,
      "supported_reasoning_levels": [],
      "shell_type": "unified_exec",
      "visibility": "list",
      "supported_in_api": true,
      "truncation_policy": { "mode": "tokens", "limit": 488000 },
      "context_window": 1000000,
      "max_context_window": 1000000,
      "effective_context_window_percent": 95,
      "input_modalities": ["image", "text"],
      "supports_reasoning_summary_parameter": false,
      "supports_search_tool": false
    }
  ]
}

$ diff .../catalogs/rev_c63a6e34074cc5e5/models.json .../catalogs/rev_c9a0dbf7ff24a147/models.json
（无差异）      # 两次 apply 只换了目录路径前缀，模型内容相同
```

**解读（实测通过）**：目录里只有 1 个模型，slug 是**编译出的别名** `gs/<providerId>/<modelInstanceId>`，而 `description` 里记录了真实上游 ID `deepseek-v4.1`——别名与上游 ID 是分离的。能力字段齐全：上下文 1000000、truncation 488000、模态 `[image, text]`、`supported_reasoning_levels: []`（无思考档位）。**注意 `truncation_policy.limit = 488000` 与库里的 `outputLimit = 512000` 不一致**（见 2.2 的对比），这两者一个是宿主侧截断策略、一个是路由期收口值，审计未能确认这种不一致是有意为之还是缺陷，**无法验证**。

### 2.2 库里声明的策略（`models.payload`）

```
$ sqlite3 .../metadata.sqlite "select payload from models;" | python3 -m json.tool
{
  "upstreamId": "deepseek-v4.1",
  "catalogAlias": "gs/57745d8a-.../14f3fca6-...",
  "hostState": "awaiting_reload",
  "inCatalog": true,
  "policy": {
    "contextLimit": 1000000,
    "outputLimit": 512000,
    "reasoning": { "support": "unknown", "control": "none", "allowedValues": [], "defaultValue": null },
    "inputs": [
      {"kind":"text",  "upstream":"supported", "gateway":"supported",  "host":"supported",  "effectivePath":"native"},
      {"kind":"image", "upstream":"supported", "gateway":"supported",  "host":"supported",  "effectivePath":"native"},
      {"kind":"audio", "upstream":"unsupported","gateway":"unknown",    "host":"unknown",    "effectivePath":"blocked","blockedReasonKey":"capability.reason.upstreamUnsupported"},
      {"kind":"video", "upstream":"unknown",     "gateway":"unsupported","host":"unsupported","effectivePath":"blocked", ...},
      {"kind":"pdf",   "upstream":"unknown",     "gateway":"unsupported","host":"unsupported","effectivePath":"blocked", ...},
      {"kind":"document","upstream":"unknown",   "gateway":"unsupported","host":"unsupported","effectivePath":"blocked", ...}
    ]
  },
  "capabilityRevision": 2, "version": 6
}
```

**解读（实测通过）**：宿主层能力（`host` 列）由 `WorkspaceService` 重算而不是照抄用户输入（`audio` 被标 `blocked` 而非 `supported`），说明「声明可转发 / 实际可否走原生通道」是分开建模的。`hostState = awaiting_reload` 与操作流水的 stage 一致。

### 2.3 别名准入（「不能把原生 slug 塞进目录」）

```
$ T=$(.../bin/gptswitch-auth-helper --instance local-main)
$ curl -s --noproxy '*' -i -X POST -H "Authorization: Bearer $T" -H "content-type: application/json" \
    -d '{"model":"deepseek-v4.1","input":"hi","stream":false}' \
    http://127.0.0.1:18765/i/inst_feea2e927725590c/c/rev_c9a0dbf7ff24a147/v1/responses
HTTP/1.1 404 Not Found
{"error":{"code":"ROUTE_MISMATCH","details":["目录版本 rev_c9a0dbf7ff24a147 不包含 alias deepseek-v4.1"],"message_key":"error.unknownAlias","retryable":false}}

$ # 同一个目录前缀，换用目录里的别名
（见 3.5，200 + 流式回复）
```

**解读（实测通过）**：原生 slug `deepseek-v4.1` 被网关按 `unknownAlias` 拒绝，只有目录里编译出的别名可通过。memory 里「网关按别名准入，不能把原生 slug 塞进目录」这条**在真机上成立**。

### 2.4 `~/.codex/models_cache.json`

```
$ stat -f "%N mtime=%Sm size=%z" ~/.codex/models_cache.json
/Users/<user>/.codex/models_cache.json mtime=Sep 20 18:29:37 2026 size=229177

$ grep -c "gs/" ~/.codex/models_cache.json          # 0
$ grep -c "Switchelp" ~/.codex/models_cache.json    # 0
$ python3 -c "import json;d=json.load(open('$HOME/.codex/models_cache.json'));print(len(d['models']),[m['slug'] for m in d['models']])"
7 ['gpt-6-astra', ...]        # 全是原生 OpenAI 模型，无 gs/ 别名
$ head -c 200 ~/.codex/models_cache.json
{ "fetched_at": "2026-09-20T10:29:37.495385Z", "etag": "W/\"cc84b142c478d2fdd3d1257f1a8eefc2\"",
  "client_version": "0.155.0", "identity": "ae140d..." , "models": [ ... ] }
```

**解读（实测通过）**：`models_cache.json` **没有被本工具改写**。它由 Codex 自身的在线刷新写入（含 `etag` / `identity` / `fetched_at`，`client_version 0.155.0`），内容只有 7 个原生模型，不含任何 `gs/` 别名。宿主侧自定义模型走的是 `model_catalog_json` 这条独立路径，与这份缓存无关。对应日志：

```
$ sqlite3 ~/.codex/logs_2.sqlite "select datetime(ts,'unixepoch','localtime'),target,substr(feedback_log_body,1,120) from logs where feedback_log_body like '%list_models%' order by ts desc limit 2;"
2026-09-20 18:29:37|INFO|feedback_tags|list_models{refresh_strategy=online}:endpoint_session.execute_with{http.method=GET api.path="models"
```

---

## 3. 网关（本机 18765）

### 3.1 端口与进程（审计开始时）

```
$ lsof -nP -iTCP:18765        # 审计开始时
（无输出）

$ ps aux | grep -iE "switchelp|gptswitch" | grep -v grep
# 只有 vite/esbuild 开发服务器，没有 app 进程
```

**解读（实测失败——指「开箱即用」）**：审计开始时**网关并未运行**，app 进程也不在。也就是说：`config.toml` 里指向 `127.0.0.1:18765` 的链路**在宿主不重启、Switchelp 不启动时就断**。用户当前的 Codex 处于「配置指向本工具、但本工具没在跑」的状态——这正是下面第 3.5 节必须先把 app 拉起来才能测的原因。

启动已构建的发行版 app：

```
$ ./target/release/bundle/macos/Switchelp.app/Contents/MacOS/gptswitch > /tmp/switchelp-audit-app.log 2>&1 &
pid=28798
$ sleep 6; lsof -nP -iTCP:18765
COMMAND     PID USER   FD   TYPE  NODE NAME
gptswitch 28798  lex   11u  IPv4  TCP 127.0.0.1:18765 (LISTEN)
```

**解读（实测通过）**：app 启动即拉起网关，**只绑定 IPv4 loopback**（不是 `0.0.0.0`），与 README 声明一致。

### 3.2 无 token / 错误 token → 拒绝

```
$ curl -s --noproxy '*' -i http://127.0.0.1:18765/health
HTTP/1.1 401 Unauthorized
{"error":{"code":"UNAUTHORIZED","details":["缺少 Bearer 令牌"],"message_key":"error.unauthorized","retryable":false}}

$ curl -s --noproxy '*' -i .../i/inst_feea2e927725590c/c/rev_c9a0dbf7ff24a147/v1/models
HTTP/1.1 401 Unauthorized
{"error":{"code":"UNAUTHORIZED","details":["缺少 Bearer 令牌"], ...}}

$ curl -s --noproxy '*' -i -H "Authorization: Bearer wrongtoken" .../v1/models
HTTP/1.1 401 Unauthorized
{"error":{"code":"UNAUTHORIZED","details":["令牌不匹配"], ...}}
```

**解读（实测通过）**：认证在路由分发**之前**执行，所以连 `/health` 都要令牌（`admin` 类信息不泄露）。三个响应结构一致、只有 `details` 不同，`error.unauthorized` 与 `codex/gateway/auth.rs` 的 `auth_failure_does_not_reveal_whether_a_token_exists` 单测意图一致。

### 3.3 带 Origin / 浏览器预检 → 拒绝

```
$ curl -s --noproxy '*' -i -H "Authorization: Bearer $T" -H "Origin: https://evil.example.com" .../v1/models
HTTP/1.1 401 Unauthorized
{"error":{"code":"UNAUTHORIZED","details":["该入口不接受带 Origin 的请求"], ...}}

$ curl -s --noproxy '*' -i -X OPTIONS -H "Origin: https://evil.example.com" -H "Access-Control-Request-Method: POST" .../v1/models
HTTP/1.1 401 Unauthorized
{"error":{"code":"UNAUTHORIZED","details":["该入口不接受浏览器预检请求"], ...}}
```

**解读（实测通过）**：即使携带**正确令牌**，只要带 `Origin` 就被拒；预检（`Access-Control-Request-Method`）同样被拒。README「rejects requests that carry `Origin` or browser preflight」这条**证真**（对照 `crates/switch-core/src/gateway/auth.rs:142-147`）。DNS rebinding 防护（Host 必须是 loopback）也在同一校验器里，本次未单独发包验证其边界（单测覆盖）。

### 3.4 `/health` 与 `/models`（正确令牌）

```
$ T=$(.../bin/gptswitch-auth-helper --instance local-main); echo "len=${#T}"
len=64
$ curl -s --noproxy '*' -i -H "Authorization: Bearer $T" http://127.0.0.1:18765/health
HTTP/1.1 200 OK
{"served":6,"status":"ok"}

$ curl -s --noproxy '*' -i -H "Authorization: Bearer $T" .../i/inst_feea2e927725590c/c/rev_c9a0dbf7ff24a147/v1/models
HTTP/1.1 200 OK
{"data":[{"id":"gs/57745d8a-08f4-4e72-8b64-1dc95f4699be/14f3fca6-1920-4f29-b4d0-5e32ccde5397","object":"model","owned_by":"gptswitch"}],"object":"list"}
```

**解读（实测通过）**：`/health` 与「按目录版本列别名」两个端点可用，且 `/models` 的返回集合与目录版本里的别名**逐一对应**（也是启动恢复确实发布了路由的直接证据，见 7.2）。

### 3.5 真实端到端推理（使用者的真实中转上游，非 mock）

```
$ curl -s --noproxy '*' -o /tmp/d-body.txt -w "http_code=%{http_code} time=%{time_total}\n" -m 60 \
   -X POST -H "Authorization: Bearer $T" -H "content-type: application/json" \
   -d '{"model":"gs/57745d8a-.../14f3fca6-...","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"只回复两个字：可用"}]}],"stream":true}' \
   http://127.0.0.1:18765/i/inst_feea2e927725590c/c/rev_c9a0dbf7ff24a147/v1/responses
http_code=200 time=3.133640

event: response.created
data: {"response":{"id":"resp_93ce9784...","model":"gs/57745d8a-.../14f3fca6-...","status":"in_progress",...},"type":"response.created"}
event: response.output_text.delta
data: {"delta":"可用","item_id":"msg_13432497...","type":"response.output_text.delta"}
event: response.output_text.done
data: {"text":"可用", ...}
...
```

**解读（实测通过；同时证伪 README 的一条「未验证」声明）**：这是本次审计最重要的一条证据——**用用户机器上真实配置的真实上游（`https://api.<上游域名>/v1`，chat_completions 协议）跑通了一轮完整推理**：宿主侧 Responses 请求 → 网关 → 上游 chat/completions 适配 → 流式还原 → 回写宿主。3.1 秒返回内容「可用」。

- README「Current status」写的是 *"it has **not been verified against a real third-party provider yet**"* / 中文版「真实供应商的响应质量……未验证」——**这句话在真机上已被证伪**：真实供应商的链路不仅验证过，而且此刻仍然可用。响应质量本身属于主观项，不在本次审计范围。
- 同时证伪了「网关只能对着 mock 工作」这一潜在怀疑：mock 探针（第 5 节）与真实上游都通过。

### 3.6 缺陷 A：`GET /models` 遇未知目录版本 → 空回复（连接被直接关闭）

```
$ curl -sv --noproxy '*' -H "Authorization: Bearer $T" \
    http://127.0.0.1:18765/i/inst_feea2e927725590c/c/rev_deadbeef/v1/models
* Connected to 127.0.0.1 (127.0.0.1) port 18765
> GET /i/inst_feea2e927725590c/c/rev_deadbeef/v1/models HTTP/1.1
> Authorization: Bearer <redacted>
* Request completely sent off
* Empty reply from server
* Closing connection

$ curl -s --noproxy '*' -o /dev/null -w "http_code=%{http_code}\n" ...同上
http_code=000
```

同一路径换 POST 推理则正常返回结构化 404：

```
$ curl -s --noproxy '*' -i -X POST .../i/inst_feea2e927725590c/c/rev_deadbeef/v1/responses
HTTP/1.1 404 Not Found
{"error":{"code":"ROUTE_MISMATCH","details":["该目录版本未发布：rev_deadbeef"],"message_key":"error.unknownCatalogRevision","retryable":false}}
```

用 debug 构建 + 隔离数据目录复现过第二次（同样 `http_code=000`、无 body）。

**解读（实测失败）**：这是**独立的、可复现的实现缺陷**。根因就在代码里：

```rust
// crates/switch-core/src/gateway/server.rs:230-248
match (request.method.as_str(), route_kind(&request.path)) {
    ("GET", RouteKind::Health) => write_json(...),
    ("GET", RouteKind::Models { revision }) => self.handle_models(&mut writer, &revision),  // ← 直接透传 Err
    ...
}
// crates/switch-core/src/gateway/server.rs:158-160（接受循环）
let _ = gateway.handle(stream);   // ← 错误被丢弃，没有任何兜底写响应
```

`handle_models` 在目录版本未发布时返回 `Err`（`server.rs:252-256`），而 `handle` 的 `Result` 在 `spawn` 的线程里被 `let _ =` 吞掉，于是**一个 HTTP 字节都没写**，客户端看到空回复。同一函数里 `/responses` 分支的错误都被显式 `write_error` 处理，只有 `/models` 这个分支漏了。影响：任何指向已回收目录前缀的 `GET /v1/models` 探测都会得到「连接被关闭」而不是可诊断的 404 JSON；对宿主正常推理路径无影响（宿主不调 `/models`）。

### 3.7 缺陷 B：`GET /models` 不校验 URL 前缀里的实例

```
$ # 令牌绑定实例 inst_feea2e927725590c，但 URL 前缀故意写成 inst_other
$ curl -s --noproxy '*' -i -H "Authorization: Bearer $T" \
    http://127.0.0.1:18765/i/inst_other/c/rev_c9a0dbf7ff24a147/v1/models
HTTP/1.1 200 OK
{"data":[{"id":"gs/57745d8a-.../14f3fca6-...","object":"model","owned_by":"gptswitch"}],"object":"list"}
```

对比推理路径会严格校验：

```
$ curl -s --noproxy '*' -i -X POST -H "Authorization: Bearer $T" -H "content-type: application/json" \
    -d '{"model":"gs/57745d8a-.../14f3fca6-...","input":"hi","stream":true}' \
    http://127.0.0.1:18765/i/inst_other/c/rev_c9a0dbf7ff24a147/v1/responses
HTTP/1.1 401 Unauthorized
{"error":{"code":"UNAUTHORIZED","details":["令牌属于实例 inst_other，请求指向实例 inst_feea2e927725590c"],"message_key":"error.instanceMismatch","retryable":false}}
```

**解读（实测失败）**：两处问题。

1. **实例绑定在 `/models` 上缺失**。`handle_models`（`server.rs:252`）只接受 `revision`，完全丢掉了 `admission_from_path` 那条「前缀实例必须等于快照实例」的校验；代码注释里写的「`admission` 仍会校验该目录修订确实属于前缀里声明的实例」只对推理路径成立。危害有限（仍需有效令牌，且只泄露别名集合），但**与设计文档/代码注释的声明不符**，属声明与实现偏差。
2. **`instanceMismatch` 的文案把两个实例名写反了**。实际 URL 前缀是 `inst_other`（请求指向），令牌属于 `inst_feea2e927725590c`；文案却说「令牌属于实例 inst_other，请求指向实例 inst_feea2e927725590c」。对应构造处 `crates/switch-core/src/gateway/routing.rs:206-210` 与 `:230-234`：字段 `expected` 被赋成 **URL 前缀实例**（`AdmissionError` 的字段命名把「URL 声明」叫 expected），而 `Actual` 是快照/令牌实例，但渲染模板（`routing.rs:59-61`）假定 `expected` 是令牌方。用户按提示排查会找错方向。

---

## 4. 凭据边界（核心安全声明）

### 4.1 上游 Key 不在 `config.toml`

```
$ grep -inE "api[_-]?key|secret|sk-|token" ~/.codex/config.toml
（无匹配）

$ grep -n "Bearer\|authorization\|experimental_bearer" ~/.codex/config.toml
（无匹配）
```

**解读（实测通过）**：连 `experimental_bearer_token` 这类 Codex 原生支持的「把 token 写进 config」字段都没有被使用，配置里只有 helper 命令。

### 4.2 上游 Key 在系统凭据库，SQLite 里只有引用与掩码

```
$ sqlite3 .../metadata.sqlite "select payload from credentials;"
{"id":"9a995bdf-b649-4ba5-af84-e6f92e8b0d8c","providerId":"57745d8a-08f4-4e72-8b64-1dc95f4699be","label":"默认",
 "secretRef":"gptswitch/57745d8a-.../9a995bdf-.../v1/6ad56e76-9087-4219-80c8-acd0676538dc",
 "secretVersion":1,"maskedSuffix":"••••••••ITlw","status":"saved"}

$ security find-generic-password -s app.gptswitch.desktop -a "gptswitch/57745d8a-.../6ad56e76-..." | grep -E '"acct"|"svce"|"cdat"'
    "acct"<blob>="gptswitch/57745d8a-08f4-4e72-8b64-1dc95f4699be/9a995bdf-b649-4ba5-af84-e6f92e8b0d8c/v1/6ad56e76-9087-4219-80c8-acd0676538dc"
    "svce"<blob>="app.gptswitch.desktop"
    "cdat"<timedate>="20260920084716Z"

$ （本机凭据库全量转储，只筛出本应用的条目名与帐号字段；仅保留输出）
    "acct"<blob>="gptswitch/27dba223-.../51cbd6db-..."
    "acct"<blob>="gptswitch/3ea1cae4-.../133a0515-..."
    "acct"<blob>="gptswitch/57745d8a-.../6ad56e76-..."     # ← 与上面 SQLite 的 secretRef 完全一致
    "svce"<blob>="app.gptswitch.desktop"
```

**解读（实测通过）**：`credentials` 表里**没有密钥明文字段**，只有 `secretRef` + `maskedSuffix`（尾 4 位 + `••••••••` 前缀）+ `secretVersion`。按 `secretRef` 反查 Keychain 命中一条 `svce = app.gptswitch.desktop` 的真实条目，条目名与库里的引用**逐字相同**。README「Upstream keys only in the system credential store; SQLite keeps references and a mask」这条**证真**。（3 条历史条目对应 3 个 provider 版本，用户换过供应商。）

> 本报告全程**未打印任何密钥/token 明文**；`security` 输出只取 `acct`/`svce`/时间属性，token 一律脱敏为前 4 位 + `***`（如 `bac9***`）。

### 4.3 helper 只输出本地网关 token

```
$ cat .../bin/gptswitch-auth-helper
#!/bin/sh
# Switchelp 本机网关凭据 helper。
# 由宿主以 `--instance <id>` 调用；只输出本机访问令牌，不接触上游 Key。
set -eu
... 若 instance 非空且 != "local-main" 则 exit 1 ...
dir="$(cd "$(dirname "$0")/.." && pwd)"
cat "$dir/gateway-token"

$ grep -ciE "sk-|api_key|Bearer " .../bin/gptswitch-auth-helper
0

$ T=$(.../bin/gptswitch-auth-helper --instance local-main); echo "len=${#T} prefix=${T:0:4}***"
len=64 prefix=bac9***
$ python3 -c "import re,sys;s=open('.../gateway-token').read().strip();print(len(s), bool(re.fullmatch(r'[0-9a-f]{64}',s)))"
64 True
```

**解读（实测通过）**：helper 是一个 511 字节的 POSIX shell 脚本，唯一动作就是 `cat` 同目录的 `gateway-token`（64 位十六进制 / 256-bit 熵，`GatewayToken::generate` 为两个 UUIDv4 拼接后 SHA-256）。脚本内**不含任何上游密钥**。README「The helper reads the token file (0600) from the app data directory (0700) and prints only the token on stdout」**证真**。

补充缺陷（实测失败，轻微）：

```
$ .../bin/gptswitch-auth-helper --instance bogus
gptswitch-auth-helper: instance mismatch
exit=1

$ .../bin/gptswitch-auth-helper            # 不带 --instance
bac9***（完整 64 位 token）exit=0
```

`--instance` 缺失时**不做拒绝**（脚本里 `[ -n "$instance" ] && ...` 短路），因此任何人都能无参调用 helper 拿到 token。由于 token 文件本身是 `0600`（同一用户可读），这一步没有实质提权，但「必须带 `--instance local-main`」的意图没有被强制。

### 4.4 令牌每次启动轮换

```
$ for i in 1 2; do GPTSWITCH_TEST_DATA_DIR=/tmp/gptswitch-rotate <debug app> & ... cat $W/gateway-token; kill; done
run1 pid=37032 token_len=64 prefix=6779bb0f***
run2 pid=37070 token_len=64 prefix=7bf73d6f***
```

**解读（实测通过）**：两次启动产出两个不同的 64 位十六进制 token（`start_gateway` 每次 `GatewayToken::generate()` + `install_auth_helper` 覆写）。README「a fresh token on every launch」**证真**。副作用：app 重启会让**已经在跑的宿主**手里的旧 token 失效，最长 5 分钟（`refresh_interval_ms = 300000`）内宿主的请求会 401——这属于设计取舍，README 未提及。

### 4.5 提交前自动备份原配置

```
$ ls -la .../backups/
1789894130000-08b86a98.{json,toml}   1789900742000-55a3f705.{json,toml}

$ head -c 250 .../backups/1789900742000-55a3f705.json
{ "id": "1789900742000-55a3f705", "sourcePath": "/Users/<user>/.codex/config.toml",
  "createdAt": "2026-09-20T10:39:02.220904Z", "contentHash": "55a3f705...", "bytes": 4103,
  "mayContainSecrets": true }

$ diff .../backups/1789900742000-55a3f705.toml ~/.codex/config.toml
5c5,7
< model = "gpt-5.6-sol"                    # ← 应用前用户的原始选择
---
> model = "gs/57745d8a-.../14f3fca6-..."
> model_provider = "gptswitch"
> model_catalog_json = ".../catalogs/rev_c9a0dbf7ff24a147/models.json"
133a136,148
> [model_providers] / [model_providers.gptswitch] / [model_providers.gptswitch.auth] ...
```

**解读（实测通过）**：备份是**写入前**的完整原文件（含被改前的 `model = "gpt-5.6-sol"`），带 `contentHash` 与 `bytes` 便于校验与回滚；`mayContainSecrets: true` 是对用户配置可能含密钥的诚实标注。这条是「不会毁掉用户配置」的最直接保障，**证真**。

---

## 5. 既有探针实跑

前置：三个探针默认用 `/Applications/ChatGPT.app/Contents/Resources/codex`；`probe-full-loop` 默认需要 `target/debug/bundle/macos/Switchelp.app`（本机原先没有，本次按 README 命令补构建）。

```
$ pnpm exec tauri build --debug --bundles app
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 29.03s
    Bundling Switchelp.app (.../target/debug/bundle/macos/Switchelp.app)
    Finished 1 bundle at: .../target/debug/bundle/macos/Switchelp.app
```

### 5.1 `probe-catalog.mjs` — 真实 app-server 解析自定义目录

```
$ node scripts/g0/probe-catalog.mjs
"binaryVersion": "codex-cli 0.155.0-alpha.9.2",
"models": [
  {"model":"gptswitch/probe-a","displayName":"测试供应商 A · Code Model",
   "supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"high"}],
   "defaultReasoningEffort":"low","inputModalities":["text"],"isDefault":true},
  {"model":"gptswitch/probe-b","displayName":"测试供应商 B · Vision Model",
   "supportedReasoningEfforts":[{"reasoningEffort":"medium"}],
   "inputModalities":["text","image"],"isDefault":false}
],
"result": "app-server custom catalog and pagination passed; Desktop UI and routing still unverified"
```

**解读（实测通过）**：分页（`limit:1` 逐页拉）与条目数、模态、思考档位、`displayName` 全部与目录一致。隔离 `CODEX_HOME`，未触碰真实配置。

### 5.2 `probe-apply-pipeline.mjs` — 真实管线产物在真实 Codex 中列出并被路由

```
$ node scripts/g0/probe-apply-pipeline.mjs
"status": "passed: 真实管线产物在真实 Codex 中列出并被路由（app-server 层）；Desktop UI 仍未验证",
"modelList": [
  {"id":"gs/04679e84-.../39b381a0-...","displayName":"G0 文本模型","inputModalities":["text"],"reasoningEfforts":["low","high"]},
  {"id":"gs/04679e84-.../7758473d-...","displayName":"G0 视觉模型","inputModalities":["image","text"],"reasoningEfforts":["medium"]}
],
"requests": [
  {"path":"/i/inst_g0/c/rev_68543c792413478b/v1/responses","model":"gs/04679e84-.../39b381a0-...","effort":"low","inputItems":3},
  {"path":"/i/inst_g0/c/rev_68543c792413478b/v1/responses","model":"gs/04679e84-.../7758473d-...","effort":"medium","inputItems":3}
],
"authHelperInvocations": ["1789902201 --instance local-main", "1789902201 --instance local-main"]
```

**解读（实测通过；额外验证一条 README 未展开的声明）**：`authHelperInvocations` 非空——**真实 Codex 进程确实执行了 helper**（Command 型认证生效），这是「上游 Key 不进 config」方案可行的关键一环。请求路径带 `/i/inst_g0/c/rev_.../v1` 实例+目录前缀，说明路由快照被真的用起来了。

### 5.3 `probe-full-loop.mjs` — 真实应用 + 真实网关 + 真实 Codex + chat 适配

```
$ node scripts/g0/probe-full-loop.mjs
{
  "appBinary": ".../target/debug/bundle/macos/Switchelp.app/Contents/MacOS/gptswitch",
  "helperTokenLength": 64,
  "gatewayModels": ["gs/83a1fac2-.../2e28a0f8-...", "gs/83a1fac2-.../97e92246-..."],
  "modelList":     ["gs/83a1fac2-.../2e28a0f8-...", "gs/83a1fac2-.../97e92246-..."],
  "upstreamRequests": [
    {"path":"/v1/chat/completions","model":"synthetic/text-model",  "maxTokens":4096,"effort":"low","messages":4,"bearerSeen":true},
    {"path":"/v1/chat/completions","model":"synthetic/vision-model","maxTokens":4096,"effort":"low","messages":4,"bearerSeen":true}
  ],
  "result": "passed: 真实应用 + 真实网关 + 真实 Codex + chat 适配全链路打通（上游为本地 mock）"
}
--- 应用日志 ---
Switchelp 启动恢复：operation=51a8963e-... instance=inst_e2e applied=false
```

**解读（实测通过）**：链路最完整的一条证据——**应用本身**（不是探针自建网关）被拉起，它生成令牌、装 helper、并在启动时重新发布种子事务的路由；真实 Codex 随后列出该目录的全部别名、逐个模型完成真实一轮；上游收到的是**真实上游 ID**（`synthetic/text-model`，不是别名），`max_tokens` 被收口到声明的 4096、`reasoning_effort` 映射为声明的 `low`、chat 适配产出 4 条消息（system + 历史 + 用户）。README「chat adapter / output-limit enforcement / reasoning-level mapping backed by the request parameters a real upstream received」**证真**。

---

## 6. 原生选择器与 Desktop GUI

### 6.1 `.codex` 真实配置下 `model/list` 的内容

```
$ /Applications/ChatGPT.app/Contents/Resources/codex --version
codex-cli 0.155.0-alpha.9.2

$ node /tmp/codex-realmodel-probe.mjs    # 用真实 HOME/CODEX_HOME 调 app-server model/list，分页拉全
[
  { "id":"gs/57745d8a-.../14f3fca6-...", "name":"deepseek-v4.1",
    "modalities":["image","text"], "efforts":[], "def":"none" }
]
count = 1

$ shasum -a 256 ~/.codex/config.toml   # 前后一致
0dc8fb41682a8cc0f54a8d37298c4c9e6450200d8bd6aa5ea8bd044a78236f46
```

**解读（实测通过，但暴露一个重要副作用）**：自定义模型**确实出现在原生 `model/list` 里**（`displayName` 为 `deepseek-v4.1`，模态含 image）。**但整个列表只有这 1 个模型**——列表被自定义目录**完全替换**，用户原来的 7 个原生模型（含备份里记录的默认 `gpt-5.6-sol`、缓存里的 `gpt-6-astra`）**全部从宿主模型菜单消失**。README 只说「the model menu comes from a compiled catalog file」，没有说明这是**替代而非追加**；用户在应用本工具后无法再在同一菜单里选原生模型，需要 restore 才能恢复。这是本次审计发现的对用户体验影响最大的一条**未被文档声明**的行为。

（另外 `efforts: []` / `def: "none"` 与目录里 `supported_reasoning_levels: []` 一致，但与 `config.toml` 里遗留的 `model_reasoning_effort = "high"` 相互矛盾；实测未导致推理失败。）

### 6.2 Desktop GUI 模型选择器：README 说「未验证」，证据指向「已实际使用」

```
$ sqlite3 ~/.codex/logs_2.sqlite "select feedback_log_body from logs where feedback_log_body like '%thread/start%' and feedback_log_body like '%gs/57745d8a%' ... limit 1;"
app_server.request{... rpc.method="thread/start" ... app_server.client_name="Codex Desktop"
  app_server.client_version="26.915.31945"}:app_server.thread_start.create_thread:thread_spawn:session_init:
  Configuring session: model=gs/57745d8a-08f4-4e72-8b64-1dc95f4699be/14f3fca6-1920-4f29-b4d0-5e32ccde5397;
  provider=ConfiguredModelProvider { info: ModelProviderInfo { name: "Switchelp",
    base_url: Some("http://127.0.0.1:18765/i/inst_feea2e927725590c/c/rev_c63a6e34074cc5e5/v1"),
    auth: Some(ModelProviderAuthInfo { command: ".../bin/gptswitch-auth-helper",
      args: [<redacted>, <redacted>], timeout_ms: 5000, refresh_interval_ms: 300000 }),
    wire_api: Responses, ... } }
# 同一模式在 2026-09-20 17:09:20（thread/start）与 18:03:18（thread/resume）各出现一次

$ sqlite3 ~/.codex/logs_2.sqlite "select ... where feedback_log_body like '%model/list%' order by ts desc limit 5;"
2026-09-20 18:26:10 | INFO | codex_models_manager::cache  | ... model/list ...
2026-09-20 18:39:10 | TRACE| codex_app_server::message_processor | app-server request: model/list connection_id=ConnectionId(0)
```

**解读（无法验证「UI 一定显示」；但可证伪 README 的「证据只到 app-server 层」）**：

- README（中英）都写「**Not verified: the Desktop GUI model picker**（现有证据是 app-server 层）」。实测证据比这更强：**Codex Desktop 客户端本身**（`client_name="Codex Desktop"`，`client_version="26.915.31945"`，即 GUI 那条连接 `connection_id=0`）在 17:09 与 18:03 两次以受管模型 + `Switchelp` provider **成功创建/恢复会话**，且 GUI 连接在 18:26、18:39 都调用了 `model/list`（拿到的就是那个只有 1 项的目录）。这已经从「app-server 层」推进到了「Desktop 客户端实际用它发起会话」。
- **仍然无法验证**的是「GUI 下拉框里肉眼能看到这个模型名」——需要截屏/读屏比对，本次为纯命令行审计，未开 GUI。因此该条应改写为「GUI 已完成实际使用的证据链，但『选择器列表渲染』未做视觉核验」。

---

## 7. 「应用后自动重启宿主」

### 7.1 实现位置

```
$ grep -rn "restart" src/app/PendingApplyBar.tsx
80:      const report = await client.restartHost(await instanceIdOf(client)).catch(() => null);
82:        showToast(t('codex.restartHostStillRunning'), 'danger');
85:      showToast(t('codex.appliedAndRestarted'));
101:          {busy && !plan ? t('codex.committing') : t('codex.applyAndRestart')}    # 按钮文案「应用并重启 Codex」
107:      commitLabel={t('codex.applyAndRestart')}

$ grep -rn "host_restart" src-tauri/src/main.rs src-tauri/src/commands.rs
src-tauri/src/main.rs:264:            commands::host_restart,
src-tauri/src/commands.rs:340:pub async fn host_restart(...)
src-tauri/src/commands.rs:351:fn restart_host(...)   # → switch_core::platform::restart_host(...)
crates/switch-core/src/platform/mod.rs:216:pub fn restart_host(probe, plan, process_name, timing) -> RestartOutcome
```

**解读（实测通过，但需修正措辞）**：实现是「**apply + restart 合成一个动作**」——`PendingApplyBar` 里的「应用并重启 Codex」按钮先 `apply_execute` 再 `restartHost`，**不是**用户点普通「应用」后静默自动重启。核心逻辑在 `platform::restart_host`：先 `osascript quit app`（macOS）轮询等进程真退出（6s）→ 未退出才升级到 `pkill`（5s）→ 旧进程仍在则**拒绝启动**并如实返回 → 再 `open` 并在 25s 内轮询确认新进程出现。三步都以「观察到进程状态」为准，不看命令退出码（`osascript`/`open` 对不存在的应用也返回 0）。这套「确认式」设计与 README 的诚实性基调一致。

### 7.2 真机痕迹（多重交叉印证）

```
$ sqlite3 .../metadata.sqlite   # 最后一条 apply 的阶段迁移
3 committing      2026-09-20T10:39:02.220Z   # = 本地 18:39:02
4 awaiting_reload 2026-09-20T10:39:02.234Z

$ head -c 130 .../backups/1789900742000-55a3f705.json
{ "id": "1789900742000-55a3f705", "createdAt": "2026-09-20T10:39:02.220904Z" ... }   # 本地 18:39:02

$ /usr/bin/log show --start "2026-09-20 18:38:50" --predicate 'process == "ChatGPT" AND eventMessage CONTAINS "CHECKEDIN"'
2026-09-20 18:39:03.033 Df ChatGPT[6434] ... CHECKEDIN: pid=6434 asn=0x0-0x3958955 foreground=1

$ /usr/bin/log show --start "2026-09-20 18:38:20" --predicate 'process == "osascript"'
2026-09-20 18:38:30.8  osascript[6011] ... TCCAccessRequest() IPC / appleevents
2026-09-20 18:38:39.146 osascript[6011] ... Entering exit handler.

$ sqlite3 ~/.codex/logs_2.sqlite "..."   # 宿主侧最后事件
2026-09-20 18:39:11 | INFO | codex_app_server_transport::transport::stdio | SIGTERM received; closing stdio connection (45s shutdown deadline)
# 且这是该库中时间戳最大的一行——18:39:11 之后再无任何宿主日志
```

**解读（实测通过）**：四个独立来源在同一条时间线上对齐——

| 时刻（本地） | 事件 | 来源 |
| --- | --- | --- |
| 18:38:30 | `osascript` 启动，走 AppleEvent（TCC） | unified log |
| 18:39:02.220 | 配置写入 + 备份落盘（`committing`） | SQLite + backups |
| 18:39:02.234 | 事务进入 `awaiting_reload` | SQLite |
| **18:39:03.033** | **新 `ChatGPT` 进程 pid 6434 CHECKEDIN** | unified log |
| 18:39:11 | 旧宿主 app-server 收到 SIGTERM 收尾退出 | logs_2.sqlite |

配置提交完成（18:39:02.23）到新宿主进程 check-in（18:39:03.03）相隔 **0.8 秒**，与 `restart_host` 的「退出确认 → `open` → 轮询确认启动」时序吻合；旧实例的 app-server 到 18:39:11 才彻底退出（GUI 应用拆解较慢），也解释了为什么日志里 SIGTERM 出现在新进程之后。**「应用后会自动重启宿主」这条 memory 在真机上有直接痕迹支撑。**

### 7.3 启动恢复确实重发布路由（`applied=false` 不代表没生效）

```
$ ./target/release/.../gptswitch > /tmp/switchelp-audit-app.log &
Switchelp 启动恢复：operation=fcd00cfa-... instance=inst_feea2e927725590c applied=false
Switchelp 启动恢复：operation=0114b8bf-... instance=inst_feea2e927725590c applied=false
Switchelp 启动恢复：operation=dcb0cf45-... instance=inst_feea2e927725590c applied=false
Switchelp 启动恢复：operation=5c99185d-... instance=inst_feea2e927725590c applied=false
Switchelp 启动恢复：operation=684fedce-... instance=inst_feea2e927725590c applied=false

$ curl -s -H "Authorization: Bearer $T" .../i/inst_feea2e927725590c/c/rev_c9a0dbf7ff24a147/v1/models
{"data":[{"id":"gs/57745d8a-.../14f3fca6-...", ...}],"object":"list"}     # 路由可用
```

> 5 条日志全 `applied=false` 乍看像「恢复什么都没做」。对照 `crates/switch-core/src/application/apply.rs:666-692`：`applied` 只表示「是否改写了配置文件」，**路由重发布在紧随其后的第二个循环里独立进行**（对 `AwaitingReload|Pending|Verified` 的 apply 事务，目录文件哈希完整就 `router.publish`，否则报 `ConflictWithExternalChange`）。上面 `/models` 200 且能命中别名，就是路由已发布的直接证据。**解读（实测通过）**：这是「重启 app 后宿主仍能继续用」的关键保障，`applied=false` 是正常语义而非缺陷。

---

## 8. 结论汇总

| # | 验证项 | 结论 |
| --- | --- | --- |
| 1.1 | `config.toml` 受管字段与 README 链路一致 | 实测通过 |
| 1.2 | 数据目录权限、catalogs 不可变版本、无落盘诊断 | 实测通过（诊断不落盘=无法从磁盘复核） |
| 1.3 | 事务流水阶段分布 | 实测通过（Prepared 3 / AwaitingReload 2 / Verified 1；无 Conflict/RollingBack/Failed；**不存在 `committed`/`rolled_back` 这两个 stage 名**） |
| 2.1–2.2 | 目录字段与库内策略（别名/上游 ID 分离、能力重算） | 实测通过 |
| 2.3 | 别名准入：原生 slug 被拒 | 实测通过 |
| 2.4 | `models_cache.json` 未被本工具改写 | 实测通过 |
| 3.1 | 网关只绑 loopback、启动即监听 | 实测通过；**但审计开始时网关与 app 都没在跑**（链路实际处于断开状态） |
| 3.2 | 无 token / 错 token 被拒 | 实测通过 |
| 3.3 | 带 Origin / 预检被拒 | 实测通过 |
| 3.4 | `/health`、`/models` 可用且与目录一致 | 实测通过 |
| 3.5 | **真实上游端到端推理** | **实测通过 —— 证伪 README「尚未对真实第三方供应商验证」** |
| 3.6 | `GET /models` 未知目录版本 → 空回复（`http_code=000`） | **实测失败（缺陷 A，code 级定位到 `server.rs:158` 的 `let _ = handle(...)`）** |
| 3.7 | `/models` 不校验前缀实例 + `instanceMismatch` 文案把实例名写反 | **实测失败（缺陷 B，两处）** |
| 4.1–4.2 | 上游 Key 不在 config.toml、只在 Keychain、库里只有引用+掩码 | 实测通过 |
| 4.3 | helper 只输出 token | 实测通过；**但缺 `--instance` 时不报错（缺陷 C，轻微）** |
| 4.4 | 每次启动轮换 token | 实测通过（副作用：宿主可能 5 分钟内 401，未文档化） |
| 4.5 | 提交前自动备份原配置 | 实测通过 |
| 5.1 | `probe-catalog.mjs` | 实测通过 |
| 5.2 | `probe-apply-pipeline.mjs`（含宿主真实调用 helper） | 实测通过 |
| 5.3 | `probe-full-loop.mjs`（真实 app+网关+Codex+chat 适配） | 实测通过 |
| 6.1 | 自定义模型出现在原生 `model/list` | 实测通过；**但目录完全替换原生模型列表（原生 7 个模型全部消失），此副作用未在 README 声明** |
| 6.2 | Desktop GUI 模型选择器 | 部分验证：Desktop 客户端已用受管模型真实建会话（强于 README 自称的「仅 app-server 层」）；「选择器列表渲染」无法验证（纯 CLI 审计未开 GUI） |
| 7.1–7.2 | 应用后重启宿主 | 实测通过（实现是「应用并重启」合成动作；真机时间线 18:39:02.23 提交 → 18:39:03.03 新宿主 check-in） |
| 7.3 | 启动恢复重发布路由 | 实测通过（`applied=false` 是正常语义） |

### 被证伪 / 需修正的声明

1. **证伪**：README「it has **not been verified against a real third-party provider yet**」（中文版同）。本次用用户真实配置的真实上游跑通完整推理并返回内容。
2. **需修正（措辞过旧）**：README「Not verified: the Desktop GUI model picker（现有证据是 app-server 层）」。真机日志显示 `client_name="Codex Desktop"` 的 GUI 连接已用受管模型成功创建/恢复会话并调用 `model/list`——证据层级高于 app-server 层；剩下未验证的只是「列表渲染的视觉核验」。
3. **声明与实现不符**：网关代码注释称「`admission` 仍会校验该目录修订确实属于前缀里声明的实例」，但该校验只覆盖推理路径，`GET /models` 完全没有实例校验。
4. **未声明的重要行为**：应用本工具后 `model_catalog_json` 会**替换**宿主原生模型列表（实测只剩 1 项），原生模型（含默认 `gpt-5.6-sol`）从菜单消失。README 未提示这一点，也未提示 restore 是恢复途径。
5. **未声明的副作用**：app 每次启动轮换 token，会让在跑的宿主最长 5 分钟内 401。

### 最严重的问题（按影响排序）

1. **`GET /models` 错误路径静默断连（缺陷 A）**：唯一的「HTTP 层无响应」缺陷，根因是 `server.rs` 接受循环 `let _ = gateway.handle(stream)` 吞掉错误 + `/models` 分支未兜底写响应。同一处写法会让**任何**未来从 `handle` 里冒出的 `Err` 变成空回复，是结构性的错误处理漏洞（其余端点目前都自己 `write_error` 才没暴露）。
2. **原生模型列表被整表替换且未声明**：用户应用后失去所有原生模型入口，属高影响、低可见度的行为变更。
3. **实例绑定在 `/models` 缺失 + 错误文案实例名颠倒**：安全影响有限（仍需有效 token），但直接误导排障方向。

### 端到端链路当前是否可用

**可用，但需要 app 处于运行状态。** 实测证据：用户真实配置 + 真实上游完成一轮推理（3.13s 返回「可用」）；三个 G0 探针全部通过；宿主原生 `model/list` 返回受管模型；应用+重启链路在 18:39 有完整真机痕迹。**审计开始时的静默风险**是：`config.toml` 已指向 `127.0.0.1:18765`，而 Switchelp 与宿主**都没有在运行**——此时若用户打开 Codex，第三步就会打到一个没监听的端口。也就是说链路本身的正确性已被证明，但「app 必须常驻」这一前提既不在配置里体现，也没有能在 app 缺席时自愈的机制。
