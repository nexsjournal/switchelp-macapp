# 需求一致性审计（2026-09-20）

对象：Switchelp 仓库 `main`（`package.json` / `src-tauri/tauri.conf.json` 版本 **0.2.0**）。
方法：以 `docs/01-product-requirements.md`（PRD）、`docs/appendix/02-traceability-and-risks.md`（R01–R15、Q01–Q09）为需求基准，逐条到代码、测试、CI 与产物里找可复核证据；文档自述不作为结论依据。
证据等级：**A** 代码/测试/配置文件的行级引用；**B** 命令输出与产物；**C** 未执行、未测量（标「未验证」）。
符号约定：`文件:行` 均相对仓库根 `/Users/<user>/Code/ProjDev/gptswitch-macapp`。

---

## 0. 「CodeX Play 到底指什么」的结论

**结论：仓库与 `~/.codex` 中不存在「CodeX Play / Codex Play / CodexPlay」这个名字；最接近、且用户本人明确点名的参考产品是 CodexSplit（曾名 OpenCodex）。**

证据：

1. 全仓库（含 `.git` 历史）检索无 `codex play` / `codexplay` 字样：

```
$ grep -ril "codex[ -]*play\|codexplay\|opencodex" .
crates/switch-core/tests/apply_service.rs
crates/switch-core/src/codex/detect.rs
docs/research/01-reference-projects.md
docs/appendix/01-source-index.md
docs/appendix/02-traceability-and-risks.md
docs/architecture/01-system-architecture.md
docs/appendix/evidence-manifest.json
src/features/codex/CodexConfigPage.test.tsx
src/dev/visual-fixture.tsx
（git grep 覆盖全部 rev-list，命中同一批文件）
```

其中 `opencodex` 的命中全部是 **CodexSplit 的旧名**，不是另一个产品。`codexplay` 在仓库里**零命中**。

2. 全 `~/.codex` 文本文件里唯一命中 `codexplay` 的是浏览器插件的压缩产物标识 `codexPlaywrightInjected`（`~/.codex/plugins/cache/openai-bundled/browser/26.915.31945/scripts/browser-service.mjs`），与产品名无关。

3. `~/.codex/sessions` 中用户的原始需求（2026-09-17 19:17，`sessions/2026/09/17/rollout-2026-09-17T19-17-05-01a0af15-a17a-75f0-af85-00916ec57c68.jsonl` 的 user 消息）逐字写明：

> 「我现在想做一个类似于 codexsplit，或者是之前叫 opencodex 的 桌面端应用，可以支持 mac 和 windows … https://github.com/AITabby/codexsplit … https://github.com/christiandoxa/prodex … https://github.com/Lampese/codex-switcher … 还有这个 星算助手 … 另外我比较喜欢 codexsplit 的设计风格，你可以参考 referimg/ 中的截图 … 核心功能其实是：我主要需要一个工具，能让我的 Codex 切换不同配置的供应商源 API key，并且在右下角的模型这里显示出来。」

同一批会话里用户还说过「CodexSplit 这个是用什么技术方案做的」「我和 星算助手的开发者聊过后…」。**没有任何一轮会话出现「Play」。**

4. `~/.codex/config.toml`（及其备份）里确有 **CodexSplit / OpenCodex 的托管块残留**：

- 当前 `~/.codex/config.toml` 已是 Switchelp 接管（`model_provider = "gptswitch"`、`[model_providers.gptswitch]`、`auth.command` 指向 `gptswitch-auth-helper`）。
- `config.toml.bak-20260920-171739:1,137-141`：`# >>> opencodex managed >>>` 与 `[model_providers.opencodex] name = "CodexSplit"`。
- `config.toml.bak-20260829:2`：`model_catalog_json = ".../OpenCodex/custom_model_catalog.json"`。
- `config.toml.bak-before-fix-20260917-113609:117`：`CODEX_CLI_PATH = "/Applications/CodexSplit.app/Contents/Resources/dist/codex-provider-bridge"`。
- 另见 `~/.codex/opencodex-catalog.json`、`~/.codex/opencodex-journal.json`、`~/.codex/.opencodex-native-main.claim.sqlite`。

5. `~/.codex` 里还有**第三个工具**的残留（memory 提示里提到的「CodexSplit/opencodex 标记」之外）：**CC Switch** 与 npm 版 `opencodex`。证据：`~/.codex/cc-switch-model-catalog.json`、`~/.cc-switch/cc-switch.db`、`~/.codex/.opencodex-native-main.*.sqlite`；存档会话 `archived_sessions/rollout-2026-08-08T17-59-59-*.jsonl` 记录了用户安装 `@bitkyc08/opencodex v2.11.0`（`ocx` CLI、仪表盘 :10100），以及 `lidge-jun/opencodex`。**注意：npm 版 `opencodex` 与 CodexSplit 的旧名同名但是不同项目**，属于命名冲突风险。

对本项目的影响：`crates/switch-core/src/codex/detect.rs:334-347` 已把 `codexsplit` / `opencodex`（映射为 CodexSplit）、`cc-switch` / `ccswitch`、`xingsuan` 列入外来管理器探测；但**未覆盖 CC Switch 的实际文件名/托管块写法与 npm 版 opencodex**（仅靠子串），存在漏报可能（详见 §1.5）。

---

## 1. 参考产品功能对比（PRD §「调研」逐条对照实现）

### 1.1 借鉴自 CodexSplit（黑灰层级、供应商列表、Key 管理、模型命名空间、明确「应用」入口）

| 承诺 | 结论 | 证据 | 差异与影响 |
| --- | --- | --- | --- |
| 保留黑灰层级视觉 | 一致 | `src/theme.ts`、`src/locales/zh-CN.ts`、`docs/design/01-foundations.md`；`docs/audits/2026-09-19-design-conformance.md` | 视觉双平台/缩放实测未做（见 §5） |
| 供应商分组/列表 | 一致 | `src/app/App.tsx:234-251`（列表 + 状态摘要） | 无搜索框（见 P0-2） |
| Key 管理 | **部分实现** | `src/features/providers/ProviderForm.tsx:115-122`（仅在「还没有 Key」时新增）、`:351-354`（>1 时仅下拉切换）、`:288-303`（菜单只有备注/无认证/停用/删除） | 无法在 UI 里新增第二个 Key、改名、删除单个 Key、禁用单个 Key |
| 模型命名空间（alias） | 一致 | `crates/switch-core/src/codex/catalog.rs:150-169,296`（alias 唯一、拒绝重复）、`src/domain` 侧 `CatalogAlias::from_parts` | — |
| 明确「应用」入口 | 一致 | `src/app/PendingApplyBar.tsx`、`src/features/codex/CodexConfigPage.tsx:281-286` | — |
| 借鉴其「供应商表单覆盖完整手工字段」 | 部分实现 | `ModelFormDialog.tsx` / `ModelEditorPage.tsx`（长度/能力/档位齐全） | **缺每模型「协议」字段**（见 P0-4） |

CodexSplit「视觉桥 + 默认补 low/medium/high」这一被点名**不宜沿用**的行为，本项目确实**未照搬**：`crates/switch-core/src/domain/reasoning.rs:65-90`（未声明集合不得设默认、非档位式不投影）、`catalog.rs:255-266`（不可选时给 warning）。结论：**一致（有意排除且执行到位）**。

### 1.2 借鉴自 Prodex（转换有结果等级、续接绑定优先、输出开始后不自动换路）

| 原则 | 结论 | 证据 |
| --- | --- | --- |
| 转换结果分级（lossless/degraded/rejected/unsupported） | 一致 | `crates/switch-core/src/protocols/chat.rs`（`losses`）、`crates/switch-core/src/gateway/server.rs:384-390`（有 loss 即写 warning 日志） |
| 续接绑定优先于调度 | 一致 | `crates/switch-core/src/gateway/server.rs:321-332`（发布后换 Key 直接 `ContinuationBound` 拒绝）；`crates/switch-core/src/domain/error.rs:18` |
| 输出开始后不自动换路 | 一致（更保守） | `crates/switch-core/src/gateway/routing.rs:133-136` `allows_credential_swap` **恒 false**；“请求已提交/未提交”的区分体现为取消即失败：`tests/gateway_server.rs::a_client_disconnect_stops_the_upstream_stream` |
| 不引入其企业面（PG/Redis/多租户/profile） | 一致（有意排除） | 依赖里只有 `sqlite`/`ureq`/`tauri`；`crates/switch-core/src/storage/*` |

### 1.3 借鉴自 Lampese/codex-switcher（Tauri 托盘、平台入口、切换反馈、串行化切换）

| 承诺 | 结论 | 证据 |
| --- | --- | --- |
| Tauri 跨平台壳 | 一致 | `src-tauri/tauri.conf.json`、`Cargo.toml` |
| 托盘 | 一致 | `src-tauri/src/main.rs:70-138` |
| 平台入口 | 一致 | `crates/switch-core/src/platform/mod.rs`（窗口 chrome、helper 文件名、重启计划） |
| 串行化切换 | 一致 | `crates/switch-core/src/application/mod.rs:52`（`mutations: Mutex`）、`apply.rs:321-324`（`commits: Mutex`）、`tests/sqlite_repository.rs::concurrent_connections_cannot_overwrite_a_winning_edit` |
| 不复制账号轮换 / OAuth | 一致（有意排除） | 全仓库无 oauth/订阅相关实现（见 §6） |
| **托盘行为** | **与文档矛盾** | 代码 `src-tauri/src/main.rs:238-245`：`CloseRequested` → `prevent_close()` + `hide()`（关闭窗口隐藏到托盘）。文档 `docs/appendix/01-source-index.md:265` 却写「刻意**不**把关闭窗口改成隐藏到托盘——那会改变用户对『关闭』的预期」。二者相反 |

### 1.4 借鉴自星算助手（模型一次定义、应用到指定工具实例；扫描检测、统一错误入口）

| 承诺 | 结论 | 证据 |
| --- | --- | --- |
| 模型定义与「应用」分离 | 一致 | `ModelEditorPage` / `ModelsPage` 只保存；`CodexConfigPage` 负责应用 |
| 安装检测 / 扫描 | 一致 | `crates/switch-core/src/codex/detect.rs:140`（`detect`）、`:50-66`（实例字段：路径、版本、配置根、冲突管理器） |
| 统一错误入口 | 一致 | `crates/switch-core/src/domain/error.rs`（`CoreError` + `messageKey` + `recoveryActions`）；前端 `toCoreError` |
| 不引入综合工作台/平台目录/登录余额 | 一致（有意排除） | 无相关代码；`src/features/overview/OverviewPage.tsx:15` 明确不展示余额/成功率 |
| 「手工模型表单提供完整上下文/输出/模态矩阵」 | **不一致（更好）** | 星算助手实测未提供完整手工编辑；本项目提供 `ModelEditorPage.tsx:133-166` |

### 1.5 参考产品「不应沿用」项的落实情况

| 参考行为 | 本项目处理 | 结论 |
| --- | --- | --- |
| CodexSplit 目录写视觉桥（模型不支持图片也声明可收图） | 未照搬：`catalog.rs:272-277`（仅投影已声明模态）、`capability.rs:195-209`（未知即 blocked） | 一致 |
| CodexSplit 合并宿主 `models_cache.json` | 未照搬：本项目只写自有目录（`apply.rs:289,816-823`），不改宿主缓存 | 一致 |
| CodexSplit 自动推理档位补齐 | 未照搬：`reasoning.rs:65-79` | 一致 |
| 跨供应商自动发送内容 | 未实现（有意排除）：`routing.rs:134` 恒 false | 一致 |

---

## 2. R01–R15 逐条核验

| ID | 结论 | 真实证据（file:line / 命令） | 差异与影响 |
| --- | --- | --- | --- |
| R01 macOS 和 Windows | **部分实现** | macOS：`.github/workflows/release.yml:160-238`（aarch64/x86_64 矩阵、`codesign --verify`、`spctl --assess`、notarytool）+ 产物 `dist-release/Switchelp_0.2.0_aarch64.dmg`、`Switchelp-0.2.0-arm64.zip`（B）。Windows：`release.yml:104-158`（`x86_64-pc-windows-msvc`）、`platform/mod.rs:406-409`（`gptswitch-auth-helper.cmd`）、`platform/mod.rs:587`（按镜像名结束进程） | 无 Windows 产物；`release.yml:106` Windows job 仅在 `workflow_dispatch && inputs.with_windows` 时构建。Windows 未真机验证 → 「两平台正式包与兼容矩阵」未达成 |
| R02 切换供应商 API Key | **已实现** | `commands.rs:175-188`（`credentials_select`）、`application/mod.rs:189-221`、`gateway/server.rs:321-332`（换 Key 后旧路由拒绝）、`tests/gateway_server.rs::credential_version_change_blocks_the_request`、`tests/workspace_service.rs::key_rotation_keeps_old_request_secret_and_rejects_stale_edit` | UI 只支持「替换」与「切换」（`ProviderForm.tsx:115-122,351-354`）；见 R05 |
| R03 在 Codex 原生模型选择器显示 | **部分实现（宿主 GUI 未验证）** | 代码：`codex/catalog.rs` 全量、`application/apply.rs:225-243,945-969`（写 `model_catalog_json` + `[model_providers.gptswitch]`）。文档声称 app-server `model/list` 实测一致（`docs/appendix/01-source-index.md:184-189,224-267`） | 文档引的证据文件 `.local/g0-apply-pipeline.json` **本机不存在**（`.local` 被 `.gitignore:13` 排除且目录缺失，`ls .local` → No such file）→ 追踪表证据不可复核。`PRD §7` 的「菜单选择与实际路由一致」未验证 |
| R04 查看连接状况 | **部分实现** | `diagnostics/probe.rs:173-206`（connect / credential / model / generate 四阶段）、`commands.rs:437-468`（`gateway_status`）、`OverviewPage.tsx:110-131`（本地服务/模型调用/Codex 加载） | PRD §2 要求「网关、认证、模型调用、工具调用、Desktop 加载」五面。现状：认证并入 probe 时间线；**工具调用无阶段**；`OverviewPage.tsx:117-120` 的「模型调用」是硬编码「未测试」，不读真实状态 |
| R05 随时添加 Key 与供应商 | **部分实现** | 供应商：`ProviderForm.tsx:57-63,175-177` + `commands.rs:122-133`（新增/编辑）；Key：`commands.rs:145-159`（`credentials_add`）**仅在前端「没有当前 Key」时被调用**（`ProviderForm.tsx:115-122`） | 第二个及以后的 Key 在界面上**无法新增**（无「添加 Key」按钮、无备注输入）；`credentials_delete` 有命令但前端仅 `client.ts:199` 定义、无任何调用点（`grep addCredential\|deleteCredential src/` 仅命中定义与测试替身）→ 单个 Key 不能改名/删除/禁用 |
| R06 减少反复校准和错误 | **已实现** | `codex/plan.rs`（`build_plan`/`check_cas`/`decide_recovery`/TTL）、`application/apply.rs:315-408`（CAS→发布→写前后日志→原子写）、`tests/apply_service.rs` 20 项（冲突/过期/篡改哈希/幂等/恢复） | — |
| R07 手动添加自定义模型 | **已实现** | `ModelFormDialog.tsx:74-99`、`ModelEditorPage.tsx:78-103`、`application/mod.rs:223-301`；发现层不覆盖用户手艺（`mod.rs:421-457`、`tests/workspace_service.rs::discovery_updates_the_discovered_layer_without_overwriting_user_values`） | — |
| R08 每模型上下文 | **已实现** | `catalog.rs:201-224,315-317`（`context_window`/`max_context_window` 取用户声明）、`apply.rs:224-229`（应用阶段缺失即拒绝）、`tests/apply_service.rs::plan_refuses_models_without_declared_context` | 有效 context 的「观测」未做（只写目录，无读取宿主 effective config 的路径） |
| R09 每模型最大输出 | **已实现** | `apply.rs:852`（`output_limit` 冻结进路由）、`gateway/routing.rs:81`（`limits()`）、`tests/gateway_server.rs::the_declared_output_limit_reaches_the_upstream_request`（断言上游收到声明上限） | — |
| R10 文本、图片、视频、PDF 等 | **部分实现** | 四层交集：`capability.rs:184-232`；pdf/video 永不投影：`capability.rs:93-105`、`catalog.rs:430-446`（测试）；未声明模态拒绝且不触达上游：`gateway/server.rs:345-371`、`tests/gateway_server.rs::an_undeclared_modality_is_rejected_before_reaching_the_upstream` | **转换路径未实现**：`capability.rs:179`（`conversion_id`）与 `InputPath::Converted` 无任何生产者；PRD P1 的「显式 PDF 转换/图像降级策略」未做。这本身与「不可执行项可见但不可启用」一致，故仍是部分实现 |
| R11 是否思考、可选档位 | **已实现** | `domain/reasoning.rs:51-113`、`catalog.rs:226-266`、`apply.rs:853-859`（档位冻结）、`gateway/server.rs` 档位收口（未声明集合即摘字段，见 `docs/appendix/01-source-index.md:263`） | 上游对档位的真实响应质量未验证（Q05 未验证） |
| R12 参考 CodexSplit 风格 | **部分实现** | 设计与 token：`docs/design/01-foundations.md`、`src/theme.ts`、`theme.test.ts`、`src/app/App.test.tsx`；审计：`docs/audits/2026-09-19-design-conformance.md` | 「双平台视觉/缩放验收」未做（`docs/appendix/01-source-index.md:219-222` 自述为未验证；`platform/mod.rs` 有 Windows 分支但无 OS 真机记录） |
| R13 看三个开源项目和星算助手 | **已实现** | `docs/research/01-reference-projects.md`、`docs/research/02-codex-feasibility.md`、`docs/appendix/01-source-index.md:7-65`（固定 SHA：codexsplit `6cab7ed6…`、prodex `9580d5f1…`、codex-switcher `839f2882…`）、`docs/appendix/evidence-manifest.json` | 星算助手无源码级证据（`01-source-index.md:84-92` 自述仅观察），符合其「保持未知」声明 |
| R14 详细设计到图标/文字/颜色/字体/组件/模块/模板/系统/页面 | **已实现** | `docs/design/01-foundations.md`、`02-icons-type-and-copy.md`、`03-components.md`、`04-pages-and-flows.md`、`05-patterns-and-accessibility.md` | 状态覆盖/无障碍在代码侧有测试（`src/app/accessibility.test.tsx`），真机读屏未验证 |
| R15 先写文档、不直接开发 | **已实现（当时轮次）／追踪表已过时** | `docs/` 全套文档存在；但当前仓库已有完整应用工程（`src/`、`crates/switch-core/`、`src-tauri/`，`git log` 已到 0.2.0） | 追踪表 `docs/appendix/02-traceability-and-risks.md:21` 仍写「本轮文件范围 docs 目录 / 无应用工程与配置改动」。该行未随阶段更新，会让人误判项目仍在纯文档期 |

---

## 3. P0 清单逐条核验（PRD §3）

| P0 项 | 结论 | 证据 | 差异 |
| --- | --- | --- | --- |
| 供应商增删改、搜索；自定义 URL；预设以填表辅助 | **部分实现** | 增删改：`commands.rs:115-133,209-219`、`application/mod.rs:81-116,370-403`；URL 校验归一化：`domain/url.rs`；**搜索：未实现**——`App.tsx:104` 有 `query` state、`:177-178` 有过滤逻辑、`:251` 有空态，但**没有任何绑定 `query` 的输入框**（`grep setQuery src/app/App.tsx` 只命中 `:166,306` 的 `setQuery('')` 复位）。`transport.ts:33` 的 `listProviders` 支持 `filter.query`，但 `App.tsx:133` 调用时不传；**预设：未实现**——`transport.ts:35` `listPresets: async () => []`，无任何消费方 | 搜索是死代码；预设完全缺失（`ProviderPreset` 类型与 `preset_id` 字段存在但无 UI，`ProviderForm.tsx:111` 仅回传 `target?.presetId`）。PRD 要求「不能绑定推荐或充值」——这一点未被违反，但功能本身不存在 |
| 每供应商多个 API Key；默认固定当前 Key；新增、替换、禁用、检测；系统凭据库保存 | **部分实现** | 多 Key 数据结构与切换：`application/mod.rs:189-221`、`domain/credential.rs:142-150`；替换：`commands.rs:160-174`；检测：`diagnostics/probe.rs`；系统凭据库：`credentials/system.rs`、`tests/system_vault.rs::native_vault_round_trip_and_cleanup` | **新增（第 2+ 个）无 UI 入口**；**禁用无实现**：`CredentialStatus::Disabled`（`credential.rs:43`）无任何写入路径，`commands.rs` 无 `credentials_disable`（handler 列表见 `main.rs:246-283`）；**删除单个 Key 无 UI 入口**（命令存在但无调用） |
| 从 `/models` 发现模型 + 手动新增；精确保留 ID 大小写/斜杠/Unicode | **已实现** | `commands.rs:727-768`（`models_discover`）、`diagnostics/discovery.rs`、`application/mod.rs:421-457`；大小写敏感与斜杠：`tests/sqlite_repository.rs::model_identity_uses_inherited_protocol_and_is_case_sensitive`、`CatalogAlias` 解析 | — |
| 独立模型编辑：显示名、上游 ID、协议、上下文、输出限制、输入能力、工具能力、推理档位 | **部分实现（缺「协议」）** | 显示名/上游 ID/上下文/输出/输入/工具/档位：`ModelEditorPage.tsx:119-175`；压缩阈值：`:183-184` | **协议字段缺失**：`ModelDraft`（`application/mod.rs:37-47`）根本没有 protocol 字段，`Model::protocol_override`（`model.rs:254`）只被读、从不被草稿写入（`grep protocol_override` → 仅 sqlite/apply 读取与默认 `None`）。所以「每模型协议覆盖」无法配置 |
| Codex 接入检测、差异预览、一次应用、失败恢复、还原上次配置 | **已实现** | 检测 `detect.rs:140`；差异 `commands.rs:258-298` + `codex/config.rs::diff_managed` + `ApplyConfirmDialog.tsx`；应用/CAS/恢复 `application/apply.rs`；还原 `apply.rs:431-553`；测试 `tests/apply_service.rs`（含 `restore_drops_managed_keys_but_keeps_an_externally_modified_field`、`recovery_*`、`external_config_change_blocks_the_commit_and_keeps_the_foreign_content`） | — |
| 模型在受支持版本的 Codex 原生选择器可见，选中后走对应服务商和 Key | **未验证** | 代码路径完整（同 R03）；本地闭环脚本 `scripts/g0/probe-full-loop.mjs` 走 app-server 层 | **Desktop GUI 选择器从未验证**；且该脚本依赖的 `.local` 证据目录不存在 |
| Responses 原生兼容 + Chat 适配通过工具调用门禁后首发，未通过则明确标实验 | **部分实现** | Responses 透传：`protocols/responses.rs`；Chat 适配：`protocols/chat.rs`；实验提示：`ProviderForm.tsx:337` + `zh-CN.ts:617` 附近的 `providers.chatAdapterExperimental` | **「工具调用门禁」没有任何实现**：`Protocol::is_verified_adapter`（`provider.rs:27-31`）只在单元测试里被引用（`provider.rs:252-253`），生产代码零调用；probe 无工具调用阶段（`probe.rs:181-195` 只有 connect/credential/model/generate）；前端把「实验」写死为静态文案而非读核心结论。结论：**按「明确标实验」这一半满足；按「通过门禁才进首发」这半无证据** |
| 分阶段连接测试、脱敏日志、配置备份；桌面托盘/菜单栏；关闭窗口后可继续代理 | **已实现** | 分阶段：`diagnostics/probe.rs:173-206`；脱敏：`diagnostics/mod.rs:36-56`（19 个 allowlist 键）+ `redact_value` `:271-283`；备份：`codex/backup.rs` + `commands.rs:483-562`；托盘：`main.rs:70-138`；关窗不退出：`main.rs:240-245` | 与 `docs/appendix/01-source-index.md:265` 自述相反（见 §1.3） |
| macOS Apple Silicon 与 Windows x64 的签名安装包及相同核心功能 | **部分实现** | macOS：`release.yml:160-238` + `dist-release/Switchelp_0.2.0_aarch64.dmg`（B） | Windows 无签名安装包、无产物、未真机验证；「相同核心功能」无从比较 |

---

## 4. 功能边界术语（PRD §5）

| 术语 | 实现是否按定义区分 | 证据 | 差异 |
| --- | --- | --- | --- |
| 已保存 | 是 | `ModelLifecycle::Saved`（`domain/model.rs`）、`CredentialStatus::Saved` | — |
| 已应用 | 是 | `ApplyStage::{Prepared,Committing,AwaitingReload}`、`AppliedSummary.stage`（`apply.rs:558-596`） | — |
| **已加载** | **是（关键不变量成立）** | `catalog.rs:333-341` `host_state_after_publish` 把 `Loaded`/`NotInCatalog`/`PendingApply` 一律降回 `AwaitingReload`；`apply.rs:411-428` 只有 `confirm_reload(loaded=true)` 才 `mark_models_loaded`；测试 `catalog.rs::publish_never_claims_loaded_state`、`apply_service.rs::unconfirmed_reload_stops_at_pending_instead_of_claiming_loaded`、`workspace_service.rs::model_save_recomputes_host_capabilities_and_never_claims_loaded` | **「已加载不能由保存推断」在代码里确实成立** |
| **已验证路由** | **否（无独立状态）** | 全仓库无「route verified」状态或按路由持久化的验证结论；`grep -i "verified"` 在 `gateway/` 只命中 `ProbeState`/`CredentialStatus` | PRD 定义「特定请求实际匹配预期供应商、模型和凭据版本」。实现只做到：网关侧按路由快照准入并记录诊断（`gateway/server.rs:282-307`），以及用户手工在 Codex 里确认加载（`confirm_reload`）。**没有任何机制把一次真实请求的结果写成「该路由已验证」**；probe 的 `generate` 阶段直连 `provider.endpoint`（`probe.rs:265-330`），**不经过本机网关**，因此不验证网关路由 |
| 恢复原生 | 是 | `apply.rs:431-481,484-553`（三方比较、保留外部改动、清空 ownership、回落 `PendingApply`） | PRD 定义「不是清空用户所有自定义设置」——实现只撤销受管字段，一致 |

---

## 5. 非功能目标（PRD §6）

PRD 自己声明「下列均为未来验收目标，尚无实测数据」。核验结果：

| 指标 | 是否有实测数据 | 证据 |
| --- | --- | --- |
| 切换 Key／发布请求策略 p95 ≤ 300 ms | **无** | 仓库无基准脚本；`scripts/` 下只有 `g0/*` 功能探针，无 `criterion`/`bench`（`grep bench scripts docs` 无命中）。仅 `docs/development/02-testing-and-release.md:86` 描述「将来要记录 p50/p95」 |
| 网关附加延迟 p95 ≤ 30 ms | **无** | 同上；`scripts/g0/mock-provider.mjs` 只做功能 mock，无延迟测量 |
| 配置安全（中断可恢复、保留注释/换行） | **部分有测试、无故障注入实测** | 有金样本测试：`tests/config_golden.rs`（24 项，含 `preserves_comments_unknown_fields_and_sections`、`preserves_crlf_line_endings`、`atomic_write_leaves_no_partial_file`、`atomic_write_preserves_file_permissions`）；「中断任何写入阶段」的故障注入未有脚本 |
| 首次打开 p95 ≤ 2 s | **无** | 无冷启动测量代码或记录 |
| 资源预算（≤80 MB / ≤180 MB） | **无** | 无内存测量；`dist-release` 只有安装包，无测量报告 |
| 可理解性：新用户不编辑 TOML 完成添加、检测、应用、恢复 | **结构上成立，行为未验证** | 全流程都有 UI：`OnboardingPage.tsx`（3 步向导）、`ProviderForm`、`ModelEditorPage`、`CodexConfigPage`、`SettingsPage` 危险区「还原为原生 Codex」（`:205`）。**初始默认协议是 `chat_completions`**（`ProviderForm.tsx:59`）而该适配器自带「实验」提示（`:337`），可能让新用户第一步就落在实验路径上 | 无用户测试（可用性测试）记录 → 「不编辑 TOML 即可完成」按代码路径成立，按实测未验证 |
| 可访问性（AA、键盘、读屏、200% 缩放） | **部分自动化、无真机记录** | `src/app/accessibility.test.tsx`（skip-link 焦点、表头 `scope`、`data-platform`）、`src/i18n.locale.test.tsx`；对比度在 `docs/audits/2026-09-19-design-conformance.md` 有静态结论 | 读屏（VoiceOver/NVDA）与 200% 缩放实测无记录（`docs/appendix/01-source-index.md:219-222` 自述未验证） |
| 隐私：不上传遥测、不记录请求正文、不导出密钥 | **结构上成立，且有 CI 门禁** | 不上传：无任何遥测上报代码；不记正文：`diagnostics/mod.rs:36-56` 唯一 allowlist，`with_metadata` 丢弃白名单外键（`:110-118`）；导出严格排除正文/密钥：`diagnostics/export.rs:57-64,102-121`，测试 `export.rs::export_never_contains_a_gateway_token_or_upstream_key`、`export_declares_its_redaction_rules`；`tests/config_golden.rs::rejects_upstream_key_as_env_key_projection`、`ownership_records_do_not_store_secrets_verbatim`；发布前扫描 `scripts/check-publish-safety.sh` 接入 `ci.yml` 的 `privacy` job | 备份文件**可能含第三方写入的密钥**（`commands.rs:481-528` 明示，并只给遮罩预览），已被排除在诊断包之外（`export.rs:117-121`）。整体属于「结构证明 + 测试」，非运行期实测 |

---

## 6. 未在需求里的实现（反向检查）

### 6.1 PRD 未要求、也不在「当前不做」清单里的功能

| 功能 | 证据 | 评价 |
| --- | --- | --- |
| 更新检查（读公开 Release，不下载） | `commands.rs:564-598`、`diagnostics/update.rs`、`SettingsPage.tsx:162-171` | 额外功能；只读、有失败态，无隐私问题。需求侧未提及 |
| 网关「暂停新请求」 | `commands.rs:619-634`、`main.rs:82-125`（托盘项）、`SettingsPage.tsx:120-127` | 额外功能。语义克制（不中断在途请求），但 PRD 未要求 |
| **应用后自动强制重启宿主 Codex** | `CodexConfigPage.tsx:184-188`、`PendingApplyBar.tsx:79-85`、`commands.rs:339-374`、`platform/mod.rs`（重启计划/信号兜底） | PRD 只在 US-01 写「保存不会偷偷重启 Codex」并要求执行后显示「已加载 / 等待 Codex 重新加载」；`docs/architecture/02-configuration-lifecycle.md` 的流程以「等待重载」为主。代码把「写配置 → 立即重启宿主」合并成一步（git commit `73bec70`「应用后自动重启 Codex」）。差异页文案有披露（`ApplyConfirmDialog.tsx:132`），因此**不是偷偷重启**；但 `quit_forced` 分支（`CodexConfigPage.tsx:112-121`）承认可能丢失未保存对话。是否越出 PRD 范围，建议产品侧明确 |
| 模型/供应商**批量**移出目录与批量删除 | `ModelsPage.tsx:92-129,177-185` | PRD P1 提的是「批量模型编辑」；实现的是批量目录/删除，属于不同能力（不算满足 P1） |
| 备份的浏览/预览/恢复 UI（设置页） | `SettingsPage.tsx:175-190` | PRD P0 有「配置备份」，此处是合理展开 |
| 主题/语言切换、i18n（中英） | `SettingsPage.tsx:88-108`、`src/i18n.ts`、`src/locales/*` | 额外；`docs/appendix/02-traceability-and-risks.md:68` 的假设里有「中文优先」，未排除英文 |

### 6.2 「当前不做」清单里是否被偷偷实现

PRD §3「当前不做」：官方账号轮换、OAuth 订阅导入、语音栏、GPT-Live、子智能体调度、会话迁移/删除、插件市场、远程控制、局域网网关、云同步、平台充值与价格商城。

```
$ grep -rin "oauth|订阅导入|语音栏|gpt-live|子智能体|会话迁移|插件市场|远程控制|局域网|云同步|充值|价格商城" \
    crates/switch-core/src src src-tauri/src
（仅命中 auth.localNoAuth / providers.noAuth*，属于「无认证供应商」文案，与账号轮换无关）
```

**结论：无一项被实现，全部为「有意排除」。** 另外 PRD §3 末段「当前范围未包含内嵌浏览器」也成立：无内嵌浏览器组件，外链走系统打开（`SettingsPage.tsx:171` 用 `target="_blank"`；`main.rs:126-152` 只 `open -a ChatGPT`）。

---

## 7. 未验证项（明确列出，不得据此声称支持）

1. **Desktop GUI 原生模型选择器**：现有全部证据在 app-server 层（`docs/appendix/01-source-index.md:9-12 节`），GUI 未验证。Tauri/Codex 版本组合下的实际菜单行为未知。
2. **Q04 的证据不可复核**：`docs/appendix/02-traceability-and-risks.md:50` 与 `docs/appendix/01-source-index.md:184-189` 引用的 `.local/g0-apply-pipeline.json` **不存在**（`.local` 被 gitignore 且本机无此目录）。追踪表把 Q04 标为「✅ 已通过」缺乏可复核证据，应降级为「未验证」或补回证据。
3. **`.local` 内其余证据**：`02-traceability-and-risks.md:57-64` 的现场观测、`01-source-index.md:248` 的「实测结论」同样只有 `.local`（不存在）作为落点。脚本可重跑（`probe-full-loop.mjs`），但本轮**未执行**，故全部标未验证。
4. **真实第三方供应商**：所有链路证据的上游都是本地 mock（`scripts/g0/mock-provider.mjs`）。真实供应商的响应质量、`/models` 返回、参数遵从（Q05、Q07）未验证。
5. **Q01 / Q02 / Q03 / Q06 / Q09**：`02-traceability-and-risks.md:47-55` 仍为「待验证」，代码里无对应结论。Q06「每模型 context 是否被全局覆盖」尤其值得注意：本项目**不再写全局 `model_context_window`**（`apply.rs:966` 注释与实现），但这只证明「本工具不写」，不证明「宿主不会用其他来源覆盖」。
6. **Q08 星算助手 Rust 后台**：无公开源码，保持未知（与文档一致）。
7. **Windows 真机**：安装、`.cmd` helper、字体/滚动条、打包、签名全部未验证；`dist-release/` 无 Windows 产物。
8. **macOS Intel**：`release.yml:163` 矩阵含 `x86_64-apple-darwin`，但无产物、无记录。
9. **性能与内存**：PRD §6 全部目标值无实测（见 §5）。
10. **无障碍**：VoiceOver / NVDA 实际读屏与 200% 缩放无记录。
11. **测试基线未在本轮执行**：前端 `src/**/*.test.tsx` 共 115 个 `it/test` 用例（按文件统计），核心 `crates/switch-core/tests/` 共 88 个 `#[test]`（7 个文件之和），本轮**未运行**；结论均基于源码与测试文件的存在性，不基于「测试通过」。
    - 统计口径（命令）：`grep -c '#\[test\]' crates/switch-core/tests/*.rs` 合计 88；前端 `grep -c "it(\|test(" src/**/*.test.ts*`。
12. **`dist-release/` 产物的签名有效性**：文件名与目录存在（B），本轮**未运行** `codesign --verify` / `spctl --assess` 复核，也未核对是否已公证。

---

## 8. 发现清单（按优先级）

### P0（阻塞首发验收）

- **P0-1 多 Key 管理不闭环**：能存多个 Key，但 UI 无法新增第 2+ 个、改名、删除单个、禁用。「每供应商多个 API Key；新增、替换、禁用、检测」只完成了「替换/切换/检测」。证据：`ProviderForm.tsx:115-122,288-303,351-354`；`commands.rs`（无 `credentials_disable`）；`credential.rs:43`（`Disabled` 无写入路径）；`grep addCredential|deleteCredential src/` 仅命中定义与测试替身。
- **P0-2 供应商搜索是死代码**：有 state、有过滤、有空态，无输入框。证据：`App.tsx:104,177-178,251` 对 `setQuery` 的调用只有 `:166,306` 的复位。
- **P0-3 供应商预设未实现**：`transport.ts:35` 返回空数组，无消费方，无 UI。PRD P0 明确要求「预设以填表辅助方式提供」。
- **P0-4 模型编辑器缺「协议」字段**：`ModelDraft` 无 protocol（`application/mod.rs:37-47`），`protocol_override` 从不被写入（只被读）。每模型协议覆盖无法配置。
- **P0-5 「工具调用门禁」不存在**：`is_verified_adapter` 仅测试引用（`provider.rs:252-253`），probe 无工具阶段，前端「实验」是静态文案。Chat 适配以「明确标实验」满足了半条要求，但 P0 的「通过门禁后进入首发」无任何可执行证据。
- **P0-6 已加载/已应用不变量成立，但「已验证路由」没有实现**：无按路由的验证状态；probe 的 `generate` 直连上游、绕过网关，无法验证网关路由（`probe.rs:265-330`）。
- **P0-7 Windows 签名安装包不存在**：仅 CI 手动路径（`release.yml:106`）与 `.cmd` helper 代码（`platform/mod.rs:406-409`），无产物、无真机验证。R01「相同核心功能」无从比较。

### P1

- **P1-1 同供应商 Key 故障切换未实现**：`routing.rs:134` 恒 false（这是有意的保守设计，与 Prodex 原则一致，但与 P1 清单不符）。
- **P1-2 P1 缺口**：批量模型编辑（现有的是批量目录/删除）、配置模板、加密导入导出、托盘快捷启用 —— 均无实现。
- **P1-3 Anthropic Messages 未实现**（有意，且 UI 明示：`zh-CN.ts:617`）。
- **P1-4 转换路径未实现**：`conversion_id`/`InputPath::Converted` 无生产者；PDF 转换/图像降级策略未做（与「不可执行项可见但不可启用」一致，不算违规）。
- **P1-5 macOS Intel 与 Windows ARM64**：Intel 有构建矩阵无产物；ARM64 未评估。
- **P1-6 应用后自动重启宿主**：需求未明确要求、与 US-01「等待重新加载」流程有张力；`quit_forced` 分支可能丢未保存内容（`CodexConfigPage.tsx:112-121`）。建议产品侧确认。

### P2（文档/证据卫生）

- **P2-1 托盘行为文档与代码相反**：`main.rs:238-245`（隐藏到托盘）vs `docs/appendix/01-source-index.md:265`（自述不这么做）。应改文档（代码行为更符合 PRD「关闭窗口后可继续代理」）。
- **P2-2 Q04 证据文件缺失**：`.local/g0-apply-pipeline.json` 不存在，追踪表却标「✅」。应降级或补证据（`02-traceability-and-risks.md:50`、`01-source-index.md:184-189`）。
- **P2-3 R15 行过时**：`02-traceability-and-risks.md:21` 仍写「本轮只改 docs」，项目已到 0.2.0。
- **P2-4 外来管理器探测不全**：`detect.rs:334-347` 以子串匹配 `codexsplit/opencodex/cc-switch/ccswitch/xingsuan`。本机实际残留包含 `[model_providers.opencodex] name = "CodexSplit"`（能命中）、CC Switch 的 catalog 文件名 `cc-switch-model-catalog.json` 与 `~/.cc-switch/`（文件名能命中，若只出现在路径则不会进 config 文本）、npm 版 `@bitkyc08/opencodex` 的 `ocx` 托管块（未覆盖）。建议按真实残留样本补 fixture。
- **P2-5 PRD §2 五类状态只呈现三类**：`OverviewPage.tsx:110-131`（本地服务 / 模型调用（硬编码未测试）/ Codex 加载），缺独立「认证」「工具调用」面。
- **P2-6 `OverviewPage.tsx:117-120` 的「模型调用」不可信**：它固定显示「未测试」，即便刚跑过 probe 也不会更新。PRD §7 要求「菜单选择与实际路由一致」这类可观测结论，此处应接入真实数据或明确文案边界。
- **P2-7 前端默认协议为实验路径**：`ProviderForm.tsx:59` 默认 `chat_completions`，而该适配器自带实验提示（`:337`）。新用户首条路径落在「实验」状态，与「可理解性」目标相悖。

---

## 9. 复现命令（本报告用到的检索）

```bash
# CodeX Play
grep -ril "codex[ -]*play\|codexplay\|opencodex" .
git grep -il "codex[ -]*play\|codexplay\|opencodex" $(git rev-list --all)
grep -ril "codex play\|CodeX Play\|codexplay" ~/.codex/sessions ~/.codex/archived_sessions ~/.codex/session_index.jsonl

# ~/.codex 残留
grep -in "codexsplit\|opencodex\|cc-switch\|managed" ~/.codex/config.toml ~/.codex/config.toml.bak-*

# 证据文件是否存在
ls ~/.codex/.opencodex-native-main.* ~/.codex/opencodex-*.json ~/.codex/cc-switch-model-catalog.json
ls -la .local            # → No such file or directory

# 缺口的代码侧反证
grep -rn "addCredential\|deleteCredential" src/            # 仅定义 + 测试替身
grep -rn "setQuery" src/app/App.tsx                        # 仅复位，无输入框绑定
grep -rn "is_verified_adapter" crates src src-tauri        # 仅测试
grep -rn "protocol_override" crates/switch-core/src src/   # 只读，无写入
ls .github/workflows && grep -n "with_windows\|workflow_dispatch" .github/workflows/release.yml
```

## 10. 审计边界

- 只做了静态阅读与检索；**未构建、未运行应用、未执行测试、未做性能/内存/无障碍/签名实测**。所有「已实现」指代码路径与测试文件存在，「一致」指实现与需求描述不冲突。
- 未改动任何业务代码；本报告是唯一新增文件。
- 文档自述（含 `docs/appendix/01-source-index.md` 第 9–12 节的实测结论）一律按 C 级处理，除非有仓库内可复核的产物或测试为证。
