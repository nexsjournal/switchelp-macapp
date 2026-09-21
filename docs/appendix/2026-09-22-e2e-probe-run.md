# 2026-09-22 真机端到端探针运行记录（人工层，第三层）

依据 `docs/development/02-testing-and-release.md` 末节：`scripts/g0/` 的探针都需要一个真实
Codex 可执行文件，GitHub runner 上没有，所以只能在发布前人工跑一遍，并把输出留痕。

本文就是那一层的第一次真实留痕。**结论：这一层不再「没有任何痕迹」，但也绝不是绿的。**

- 日期：2026-09-22
- 版本：0.3.0
- commit：`e345bbd`
- 机器：macOS 26.6.2 arm64
- 真实 Codex：`/Applications/ChatGPT.app/Contents/Resources/codex`，`codex-cli 0.155.0-alpha.9.2`

## 环境与隔离的前提（先说清楚，因为后面两条探针没跑通）

- 全部探针的 `CODEX_HOME` 都在 `/tmp` 下：`probe-catalog.mjs` 用 `mkdtemp`；
  `probe-apply-pipeline.mjs` 用 `GPTSWITCH_G0_DIR`（本次 `/tmp/gptswitch-g0-apply-2026-09-22/run`）；
  `probe-full-loop.mjs` 用 `GPTSWITCH_E2E_DIR`（本次 `/tmp/gptswitch-e2e`），并把
  `GPTSWITCH_TEST_DATA_DIR` 指到同一目录（debug 构建读这个变量，见 `src-tauri/src/main.rs:203`）。
- mock 上游只接收合成请求，没有任何真实 Key 或真实域名。
- 用户真实 `~/.codex` 在跑前跑后各做了一次全量 hash（11445 个文件），并存了 75 秒空闲对照。
- **未使用的产物**：`target/debug/bundle/macos/Switchelp.app` 是一次**未完成的构建残骸**
  （缺 `Contents/_CodeSignature`，还留着 `gptswitch-bridge-app.cstemp`）。本会话对 debug 包做
  `codesign` 会卡在 `SecKeyCreateSignature`（钥匙串授权阻塞），所以按指示改用裸二进制
  `target/debug/gptswitch`（57 MB，前端资源已内嵌）。**这带来一条未验证：签名 bundle 与裸二进制是否等价，未验证。**

### 两条探针没跑通的原因：固定的网关端口 18765 被用户自己正在运行的应用占着

```
$ lsof -nP -iTCP:18765 -sTCP:LISTEN
COMMAND     PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME
gptswitch 14823  lex   11u  IPv4 ...    TCP 127.0.0.1:18765 (LISTEN)

$ ps -o pid,etime,command -p 14823
14823  01:46:42  /Applications/Switchelp.app/Contents/MacOS/gptswitch
```

`gateway::DEFAULT_PORT`（`crates/switch-core/src/gateway/mod.rs:24`）是硬编码常量 `18765`，
探针的断言也写死 `assert.equal(base.port, '18765')`，**没有环境变量可以改端口**。
这个 PID 是用户**自己安装的、正在使用的**应用，按要求**没有杀它**。因此
`probe-apply-pipeline.mjs` 与 `probe-full-loop.mjs` 无法完整跑完。

---

## 探针 1：`probe-catalog.mjs` —— 通过

命令与退出码：

```
$ node scripts/g0/probe-catalog.mjs
EXIT=0
```

探针自己的断言（`assert` from `node:assert/strict`，全过才不抛）：

| 断言 | 结果 |
| --- | --- |
| `models.length === 2` | 成立（`model/list` 带 `limit:1` 翻页走完两页） |
| `models.map(m => m.model).sort()` == `['gptswitch/probe-a','gptswitch/probe-b']` | 成立 |
| `models[0].inputModalities` == `['text']` | 成立 |
| `models[1].inputModalities` == `['text','image']` | 成立 |

关键输出：

```
binaryVersion: codex-cli 0.155.0-alpha.9.2
initialized.codexHome: /private/var/folders/.../T/gptswitch-g0-uIzK4F/codex-home
models[0]: id=gptswitch/probe-a displayName=测试供应商 A · Code Model inputModalities=["text"]
           supportedReasoningEfforts=["low","high"] defaultReasoningEffort="low"
models[1]: id=gptswitch/probe-b displayName=测试供应商 B · Vision Model inputModalities=["text","image"]
           supportedReasoningEfforts=["medium"] defaultReasoningEffort="medium"
result: app-server custom catalog and pagination passed; Desktop UI and routing still unverified
```

**判定：通过。** 真实 Codex 的 app-server 能解析我们手写的 `model_catalog_json`，能分页列出，
能力（text / image）与思考档位都按声明读到。产物：`.local/g0-catalog.json`（未提交）。
**仍未验证**（探针自己说的）：Desktop UI 与实际路由。

---

## 探针 2：`probe-apply-pipeline.mjs` —— 未跑通（卡在第 2 步 / 共 5 步）

命令与退出码：

```
$ GPTSWITCH_G0_DIR=/tmp/gptswitch-g0-apply-2026-09-22/run node scripts/g0/probe-apply-pipeline.mjs
EXIT=1
```

**第 1 步成功**（真实「计划 → CAS → 原子写入」管线跑通了，产物落在隔离目录）：

```
/tmp/gptswitch-g0-apply-2026-09-22/run/
├── app-data/catalogs/rev_628a4468049d2b28/models.json
├── bin/gptswitch-auth-helper
├── codex-home/config.toml
└── metadata.sqlite

config.toml:
  model = "gs/3956057d-6f5f-48ba-a29c-a3837198c18b/1fc0e93e-ea10-4067-96ef-b33df20e748f"
  base_url = "http://127.0.0.1:18765/i/inst_g0/c/rev_628a4468049d2b28/v1"
```

**第 2 步失败**，原始报错（未做任何美化）：

```
node:net:1940
    const ex = new UVExceptionWithHostPort(err, 'listen', address, port);
               ^

Error: listen EADDRINUSE: address already in use 127.0.0.1:18765
    at Server.setupListenHandle [as _listen2] (node:net:1940:16)
    at listenInCluster (node:net:1997:16)
    at node:net:2206:12
    at process.processTicksAndRejections (node:internal/process/task_queues:90:21) {
  code: 'EADDRINUSE',
  errno: -48,
  syscall: 'listen',
  address: '127.0.0.1',
  port: 18765
}
```

未得到评估的断言（因为第 2 步就断了）：

- `model/list` 恰好等于管线编译出的 alias；
- 每个模型的 `input_modalities` / reasoning 档位 / `display_name` 与目录一致；
- 每个 alias 各产生一次上游请求，且请求带本实例的目录前缀；
- 宿主**真的调用过** auth helper（决定 `Command` 型认证可用性）。

**判定：未跑通。卡在「按 config.toml 里的真实 base_url 起 mock 上游」这一步——18765 被用户运行中的应用占用。**
管线本身（第 1 步）是成功的，但「真实产物能否被真实 Codex 列出并路由」这条链**没有被验证**。

---

## 探针 3：`probe-full-loop.mjs` —— 未跑通（卡在第 4 步 / 共 6 步）

命令与退出码（按你的指示改用裸二进制）：

```
$ GPTSWITCH_APP_BINARY=$PWD/target/debug/gptswitch \
  GPTSWITCH_E2E_DIR=/tmp/gptswitch-e2e \
  node scripts/g0/probe-full-loop.mjs
EXIT=1
```

原始报错（未美化）：

```
--- 应用日志 ---
Switchelp 启动恢复：operation=ff928c05-9b17-4eb0-9937-a97a3e0bc711 instance=inst_e2e applied=false
Switchelp 网关未启动：error.portInUse ["无法绑定 127.0.0.1:18765：Address already in use (os error 48)。端口被占用最常见的原因是已经开着另一个 Switchelp 实例；也可能被别的程序占用——本工具不会自动换端口，因为 Codex 配置里写的就是这个地址。"]

file:///Users/example/Code/ProjDev/gptswitch-macapp/scripts/g0/probe-full-loop.mjs:200
  throw new Error(`应用未在 30 秒内就绪；日志：${appLog}`);
        ^

Error: 应用未在 30 秒内就绪；日志：...
    at waitForGateway (file:///Users/example/Code/ProjDev/gptswitch-macapp/scripts/g0/probe-full-loop.mjs:200:9)
    at async file:///Users/example/Code/ProjDev/gptswitch-macapp/scripts/g0/probe-full-loop.mjs:111:17
```

这里有两条**有价值的事实**，都是真的：

1. **裸二进制本身没问题。** `target/debug/gptswitch` 起来了，跑了启动恢复
   （`operation=ff928c05... instance=inst_e2e`），然后因为端口占用而**按设计拒绝启动网关**，
   报的是带类型的 `error.portInUse`，并且**没有偷偷换端口**（这正是 `docs` 里写的预期行为）。
   所以这次失败纯粹是端口，不是应用启动不了。
2. **签名 bundle 与裸二进制是否等价，未验证。** 本次全程没有用过 bundle（它是未完成构建的残骸）。

未得到评估的断言：网关按已发布目录版本提供全部 alias；宿主菜单列出全部自定义模型；每个 alias
真实跑完一轮；上游实际收到的是**真实上游 ID 而非 alias**、`max_tokens==4096`、
`reasoning_effort=='low'`、`messages>=2`；helper 令牌长度 64。

**判定：未跑通。卡在 `waitForGateway`——应用网关无法绑定 18765（`error.portInUse`）。
裸二进制可用；bundle 等价性未验证。**

---

## 探针 4：`coexist-check.mjs` —— 失败（6 条断言里 3 条不过，且可复现）

命令与退出码：

```
$ cargo run -q -p switch-core --example g0_apply_pipeline -- /tmp/g0-coexist   # EXIT=0
$ node scripts/g0/coexist-check.mjs /tmp/g0-coexist
EXIT=1
```

探针自己的输出，**逐行照抄**（`✓`/`✗` 就是它的 `note(ok, ...)`）：

```
✓ initialize 通过（两根都握了手） — codexHome=/Users/example/.codex
✗ 菜单里有原生模型 0 条
✓ 菜单里有我们的模型 4 条 — gs/57745d8a-08f4-4e72-8b64-1dc95f4699be/14f3fca6-1920-4f29-b4d0-5e32ccde5397
✓ 合并后没有重复条目 — 4 条
✗ 我们的 slug 起线程落在托管那根 — gs/57745d8a-.../14f3fca6-... → native（model-is-native）
✓ 托管线程拿到了 threadId — 01a0c4d4-5bc6-7322-8930-671f6e5abedf
✗ 只带 threadId 的续接跟着线程走（不是靠模型猜） — → native（thread-pinned）
✓ 该错误来自 codex 本身（不经 bridge 同样如此） — {"code":-32600,"message":"no rollout found for thread id 01a0c4d4-..."}

3 项未通过：菜单里有原生模型 0 条 / 我们的 slug 起线程落在托管那根 / 只带 threadId 的续接跟着线程走（不是靠模型猜）
```

bridge 自己的日志（`/tmp/g0-coexist/bridge-check.log`）：

```
{"event":"model-list-merged","managed":2,"merged":4,"native":2}
{"child":"native","event":"routing","method":"thread/start","model":"gs/57745d8a-.../14f3fca6-...","why":"model-is-native"}
{"child":"native","event":"routed","method":"thread/start","threadId":"01a0c4d4-..."}
{"child":"native","event":"routing","method":"thread/resume","model":null,"paramKeys":["threadId"],"why":"thread-pinned"}
```

### 根因：环境，不是产品。这台机器真实的 `~/.codex` 自己就是「共存模式」

```
$ head -6 ~/.codex/config.toml
model = "gs/767e7394-5a05-47d4-aecf-90d1b0ea2a46/0bd2a770-a5b1-4498-99fc-329764e533fc"
model_provider = "gptswitch"
model_catalog_json = "/Users/example/Library/Application Support/app.gptswitch.desktop/catalogs/rev_a3a51047b9bcb04a/models.json"
```

也就是说，**native 那根 Codex 的 `model/list` 也会返回带 `gs/` 前缀的 alias**（用户自己安装的
Switchelp 一直在管着它）。探针的判据是「id 以 `gs/` 开头 == 我们的」：

```js
const ours = models.filter(m => keyOf(m).startsWith('gs/'));
```

在正常环境里这没错；但在这台机器上 4 条合并条目**全部**以 `gs/` 开头，于是 native 的条目被
误判成「我们的」。硬证据是 alias 的 UUID：

| 来源 | alias |
| --- | --- |
| 探针本次托管的（run 1） | `gs/a67de8af-a043-44ba-8e66-61969f2351f8/4b9b09d2-...` |
| 探针本次托管的（run 2） | `gs/54adabcf-07ab-4499-8b4e-209b8bc4a1f3/dc6dc426-...` |
| **探针当成「我们的」的那条** | `gs/57745d8a-08f4-4e72-8b64-1dc95f4699be/14f3fca6-...` |
| 用户真实目录里的 alias | `gs/57745d8a-.../14f3fca6-...`（一模一样） |

托管 alias 每次种子都是随机 UUID、两次都不同；探针两次都挑了**同一条用户原生 alias**。
所以 bridge 判 `model-is-native` 并路由到 native 是**正确的**，探针的 3 条断言是**假阴性**。
两次独立运行（`/tmp/g0-coexist`、`/tmp/g0-coexist2`）输出逐字相同，结论可复现。

**一个必须写清楚的副作用：** 因为这条误判，探针发起的 `thread/start` 被 bridge 正确地路由到了
**native 那根**，也就是用户真实的 `~/.codex`——这与脚本注释里「原生那侧不真的起线程」的
自律相违背。实际影响经核查为零：它只是 `thread/start`，没有跑回合，因此没有落任何 rollout
（随后的 `thread/resume` 返回 `no rollout found`，且该 threadId 在 `~/.codex` 里**搜不到**）。

**判定：失败（3/6 假阴性），根因是环境（机器本身已在共存模式），未证明存在产品缺陷。**

---

## 用户真实 `~/.codex` 的非干扰核查

因为探针 4 会去读真实 home、探针 3 会往登录钥匙串写一条合成条目，这两件事都单独核过。

`~/.codex` 全量 hash 前后对比（11445 个文件）在**探针 4 第一次运行的那个窗口**里出现变动：
`logs_2.sqlite`、`thread_history_1.sqlite`、`goals_1.sqlite`、`queue_1.sqlite`、
`memories_1.sqlite`、`state_5.sqlite`（含 `-wal`/`-shm`）。为判断是不是探针写的，做了两件事：

1. **空闲对照**：什么都不跑，75 秒后重新采样，**零变动**。
2. **带紧贴采样的复跑**：重跑一遍 `coexist-check.mjs`，前后对 `~/.codex/*.sqlite*` 采样，
   **零变动**。

同时：探针起过的两个 threadId（`01a0c4d4-...`、`01a0c4d8-...`）在整个 `~/.codex` 里
**grep 不到**；`~/.codex/sessions` 下 60 分钟内**没有任何文件被改**；用户的
`config.toml`（共存模式）原样未动。变动最合理的解释是**当时并发的 ChatGPT 桌面端**
（`logs_2.sqlite` 有 313 MB、`thread_history_1.sqlite` 有 398 MB，是活跃使用中的库），
但**不能完全排除**是 `codex app-server` 以真实 home 启动时的记账写入，故如实记下。

### 钥匙串：合成条目已确认写入并被精确删除

`probe-full-loop.mjs` 会执行（脚本原文，第 84-88 行）：

```js
const keychainService = 'app.gptswitch.desktop';
execFileSync('security', ['add-generic-password', '-s', keychainService, '-a', seed.secretRef, '-w', syntheticSecret, '-A', '-U']);
```

独立核对结论：

- ① service **固定**是字面量 `app.gptswitch.desktop`；account 是种子报出的 `secretRef`，
  形如 `gptswitch/<provider_uuid>/<credential_uuid>/v1/<uuid>`，**全部是随机 UUID**
  （`ProviderId::generate()` = `uuid::Uuid::new_v4()`），不可能撞上用户真实凭据的条目。
- ② 跑完实测。本次探针 3 用的 ref 是
  `gptswitch/46bb6ff5-9903-444a-80ce-643188f310c3/117712f8-2951-44fb-a144-24fe3d14b8a4/v1/c3060821-ea34-45f5-8a82-ac784ac1329b`：

```
$ security find-generic-password -s app.gptswitch.desktop -a gptswitch/46bb6ff5-.../c3060821-...
（查询结果：该项不存在，工具返回 item-not-found）
EXIT=44
```

**查不到 = 没有残留。** 钥匙串里 `app.gptswitch.desktop` 名下还剩 4 条，全部属于用户真机
（provider `57745d8a...`、`767e7394...`），未被触碰。用户的应用进程 PID 14823 在全部探针跑完后**仍存活**。

---

## 汇总

| 探针 | 命令 | 退出码 | 判定 |
| --- | --- | --- | --- |
| `probe-catalog.mjs` | `node scripts/g0/probe-catalog.mjs` | 0 | **通过** |
| `probe-apply-pipeline.mjs` | `GPTSWITCH_G0_DIR=… node scripts/g0/probe-apply-pipeline.mjs` | 1 | **未跑通**：`EADDRINUSE 18765`（第 1 步管线成功，第 2 步起 mock 上游失败） |
| `probe-full-loop.mjs` | `GPTSWITCH_APP_BINARY=…target/debug/gptswitch GPTSWITCH_E2E_DIR=… node …` | 1 | **未跑通**：应用网关 `error.portInUse` 18765（裸二进制本身可用） |
| `coexist-check.mjs` | `cargo run … g0_apply_pipeline … && node scripts/g0/coexist-check.mjs …` | 1 | **失败**：3/6 断言假阴性，根因是机器本身已在共存模式；bridge 路由其实正确 |

**人工层现在到底有没有被真正跑过？** 跑过了，但只跑绿了一条。第三层第一次留下了
真实痕迹：≥1 条探针（`probe-catalog`）在真实 Codex 上端到端通过；两条被**固定端口 18765**
挡住（用户自己装的应用正在用），一条因为**这台机器真实 `~/.codex` 已处于共存模式**而假阴性。
**这不是绿，不能当作发布门禁已过。** 要让这一层真正变绿，需要在

1. 用户退出自己运行中的 `/Applications/Switchelp.app`（或给出端口可配置能力）；且
2. 用一个**不在共存模式**的 `GPTSWITCH_BRIDGE_NATIVE_HOME` 跑 `coexist-check.mjs`（目前脚本写死 `~/.codex`）

之后重跑。另有一条遗留未验证：**签名 bundle 与裸二进制是否等价**。
