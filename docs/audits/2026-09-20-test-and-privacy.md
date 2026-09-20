# 测试与隐私审计（2026-09-20）

审计对象：Switchelp / `gptswitch-macapp`，工作区版本 0.2.0，HEAD = `2fda2a5`。
本报告只做审计与记录，未改动业务代码，未 push，未改写 git 历史。

> 脱敏约定：所有真实用户名、域名、令牌、实例/修订 id 一律以「前 4 位 + `***`」或 `<redacted>` 呈现。
> 本文件本身不含任何真实凭据、完整域名或令牌，可安全提交（写完后应重跑 `scripts/check-publish-safety.sh` 确认）。

---

# A. 测试覆盖

## A1. 门禁结果（实测）

| 门禁 | 命令 | 结果 | 用例数 | 耗时 | 备注 |
| --- | --- | --- | --- | --- | --- |
| 类型检查 | `pnpm typecheck` | **通过**（exit 0） | — | 1.9s | 无输出 |
| 前端测试 | `pnpm test` | **通过**（exit 0） | 112（15 文件） | 3.6s | 17 条 `act(...)` 警告（见 A5） |
| 核心测试 | `cargo test -p switch-core` | **通过**（exit 0） | **410 通过 / 3 ignored** | 11.6s（debug 已构建） | 单元 325 + apply_service 20 + config_golden 24 + gateway_server 21 + sqlite_repository 7 + workspace_service 13 |
| 隐私扫描 | `scripts/check-publish-safety.sh` | **失败**（exit 1） | — | — | 命中 3 个未跟踪审计文档（见 B2-①） |

核心测试各二进制：

- `src/lib.rs` 单元测试：325 通过。
- `tests/apply_service.rs`：20 通过。
- `tests/config_golden.rs`：24 通过。
- `tests/gateway_server.rs`：21 通过。
- `tests/sqlite_repository.rs`：7 通过。
- `tests/workspace_service.rs`：13 通过。
- `tests/restart_host.rs`：**2 ignored**。
- `tests/system_vault.rs`：**1 ignored**。

### ignore 用例核实

| 文件:行 | 用例 | ignore 原因（代码内注明） | 替代覆盖 |
| --- | --- | --- | --- |
| `crates/switch-core/tests/system_vault.rs:4` | `native_vault_round_trip_and_cleanup` | 需在目标系统上显式验收凭据库，不在普通单元测试中访问 OS 凭据库 | **无自动化替代**。`MemoryVault` 覆盖了 workspace 侧逻辑（`workspace_service.rs` 13 条、`sqlite_repository.rs` 的「不含秘密材料」），但「真 Keychain 写入/读取/删除」只有这条 ignored 用例。 |
| `crates/switch-core/tests/restart_host.rs:27` | `restarting_the_real_host_reports_the_real_outcome` | 会真的退出并重开用户正在用的 Codex | 重启**计划**有单元覆盖（`platform/mod.rs` 的 `restart_plan_ends_the_process_by_image_name_on_windows` 等）；真实退出/重开**无自动化替代**，只能人工跑。 |
| `crates/switch-core/tests/restart_host.rs:72` | `the_signal_fallback_restarts_the_real_host_when_graceful_quit_is_denied` | 同上 | 同上。 |

> 结论：真正属于「需真机验收」的只有 OS 凭据库与宿主重启两条链路，属合理 ignore；但它们**都不在 CI，也没有任何 CI 内的替代断言**，一旦回归只能靠用户报告。建议至少为「helper 脚本契约 + 重启计划」补 CI 内可跑的替代用例（见 A4）。

## A2. 「测试是否真的在测」——断言强度抽查

**总体结论：抽查的测试断言是有效的，不是「渲染没崩」的凑数测试。** 没有 snapshot 兜底（仓库内 0 个 `*.snap`），全仓只有 1 处偏弱的断言。

| 文件 | 断言强度 | 证据 |
| --- | --- | --- |
| `src/app/App.test.tsx`（21 用例 / 87 处 expect） | **强** | 断言真实行为与调用参数：`saveProvider` 的 CAS 版本（`:80` 断言第二次写入带版本 2）、冲突重读两次调用（`:108-109`）、`replaceCredential('k_1','synthetic-secret',2)`（`:206`）、`startProbe(..., {includeGenerate:false})`（`:407`）；含负面断言「不宣称已加载」（`:24-25、:472、:481`）与隐私断言「localStorage 不含秘密」（`:210-211`）。 |
| `src/features/codex/CodexConfigPage.test.tsx`（14 用例 / 43 expect） | **强** | 断言状态机语义：提交后不得出现成功文案（`:53`）、确认前不调 `confirmReload`（`:54`）、计划过期自动重生成（`:136`）、重启三分支（成功/未退出/未起来）文案各不相同（`:214、:231、:246`）、失败走 `alert` 成功走 `status`（`:230`）。 |
| `crates/switch-core/tests/apply_service.rs`（20 用例） | **强** | 断言不变量的**否定面**：计划阶段配置文件不存在（`:263`）、冲突后外部内容原样保留（`:391`）、上游 Key 绝不进 config（`:425、:711`）、提交最多到 `AwaitingReload` 不得自称 Loaded（`:409`）、过期/篡改计划被拦且不写盘（`:538、:555`）、重启后还原外部改过的字段（`:693`）。 |
| `crates/switch-core/tests/gateway_server.rs`（21 用例） | **强** | 断言上游**真实请求体**（`upstream_body["model"]`、`max_tokens`、`input` 结构，`:412-417、:474-477、:795-797`），并含安全断言「上游 5xx 回显的 Key 必须被抹掉」（`:495`）、越权/未知 alias 不进上游（`:330`）、超大体/分块体被拒（`:590、:615`）、客户端断开停止上游流（`:804`）。 |
| `crates/switch-core/tests/config_golden.rs`（24 用例） | **强** | 逐字节保真：注释/未知字段/CRLF/内联表/quoted key/Unicode 路径（`:49-121`）、`integer_notation_is_not_an_external_change`（`:314`）、文件权限保持（`:336`）、「同一版本重复应用幂等」（`:377`）、`redacted_preview_masks_secret_bearing_fields`（`:405`）、拒绝把上游 Key 当 env_key（`:422`）。 |

唯一的弱断言：`src/app/accessibility.test.tsx:70` 的 `expect(document.documentElement.dataset.platform).toBeDefined()`——只是确认属性存在，未验证取值。影响可忽略。

**没有发现** `expect(true)`、`toBeTruthy()` 兜底、快照兜底或大量「只断言渲染成功」的用例。

## A3. 覆盖缺口映射到需求

### P0 清单（`docs/01-product-requirements.md:29-41`）

| P0 条目 | 档位 | 说明 |
| --- | --- | --- |
| 供应商增删改、自定义 URL | 有覆盖 | `App.test.tsx`（增/改/删 + `deleteProvider('p_test')`）、`ProviderForm`。 |
| 供应商**搜索** | 存疑/弱 | 未见针对搜索的独立断言（需另核）。 |
| 预设以填表辅助方式提供 | **完全没测** | `listPresets: async () => []`（`src/desktop/transport.ts:35`、`src/dev/visual-fixture.tsx:114`）——**预设未实现**，仅 `presetId` 透传（`ProviderForm.tsx:111`）。 |
| 每供应商多个 Key：**新增**、替换、禁用、检测 | **完全没测（功能缺失）** | `ProviderForm.write()` 只在「无当前 Key」时 `addCredential`，有当前 Key 时一律 `replaceCredential`（`ProviderForm.tsx:114-124`）。虽有个 Key 下拉（`:353`），但**界面上没有「新增第 2 个 Key」入口**，因此永远造不出第二个 Key。**测试为什么没抓到**：`App.test.tsx` 只测了「首个 Key 新增」与「替换当前 Key」两条路径（`:206、:232`），没有任何用例 `listCredentials` 返回 ≥2 条去断言「能新增/能切换」，缺失正好落在未被断言的路径上。 |
| `/models` 发现 + 手动新增，精确保留大小写/斜杠/Unicode | 有覆盖 | `workspace_service.rs`（`discovery_follows_the_upstream_name...`）、`sqlite_repository.rs`（`model_identity_uses_inherited_protocol_and_is_case_sensitive`）、`App.test.tsx:344`（`vendor/manual`）。 |
| 独立模型编辑：显示名、上游 ID、**协议**、上下文、输出、输入能力、工具能力、推理档位 | **协议字段缺失 → 没测** | `grep protocol` 在 `ModelEditorPage.tsx` / `ModelFormDialog.tsx` **零命中**——编辑器没有协议字段。其余字段有覆盖（`ModelEditorPage.test.tsx:62` 断言 `functionTools` 写入、`ModelFormDialog.test.tsx`、`policy.test.ts` 7 条）。**测试为什么没抓到**：清单里没有「协议」这一项，自然不会有断言。 |
| Codex 接入检测 / 差异预览 / 一次应用 / 失败恢复 / 还原 | 有覆盖（强） | `apply_service.rs` + `config_golden.rs` + `CodexConfigPage.test.tsx`。 |
| 模型在 Codex 原生选择器可见 | **测试存在但不在 CI** | 只有 `scripts/g0/probe-*.mjs` 端到端探针，人工跑（见 A4）。 |
| Responses 原生 + **Chat Completions 工具调用门禁** | **门禁不存在 → 没测** | `gateway_server.rs` 覆盖了 Chat Completions 翻译（`:378`）、模态拒绝（`:715、:751`）、输出上限（`:778`），但**没有工具调用门禁**的用例；需求要求「通过工具调用门禁后才进首发，未通过则标实验」，该门禁未实现。 |
| 分阶段连接测试 / 脱敏日志 / 配置备份 / 托盘 / 关窗继续代理 | 部分 | probe 有覆盖（`ConnectionPage.test.tsx` 4 条 + `probe.rs`）；脱敏日志有覆盖但**强度不足**（见 B3）；托盘/关窗继续代理**无自动化测试**。 |
| macOS arm64 + Windows x64 **签名**包 | **没测** | 见 R01。 |

### R01–R15（`docs/appendix/02-traceability-and-risks.md:7-21`）

| ID | 档位 | 对应测试 / 缺口 |
| --- | --- | --- |
| R01 双平台正式包与兼容矩阵 | **完全没测（Windows）** | `release.yml` 的 `build-windows` 只在 `workflow_dispatch + inputs.with_windows` 时跑（`:113`），且**未签名**（README 明说 SmartScreen 未知发布者，且「第三方模型在 Windows 上尚不可用」）。macOS 只有 arm64 产物。 |
| R02 切换供应商 API Key | 有覆盖 | `App.test.tsx:182`、`workspace_service.rs`（`key_rotation_keeps_old_request_secret...`）。 |
| R03 在 Codex 原生选择器显示 | **测试存在但不在 CI** | 仅 g0 探针。 |
| R04 查看连接状况 | 有覆盖 | `ConnectionPage.test.tsx`、`probe.rs`、`gateway_server.rs:660`。 |
| R05 随时添加 Key 与供应商 | **弱（第 2 个 Key 缺失）** | 同 P0「新增」条目。 |
| R06 减少反复校准与错误（Revision/事务/CAS） | 有覆盖（强） | `apply_service.rs` 全篇、`gateway_server.rs`（绑定的凭据版本变化阻断，`:639`）。 |
| R07 手动添加自定义模型 | 有覆盖 | `App.test.tsx:325`、`ModelFormDialog.test.tsx`。 |
| R08 每模型上下文 | 有覆盖 | `apply_service.rs:293`（未声明 context 拒绝应用）、`App.test.tsx:320`。 |
| R09 每模型最大输出 | 有覆盖（强） | `gateway_server.rs:778` 断言请求实际参数 `max_tokens=2048`。 |
| R10 文本/图片/视频/PDF 四层能力 | 部分 | 模态**声明**与拒绝有覆盖（`gateway_server.rs:715、751`）；四层能力交集/转换路径的可执行性**无端到端覆盖**。 |
| R11 是否思考、可选档位 | 部分 | `policy.test.ts`（7 条）覆盖推理策略映射；「非标准档位被宿主拒绝」只有 g0 人工。 |
| R12/R14 视觉与设计系统 | 部分 | `accessibility.test.tsx`（5 条）、`design-conformance` 技能；属人工/半自动。 |
| R13 参考项目调研 | 不适用（文档项） | `docs/appendix/01-source-index.md` 固定 SHA。 |
| R15 先文档后开发 | 不适用 | — |

## A4. 真机 / 端到端缺口：g0 探针**不在任何 CI**

`scripts/g0/` 有 6 个文件（`probe-full-loop.mjs`、`probe-apply-pipeline.mjs`、`probe-catalog.mjs`、`mock-provider.mjs`、`catalog.mjs`、`rpc.mjs`）。核实结果：

```
grep -rn 'g0\|probe-\|\.mjs' .github/workflows/*.yml   → 零命中
```

- `.github/workflows/ci.yml`：只有 `frontend / core / lint / privacy / workflows` 五个 job，**没有**任何 g0 探针。
- `.github/workflows/release.yml`：也只有构建与签名，**没有**探针。
- 证据归档在 `.local/g0-apply-pipeline.json`、`.local/g0-catalog.json`（gitignore 的 `.local/`）。

**结论：端到端链路（原生选择器可见、真实上游鉴权、Desktop 应用无重载、config.toml 真实写入）过去只有人工跑过，CI 里没有任何一层能拦住它的回归。** 这是本仓库最大的回归风险点之一——`apply_service.rs` 用内存仓储 + 临时目录把核心逻辑测得很细，但它证明不了「真 Codex 是否真的读到了目录」。

建议：把 `probe-catalog.mjs` / `probe-apply-pipeline.mjs` 对 `mock-provider.mjs` 的纯本地部分（不触真实 Codex）接进 CI 的 `core` 或新增 `e2e` job。

## A5. 测试自身的问题

### 夹具（`tests/fixtures/config/*.toml`）——无僵尸夹具

对每个夹具 grep 引用（`crates/switch-core/tests/config_golden.rs` 通过 `fixture(name)` 统一加载，`CARGO_MANIFEST_DIR/../../tests/fixtures/config`）：

| 夹具 | 引用数 | 覆盖的边界 |
| --- | --- | --- |
| `broken.toml` | 1 | 损坏结构（拒绝并保持原文件） |
| `commented.toml` | 1 | 注释与未知字段保真 |
| `crlf.toml` | 1 | CRLF 换行保真 |
| `duplicate-key.toml` | 1 | 重复键 |
| `existing-gateway-provider.toml` | 1 | 已有本工具 provider 的还原 |
| `inline-model-providers.toml` | 1 | `model_providers` 为内联表 |
| `inline-table.toml` | 1 | 内联表保真 |
| `missing-keys.toml` | 1 | 缺键→记录为「不存在」基线 |
| `quoted-keys.toml` | 1 | quoted key 保真 |
| `secret-bearing.toml` | 1 | 含密钥字段的脱敏预览 |
| `unicode.toml` | 1 | Unicode 路径/值 |

11 个夹具全部被引用，**无僵尸夹具**，边界覆盖合理。

### 测试辅助与 setup

- `vitest.setup.ts`：`beforeEach` 固定 `zh-CN` 并 `resetToasts()`（提示队列是模块级状态，不清会串场）——合理且注释清楚。
- `tests/helpers/client.ts`（`testClient`）、`tests/helpers/render.tsx`：被各 `*.test.tsx` 共用。
- 注意：`vitest.setup.ts:23` 的注释承认「用例之间靠已写入的界面偏好串起上下文」——即 localStorage 不重置，存在用例间耦合隐患（目前 112 条全过，未暴露）。

### `act(...)` 警告

- **数量：17 条**，全部集中在 `src/features/codex/CodexConfigPage.test.tsx` 的 3 个用例（stderr 头 3 次），来源栈指向 `App.tsx:65`、`CodexConfigPage.tsx:21`、`Toast.tsx:75`、`ApplyConfirmDialog.tsx:49`。
- 主要来自最后一个用例「提交进行中不能取消：写入不可安全中断」（`:261`）：`releaseCommit()` 在测试函数尾部脱离 `act` 触发状态更新。
- **影响：低**。是 stderr 噪音，不影响断言结果（`pnpm test` 仍 exit 0），但会掩盖未来新增的真实警告。建议把 `releaseCommit()` 包进 `await act(async () => releaseCommit())`。

### 测试目录残留

- `crates/switch-core/examples/zz_audit_probe.rs`（161 行，未跟踪）是**上一轮审计留下的探针文件**，内容只有 loopback 地址、无敏感信息。它是未跟踪文件，会被 `check-publish-safety.sh` 的未跟踪扫描覆盖，**应清理**，否则会成为提交噪音（见 B2-①）。

---

# B. 隐私与泄露扫描

## B1. `scripts/check-publish-safety.sh` 规则集与盲区

脚本（`scripts/check-publish-safety.sh`，143 行）扫描**已跟踪文件 + 未跟踪且未被 gitignore 的文件**（`:24-40`），7 个小节：

| 小节 | 规则 | 判定 |
| --- | --- | --- |
| ① 本机绝对路径用户名 | `/Users/<name>`、`/home/<name>`、`C:\Users\<name>`，放行中性占位 | **fail** |
| ② 私网地址 | 完整四段 10./192.168./172.16-31.，放行 RFC 5737 | **fail** |
| ③ 密钥与令牌形态 | `sk-*`、`github_pat_`、`ghp_`、`glpat-`、`AIza`、`xox*-`、`AKIA`、JWT、PRIVATE KEY；放行合成夹具 | **fail** |
| ④ 凭据库引用 | macOS 凭据库 URI 前缀、`Sec*`/`*Keychain` 类名、登录钥匙串文件名、Codex 的 auth 文件路径 | **fail** |
| ⑤ 个人联系方式 | 邮箱（排除依赖 scope / 文件名） | **fail** |
| ⑥ 签名身份与团队 ID | `Developer ID ...`、`Apple Distribution: ...`、`([A-Z0-9]{10})` | warn |
| ⑦ 私有特征清单 | 读仓库外 `~/.switchelp-private-patterns`（或 `GPTSWITCH_PRIVATE_PATTERNS`/`SWITCHELP_PRIVATE_PATTERNS` 环境变量） | **fail** |

### 实测

```
检查 223 个已跟踪文件 + 3 个未跟踪文件（当前用户：<user>）
① ✗ 命中 2 个未跟踪审计文档（真实用户名路径）
② ✓ ④ ✗（keychain 引用）③ ✓ ⑤ ✓ ⑥ ✓
⑦ · 未配置 ~/.switchelp-private-patterns（可选）
→ 先处理上面标出的内容再提交（exit 1）
```

### 盲区（脚本**漏掉**的东西）

1. **私有特征清单不存在 → 第 ⑦ 节是空转。** 实测 `~/.switchelp-private-patterns` 与 `~/.gptswitch-private-patterns` **都不存在**（脚本默认路径是前者，兼容后者为环境变量名 `GPTSWITCH_PRIVATE_PATTERNS`）。后果：任何「通用规则不认」的真实标识——**第三方供应商域名**、内部主机名——都不会被拦（见 B2-②的 `<上游域名>`）。
2. **不扫 git 历史。** `scan()` 只跑 `git grep`（工作区）+ 未跟踪文件；`git log -p --all` 里的**已删除秘密**完全在覆盖之外。秘密一旦提交再删除，脚本会说「可以发布」。
3. **不扫提交者身份。** `git log` 里的作者姓名/邮箱不在扫描范围（见 B2-④）。
4. **不扫构建产物内容。** `dist-release/`、`dist/` 被 gitignore，脚本从不打开 dmg/zip 内部。二进制里嵌的本机路径（B4）因此不受门禁约束。
5. **实例/修订 id 无规则。** `inst_`/`rev_` 形态不在任何小节（这类串会暴露本机部署标识）。
6. **十六进制/base64 长串无规则。** 网关令牌是 64 位十六进制，但脚本没有「裸 hex ≥32 / base64 ≥40」规则（③ 只认带前缀的形态）。实测 64 位 hex 令牌不会被 ③ 命中。
7. **`safe_metadata` 直写无法被静态检查发现**（属运行期，见 B3）。
8. **规则依赖 `grep -E` 的行匹配，跨行拼接的秘密会漏。**

## B2. 独立复扫：被跟踪文件 + 全部 git 历史（66 个提交）

方法：`git ls-files` 全量 + `git log -p --all` 全量，规则比脚本更宽（含裸 hex/base64、实例 id、域名、邮箱、真实姓名/组织）。

### ① 未跟踪审计文档污染（脚本已抓到，**当前 HEAD 状态 exit 1**）

| 项 | 内容 |
| --- | --- |
| 证据位置 | `docs/audits/2026-09-20-implementation-verification.md:4,23,49,58,69,221,497`、`docs/audits/2026-09-20-requirements-parity.md:6`（均未跟踪） |
| 脱敏后的形态 | `/Users/<redacted>/Code/ProjDev/gptswitch-macapp`、`/Users/<redacted>/.codex/config.toml`、`/Users/<redacted>/Library/Application Support/app.gptswitch.desktop/catalogs/rev_c***/models.json`、`security dump-<keychain> … login.<keychain>-db`、真实 `inst_***`/`rev_***` |
| 危害等级 | **中**（如果被 `git add .` 直接提交，会把本机用户名、Keychain 条目名、真实实例/修订 id 一起公开） |
| 处置建议 | 这几份审计报告提交前必须把本机路径替换成中性占位、删除 Keychain dump 命令与真实 id；或把 `docs/audits/*.md` 加入 `.gitignore`（若审计报告不打算随仓库分发）。**当前仓库在「未跟踪文件」层面不是干净的。** |

### ② 真实第三方上游域名进入被跟踪代码 + git 历史

| 项 | 内容 |
| --- | --- |
| 证据位置 | `src/app/App.test.tsx:70`（`Base URL` 期望值），同一字符串在 git 历史中出现在 `b4a5e42`（该提交为首次引入） |
| 脱敏后的形态 | `https://api.<上游域名>/v1`（真实的中转服务域名，非 `example.test`） |
| 交叉证据 | `<上游域名>` 出现在本机 `~/.codex/config.toml.bak-*` 与多个 `archived_sessions/*.jsonl` 中——**它确实是使用者用过的真实上游**，不是杜撰的测试域名 |
| 危害等级 | **中低**（不泄露密钥，但公开了使用者实际使用的供应商身份，且该字符串会随仓库/发行版一起发布） |
| 处置建议 | 换成 `https://api.example.test/v1`。**注意：仅改当前文件不够——该域名已进入历史**，需要在发布前评估是否接受历史暴露，或按仓库既定规则（不重写历史）在 README/迁移说明里说明。脚本第 ⑦ 节本可拦住它，但清单未配置。 |

### ③ 被跟踪文件中的其余形态（**干净**）

| 类别 | 结论 |
| --- | --- |
| 真实 API key（`sk-*`/`sk-ant-*`/`github_pat_`/`ghp_`/`glpat_`/`AIza`/`AKIA`/JWT/PRIVATE KEY） | **无真实密钥**。历史中出现的 `sk-live-0123456789abcdefghijklmnop`、`sk-test-*`、`sk-legacy-canary-*`、`sk-mcp-canary-*` 全部是合成夹具（顺序/全同字符，被脚本的 `SYNTHETIC` 规则放行）。测试断言本身就在验证「脱敏生效」。 |
| 裸 64 位 hex / 长 base64 | 被跟踪文件中仅 `docs/appendix/evidence-manifest.json` 的 `sha256` 校验和（属文档证据，非令牌），以及 `docs/appendix/01-source-index.md` 里的固定 commit SHA。无令牌。 |
| 实例/修订 id | 被跟踪代码中全是合成占位（`inst_0123ab`、`rev_0123ab`、`rev_old`、`rev_1`）。真实 `inst_f***`/`rev_c***` **未进入仓库**。 |
| 本机用户名路径 | 被跟踪文件只用 `/Users/me/`、`/Users/example/`（`apply_service.rs:734,792`、`config_golden.rs:42`）。**干净**。 |
| Keychain 条目名 | 被跟踪文件无。 |
| 会话/日志残留（`~/.codex` 会话、SQLite、logs） | 仓库树内 **0 个** `*.db`/`*.sqlite`/`*.log`/`auth.json`/`sessions`。`.local/` 被 gitignore。**干净**。 |
| 真实姓名/组织名 | 被跟踪文件中出现的是公开的仓库归属 `nexsjournal`（`README.md:32`、`src/features/settings/SettingsPage.tsx:196`、`src/diagnostics/update.rs:29` 的 Releases API）——即仓库所有者本人，属有意公开。未出现其它姓名/组织。 |
| 私网地址 | 无（`127.0.0.1`/`18765` 是本地回环与默认端口，脚本正确放行）。 |

### ④ git 提交者身份（脚本盲区）

| 项 | 内容 |
| --- | --- |
| 证据位置 | 全部 66 个提交的 `Author`（例如 `git log -1 --format='%an <%ae>'`） |
| 脱敏后的形态 | `Nex <nexs***@gmail.com>` / `nexsjournal <nexs***@gmail.com>` |
| 危害等级 | **低**（GitHub 提交本来公开作者邮箱；且与已公开的 `nexsjournal` 账号一致） |
| 处置建议 | 若不愿公开，改用 GitHub noreply 邮箱；或提交前设 `git config user.email`。不建议为此重写历史。 |

### ⑤ `referimg/` 排除核实

- `git check-ignore -v referimg/` → `.gitignore:18:referimg/`（**确实被忽略**）。
- `git log --all --diff-filter=A -- referimg/` → **零结果**，即第三方截图**从未进入过 git 历史**。
- 结论：**干净**，符合「本地调研素材不分发」的意图。

### ⑥ `dist/` 与 `dist-release/` 是否被跟踪

- `git ls-files dist dist-release` → 空；`.gitignore` 含 `dist/`、`dist-release/`。
- **dmg/zip 未被跟踪**，不会进仓库。但这不代表内容安全（见 B4）。

### ⑦ `.gitignore` 本身的遗漏

当前 `.gitignore` 覆盖：`node_modules/ dist/ target/ .DS_Store .env .env.* coverage/ test-results/ playwright-report/ .local/ .zcode/plans/ referimg/ *.log dist-release/`。

实测**未忽略**的高危项（`git check-ignore` 全部返回 no）：

| 缺失项 | 风险 |
| --- | --- |
| `certificate.p12` / `*.p12` / `*.cer` / `*.mobileprovision` / `*.pem` | `release.yml:199` 会在工作目录解码生成 `certificate.p12`；本地手动签名演练时若敲错命令，证书可被误提交。**建议加入。** |
| `*.sqlite` / `*.db` | 应用的元数据库（含供应商/凭据引用）可能被落到仓库目录。 |
| `.vscode/` / `.idea/` | 编辑器配置常含本机路径。 |
| `.cargo/` / `.envrc` | 本机环境。 |
| `*.bak` | 本机 config 备份的常见形态。 |

（实测当前仓库树内**没有**这些文件，属预防性建议。）

## B3. 运行期泄露：诊断包与日志表

### 结论

**「不记录请求正文」成立；「不导出密钥」在常见形态上成立，但短密钥会漏。** SQLite 不存日志、不存正文、不存密钥。

### ① 请求正文：未被记录（PRD 声称成立）

- `DiagnosticEvent` 的字段是**封闭结构**，没有自由文本槽（`crates/switch-core/src/diagnostics/mod.rs:76-88`）：`timestamp/level/category_key/target_label/result_key/elapsed_ms/safe_metadata`。
- `with_metadata` 只接受 `ALLOWED_METADATA_KEYS`（19 个键，`mod.rs:34-54`），白名单外**直接丢弃**（`:110-116`）。
- 网关记录的事件只填 alias / model_id / provider_id / protocol_id / http_status / revision_id / credential_version / error_code 等结构化字段（`crates/switch-core/src/gateway/server.rs:218-226, 295-305, 359-368, 437-470`）；`error_code` 用 `format!("{:?}", error.code)` 枚举名，**不含上游正文**。
- 上游错误正文只在**响应**里出现一次，且经 `redact(text, secret)` 用当前 Key 做整串替换 + 截断 + 去换行（`server.rs:937-973`）。
- SQLite schema（`crates/switch-core/src/storage/sqlite.rs:26-46`）只有 `providers/credentials/models/revisions/operations` 五张表，**没有 logs 表**；诊断日志是内存环形缓冲（`DiagnosticLog`，默认 2000 条 / 20 MiB / 7 天）。`sqlite_repository.rs` 的 `persists_entities_across_reopen_without_secret_material` 逐字节断言库文件中不含秘密。

### ② 实验：假 key 能不能进诊断包

**实验设计**（临时 integration test，跑完即删；未改业务代码）：构造 10 种真实世界密钥形态，走 `redact_value` + `build_export` 全链路。

**结果：**

| 形态 | 长度 | 是否被脱敏 |
| --- | --- | --- |
| `sk-proj-<48>` | 56 | ✅ 抹成 `••••` |
| `sk-ant-api03-<40>` | 53 | ✅ |
| 64 位 hex（网关令牌形态） | 64 | ✅ |
| 40+ 位混合 base64 | 43 | ✅ |
| **22 位字母数字**（无前缀） | 22 | ❌ **原样泄露** |
| **30 位混合字母数字** | 30 | ❌ **原样泄露** |
| UUID 形态（36 位带连字符） | 36 | ❌ **原样泄露** |
| **29 位带点分隔** | 29 | ❌ **原样泄露** |
| **`Bearer <39 位>`** | 39 | ❌ **原样泄露**（`Bearer ` 前缀只对 `Bearer ` 后紧跟的判定分支生效——此处 `split_keep_separators` 把 `Bearer` 与 token 分成两段，第二段 39 位 < 40 阈值 → 不遮） |
| **`keyid:<38 位>`（冒号粘连）** | 38 | ❌ **原样泄露** |

导出的 `diagnostics.json` 中，形如 `Kq7ZmN2xPv9Rt4Ws6Yd8Lc` 的假 key **原样出现在 `error_code` 字段里**。

**根因**：`looks_like_secret`（`mod.rs:306-323`）只认三种高置信形态——`sk-`/`ghp_`/`xoxb-` 前缀、纯 hex ≥32、任意 ≥40 位字母数字。**20–39 位、无特征前缀的密钥全部放行**；而大量中转/自建供应商的 Key 正是这种长度和字符集。

| 项 | 内容 |
| --- | --- |
| 证据位置 | `crates/switch-core/src/diagnostics/mod.rs:306-323`（`looks_like_secret` 阈值）、`:271-283`（`redact_value`）、`:110-116`（`with_metadata` 只对值调用 redact） |
| 脱敏后的形态 | 假 key `Kq7Z***`（22 位）在导出 JSON 的 `safeMetadata.error_code` 中明文出现 |
| 危害等级 | **中**（前提是某条事件把密钥值塞进白名单键；网关路径目前只用枚举名，所以现实触发概率低，但一旦有代码把上游回显写进 `error_code`/`alias`，短 key 会直接进诊断包并可能被用户截图外发） |
| 处置建议 | ① 把「值脱敏」从启发式升级为**结构校验**：白名单键按语义做类型校验（`error_code` 只接受 `[A-Z_]+`，`http_status` 只接受数字，id 类只接受约定前缀），不合法就丢弃或整体置 `••••`；② 给 `redact_value` 增补规则（≥20 位且大小写数字混合 → 遮；`Bearer ` 后任意 ≥16 位 → 遮）；③ `DiagnosticEvent.safe_metadata` 改为私有 + builder，防止绕过白名单直写。 |

### ③ 其他观察

- `DiagnosticEvent.target_label` 与 `result_key` **不经过 `redact_value`**（只有 metadata 的值过）。网关把 `target_label` 设为 `alias` 或**原始 `request.path`**（`server.rs:222`）。若客户端把密钥塞进 URL 路径，它会以明文进入事件与诊断包。等级：低（需要异常客户端），建议 `target_label` 也过一遍脱敏。
- 网关响应的 `redact(text, secret)` 只替换**当前那一个** secret 字符串（`server.rs:960-964`）；若上游回显的是**另一个**凭据版本或另一供应商的 Key，不会被替换。等级：低，建议叠加 `redact_value` 的通用形态判定。

## B4. 发布物泄露（dist-release/）

对 `dist-release/` 的 **0.2.0 dmg/zip** 做了挂载/解包检查（`/tmp` 内，检查后已 `hdiutil detach` + `rm -rf`）：

| 项 | 结果 |
| --- | --- |
| 产物清单 | 只有 `Switchelp-0.2.0-arm64.zip` 与 `Switchelp_0.2.0_aarch64.dmg`（18:40 构建）。**没有**混入旧 `GPTSwitch` 名产物。 |
| 是否当前 HEAD 构建 | HEAD `2fda2a5` 提交时间 18:40:41，二进制签名时间 18:38:24，产物落盘 18:40。HEAD 本身只是 `Cargo.lock 跟上 0.2.0`，**可判定产物对应当前 HEAD 代码**。 |
| 包名/版本 | zip 与 dmg 内 `Info.plist`：`CFBundleName=Switchelp`、`CFBundleShortVersionString=0.2.0`、`CFBundleIdentifier=app.gptswitch.desktop`、可执行名 `gptswitch`。`README` 提到的「0.1.0 产物带旧名」是历史陈述，**当前 release 目录没有旧名产物**。 |
| 调试残留 | 无 `*.map`（source map）；`dist/` 的 JS bundle（`dist/assets/index-*.js`）**0 个** `/Users/<redacted>` 路径；应用包里没有 JS/HTML 资源文件（前端已编译进二进制）。 |
| **本机绝对路径泄露** | **有。** 发布二进制 `Switchelp.app/Contents/MacOS/gptswitch` 中含 **342 处** `/Users/<redacted-user>/.cargo/registry/src/index.crates.io-<hash>/<crate>-<ver>/src/...` 形态的字符串——Rust 依赖 panic 位置被编译进二进制，**暴露构建机用户名与 `.cargo` 目录布局**。另有 1 处 `ProjDev`、1 处 `gptswitch-macapp`（本仓库路径）。 |
| Team ID | codesign 内 `TeamIdentifier=73T***`（真实 Apple Team ID），属签名包的固有公开信息，与 `README` 的签名说明一致；**未出现在被跟踪文件里**。 |
| 本机实例 id / 上游域名 | 二进制中 `inst_f***`、`<上游域名>` **均 0 命中**——运行期标识与上游域名未进产物。 |

| 项 | 内容 |
| --- | --- |
| 证据位置 | `dist-release/Switchelp-0.2.0-arm64.zip → Switchelp.app/Contents/MacOS/gptswitch`（`strings` 命中 342 行 `/Users/<redacted-user>/.cargo/...`） |
| 脱敏后的形态 | `/Users/<redacted-user>/.cargo/registry/src/index.crates.io-<hash>/<crate>/src/...` |
| 危害等级 | **低**（仅泄露构建机用户名与目录结构，无凭据、无实例 id） |
| 处置建议 | 构建时加 `RUSTFLAGS="--remap-path-prefix=$HOME=/build --remap-path-prefix=$PWD=/src"`（或在 `release.yml` 的构建步骤设置），并在 CI 加一条「产物 strings 不得含 `/Users/`」的门禁。因 `dist-release/` 被 gitignore，发布门禁脚本扫不到它——**这正是 B1 盲区 4 的现实后果**。 |

---

# 未验证项

以下项本轮**未**取得直接证据，不做结论：

1. **`<上游域名>` 是否就是使用者当前在用的上游**——仅从 `~/.codex/config.toml.bak-*` 与 `archived_sessions/*.jsonl` 的历史命中推断，未读取当前活跃配置的 provider 段做交叉确认（避免触碰密钥）。需人工确认后再决定是否清理。
2. **真实 Codex 原生选择器是否可见自有目录**（R03）——需要真机 + 真实 Codex 版本，本轮只确认「无 CI 覆盖」，未真机复跑。
3. **Windows x64 包**——本机为 macOS，未构建/未运行；`release.yml` 的 Windows job 为手动触发且未签名，未验证其实际产物内容。
4. **真实 OS Keychain 往返**（`system_vault.rs` ignored 用例）——按设计未在普通测试中访问系统凭据库，本轮未显式运行。
5. **`git log -p --all` 中 `<上游域名>` 之外是否还有其它已删除的真实标识**——本轮用宽规则扫过全部 66 个提交，未再发现；但字符串级扫描无法覆盖「跨行拼接」「二进制 blob」两类，未做 `git rev-list --objects --all` 的全 blob 遍历。
6. **供应商搜索、托盘、关窗继续代理**（P0 中标注「存疑/无自动化」的条目）——本轮未逐页核对 UI 实现，仅确认缺少对应测试。
7. **`certificate.p12` 等未忽略项是否曾被误提交**——本轮确认当前工作区无这些文件，且 `git ls-files` 无命中；未逐提交核实历史。
8. **`docs/audits/` 是否应纳入版本管理**——属仓库策略决定，本报告只指出「当前 3 份未跟踪审计文档会触发隐私门禁失败」。
