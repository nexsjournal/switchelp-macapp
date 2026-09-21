# 产品现状盘点 + 专项审核章程

日期：2026-09-22 · 角色：产品总监（Head of Product）
仓库：`/Users/example/Code/ProjDev/gptswitch-macapp` · 工作区版本：**0.3.0**（`package.json` / `Cargo.toml` / `src-tauri/tauri.conf.json` 三处一致）
已发布正式版：**0.2.0**（仅 macOS Apple Silicon，已签名未公证）
本轮基线：`pnpm typecheck` + 187 前端用例 + `pnpm build` 全绿；`cargo fmt --check` / `clippy -D warnings` / `cargo test -p switch-core` / `cargo test -p gptswitch-bridge`（10 项）全绿；CI 七个 job 齐备（`frontend` / `core` / `lint` / `pipeline` / `probes` / `privacy` / `workflows`）。
本文件是**章程**，不是审计结论。除本文件外本轮不改任何源码，也不跑测试。

---

## 一、产品现状一句话定位

**Switchelp 是一个 macOS 本地配置应用：让第三方供应商的模型出现在 Codex 自己的模型选择器里，并管理供应商 / API Key / 每个模型的上下文、输出上限与推理档位。**

- **给谁用**：已经装了 Codex Desktop、手里有自定义或中转 API Key 的个人开发者（PRD `docs/01-product-requirements.md:7`）。不是团队、不是企业、不是要综合工作台的人。
- **现在处于什么阶段**：**「机制已验证、产品未成形」的可发布前夜**。核心链路（本机网关 → 事务写盘 → 宿主重载）在本机对真实上游跑通过（`README.md:120-130`）；但产品的表达、信息架构与转化路径仍停在「配置治理台」而不是「让我的 Key 能用的工具」（`docs/audits/2026-09-20-product-review.md` §1）。不到「可发布」，更不是「已发布迭代」——0.2.0 只是把包挂出去了。
- **最难的那个问题**：**全部价值都押在一个不完全由自己掌控的宿主行为上**——「模型真的出现在 Codex GUI 的模型选择器里」。这条在 09-19 / 09-20 / 09-21 连续多轮审计里都是「未验证」（`README.md:131-132`、`docs/audits/2026-09-20-audit-synthesis.md` §6）。它成立，产品才有意义；它漂移，供应商/Key/能力编辑全部退化成「一个 config.toml 图形界面」。

---

## 二、已交付能力清单（用户可见的三组九入口）

入口取自侧栏实际导航 `src/app/App.tsx:35-45` 与底部设置入口（`src/app/App.tsx:297`）：主流程组 3 个、扩展组 3 个、诊断与设置组 3 个。

### 主流程组

| 入口 | 能力（一句话） | 状态 |
| --- | --- | --- |
| **概览** `overview` | 当前路由 / 连接状态 / 供应商 / 待应用模型四张卡，加一条接入进度 | 已实现未验证（真实数据源与「模型调用」面是否读真值待专项核） |
| **网关** `providers`（供应商与模型） | 供应商增删改 + 搜索、每供应商多 Key、`/models` 发现与手动新增模型、独立模型编辑（显示名/上游 ID/协议/上下文/输出/能力/推理档位） | 已实现，部分未验证：搜索框已补（`src/app/App.tsx:354`）、模型级协议字段已补（`src/features/models/ModelEditorPage.tsx:144-146`）、多 Key 闭环已补（批次 E）——**供应商预设仍未实现**（全仓库无 `listPresets`，`ProviderPreset` 类型仍在 `src/contracts/types.ts:70`） |
| **Codex 配置** `codexConfig` | 接入检测、差异预览、「Codex 模型菜单会被替换」声明、一次应用（计划 → CAS → 原子替换）、失败恢复、还原原生；共存模式 Bridge 开关 | 已实现未验证（写入事务有自动化测试；宿主真机 GUI 菜单与 Bridge 界面层均未验证） |

### 扩展组

| 入口 | 能力（一句话） | 状态 |
| --- | --- | --- |
| **内容中心** `content` | 从公开源（RSS / GitHub 搜索 API）抓本地资讯快照，三页签（AI 资讯 / GitHub 热门 / 订阅源） | 已实现未验证（夹具渲染已审计；真实抓取成功率/限流未在运行时验证，`docs/development/02-testing-and-release.md:12-14`） |
| **工具管理** `tools` | 只读展示本机已装工具/CLI 与探测结果，展开看探针原文 | 已实现未验证（探针在真机上的覆盖范围待核） |
| **插件中心** `plugins` | 从公开 GitHub 仓库安装 `SKILL.md` 技能到本机 agent 工作台，按清单卸载、冲突不覆盖 | 已实现未验证（**从未对真实 `~/.codex/skills` 做过一次真实安装**，`docs/audits/2026-09-21-extension-pages-design-conformance.md` §2） |

### 诊断与设置组

| 入口 | 能力（一句话） | 状态 |
| --- | --- | --- |
| **连接诊断** `diagnostics` | 分阶段 probe（connect / credential / model / generate）与连接状态 | 已实现，部分未验证（PRD §2 要求的五面里「工具调用」无独立阶段；probe 的 generate 直连上游、不走本机网关，`docs/audits/2026-09-20-requirements-parity.md` §5） |
| **日志** `logs` | 脱敏诊断事件（allowlist 结构化抽取 + 二次脱敏） | 已实现且有 CI 隐私门禁（`privacy` job） |
| **设置** `settings` | 外观与语言（中英双语、跟随系统）、网关状态、许可证、备份浏览、还原原生、重新打开接入向导 | 已实现未验证（双语字典一致性有 `src/i18n.test.ts` 守；运行期诊断包脱敏规则有历史洞，见 §四） |

**跨入口的横向能力**：双模式（替换 / Bridge 共存）、应用内更新（侧栏胶囊 + 更新弹窗，`src/features/update/`）、托盘常驻。更新链路的真实下载安装**未验证**（`docs/architecture/06-updates.md` §10.4）。

**未实现（PRD 明确承诺过）**：供应商预设（P0）；Chat Completions 的工具调用门禁（P0 只落地了「标注实验」这一半）。**未实现（P1，有意的）**：Key 故障切换、批量模型编辑、Anthropic Messages、PDF/图像转换路径、macOS Intel / Windows ARM64。

---

## 三、核心链路（3 条）

### 链路 1：首次接入 → 应用 → 宿主重载 → 模型出现在 Codex 菜单

这是全部价值所在。关键落点：

- 检测：`crates/switch-core/src/codex/detect.rs`
- 目录编译 / 别名：`crates/switch-core/src/codex/catalog.rs`
- 计划 → CAS → 原子替换 → 发布/回收：`crates/switch-core/src/application/apply.rs`、`crates/switch-core/src/codex/config.rs`
- 界面：`src/features/codex/CodexConfigPage.tsx`、`src/features/codex/ApplyConfirmDialog.tsx`、`src/app/PendingApplyBar.tsx`
- 命令面：`src-tauri/src/commands.rs`
- 自动门禁：CI `pipeline` job 跑 `g0_apply_pipeline`（真实 config.toml + 模型目录 + helper）；人工层 `scripts/g0/probe-*.mjs`（需真 Codex 可执行文件，`docs/development/02-testing-and-release.md:141-154`）

**此链路唯一没有门禁的一段**：宿主 GUI 选择器的可见性与实际路由。只能人工，本机环境也无法自动化。

### 链路 2：请求路由 → 本机网关 → 上游（含协议翻译）

- 网关与鉴权：`crates/switch-core/src/gateway/server.rs`、`gateway/auth.rs`、`gateway/routing.rs`
- 协议适配与「损失记账」：`crates/switch-core/src/protocols/responses.rs`、`protocols/chat.rs`
- 凭据：`crates/switch-core/src/credentials/`（只进系统钥匙串；`config.toml` 只见本地 token）
- 测试：`crates/switch-core/tests/gateway_server.rs`

### 链路 3：共存模式（Bridge）——一份菜单、两根 codex

- 桥进程与线程路由：`crates/bridge/`
- 共装配：`crates/switch-core/src/codex/coexist.rs`
- 装机/注入：`src-tauri/`（`CODEX_CLI_PATH` + `open --env`）
- 自动层 `cargo test -p gptswitch-bridge`（10 项）；人工层 `scripts/g0/coexist-check.mjs`；**界面层只能人工**（`docs/development/02-testing-and-release.md:20-32`）

---

## 四、本轮审核的裁决标准

### 严重度

- **P0（挡住发布）**：违反 PRD 的 P0 承诺；或挡住主流程（用户走不完「接入 → 应用 → 在 Codex 里用上」）；或会静默损坏用户的 Codex 配置 / 泄漏上游密钥；或产品对用户声称成功而实际失败。P0 一旦成立，**版本不得称稳定版**（PRD §7）。
- **P1（本轮必须修）**：违反 `docs/design/*` 明文规范、`docs/development/02-testing-and-release.md` 的验收条目、或数据安全 / 可访问性底线；以及任何「核心层没实现、界面却像能用」的假开关。**「不做假开关」是产品原则，本轮按 P1 严格执行。**
- **P2（记入待办）**：观感、术语、一致性、文档卫生、协作面。可以只记录，不阻塞。

### 明确不计入不合格的项（已登记的例外）

1. **表单控件静息态描边低于 3:1**：`--border-field`（深色 `#50505C`、浅色 `#AAAAB2`）不满足非文本 3:1。这是 **2026-09-21 的产品决定**（用户在拿到实测数字与替代方案后明确选择「接受低于 3:1」）。登记在 `docs/design/01-foundations.md:89-116` 与 `docs/development/02-testing-and-release.md:111-116`。**审核按例外处理，不得报成不合格**；悬停档 `--border-control` 与焦点环仍需满足 3:1。
2. **有意的范围排除**：PRD §3「当前不做」清单里的东西（账号轮换、OAuth 导入、语音栏、插件市场/云端目录、云同步、充值商城等）**不是缺失，是决定**。不得当缺陷报。Windows 当前被前置检查拦下不写配置，是「不假装成功」的正确行为，不是未完成。

### 对历史审计的纪律

- **已经认账的问题不得当新发现刷数量**。`docs/audits/` 下 12 篇历史审计（含 `2026-09-20-audit-synthesis.md` 的问题总表）已登记的问题，本轮**只能做「验证是否已修复」**，并给出与登记条目一一对应的结论（已修复 / 未修复 / 部分修复）。
- 每份专项报告必须带一节「与历史审计的对照」：本次结论**新增**了什么、**推翻**了历史哪条、**复核**了哪条仍存在。没有这一节的报告不收。
- 每条结论必须给出**可复核的证据形态**：`文件:行`（行号必须当场核过）、命令与其原始输出、或实测数字。**没量过的写「未验证」，不许写推断当结论**，不许编数字。

---

## 五、六个专项审核员的任务书

通用要求：结论先行；每条给证据（`文件:行` / 实测数字 / 命令输出）；单列「未验证项」；单列「与历史审计的对照」。**不得重复计入历史已认账项**。

### 5.1 需求审核员

- **读**：`docs/01-product-requirements.md`（尤其 §3 的 P0 清单、§5 四态定义、§7 发布验收）、`docs/development/02-testing-and-release.md` §4 的 **AC-01~AC-17**、`docs/appendix/02-traceability-and-risks.md`（R01–R15）、`docs/audits/2026-09-20-requirements-parity.md`（作为历史基线）。
- **验的命题**：
  1. PRD P0 每一条 **对照当前 0.3.0 代码** 的落地状态（历史审计已被修复的必须记为「已修复」，不要照抄旧结论）；特别复核历史上认账的：多 Key 闭环、模型级协议字段、供应商搜索、预设、工具调用门禁、`已验证路由` 是否有独立状态。
  2. **AC-01~AC-17 逐条**给出「有自动化覆盖 / 只有人工 / 无覆盖」三分类，并指出哪些 AC 当前**没有任何测试**指向它。
  3. 「做了但没承诺」的反向检查：更新检查、网关暂停、扩展三页（工具/插件/内容——PRD 明说未确认前不作为需求生效，`docs/01-product-requirements.md:60`）是否已越出需求边界。
- **合格线**：PRD P0 每一条都有明确判定；AC 表无遗漏；反向检查有结论。**不许把「未承诺」直接写成缺陷**——只标注「越界待产品确认」。
- **证据形态**：`文件:行` + 测试文件/命令。**未验证/未实测**：真机 GUI 菜单、Windows、真实上游质量。

### 5.2 设计与交互体验审核员

- **读**：`docs/design/*.md` 全 6 篇（重点 `01-foundations.md`、`05-patterns-and-accessibility.md`、`06-tool-hub-plugin-hub-and-content-center.md`）、`docs/audits/2026-09-21-extension-pages-design-conformance.md`、`docs/audits/2026-09-21-update-dialog-design-conformance.md`。
- **手段（必须真的量）**：`npm run dev` 起 vite → 在浏览器里打开 `http://localhost:5173/visual.html`（合成数据，不碰真配置）→ 跑 `.zcode/skills/design-conformance/scripts/in-page-audit.js`。**两套主题各自冷启动**（`?theme=light` / `?theme=dark`，等过渡走完再量）；至少覆盖 **960×640 最小窗口**；对比度用 `body` 口径（底栏在 `main` 之外）。
- **验的命题**：
  1. 明文规范条目 vs 实际渲染的**逐条符合性**：字阶（字号/行高成对）、间距阶、控件尺寸、弹窗档位、图标五档、几何四项（overlap / overflow / touching / clipped）、点击目标。给出实测数字，不是「看起来对」。
  2. **交互链路完整性**：每个入口是否闭合——入口 → 操作 → **反馈** → 出口。「应用」这类关键事务的结果是否可回看（历史审计判为「只有 5 秒 Toast」，`2026-09-20-product-review.md` §维度 5）。
  3. **「不做假开关」原则的落实**：核心层没实现的能力，界面**不得**看起来可选可用。逐项核：PDF/视频等不可执行能力是否可见但禁用且说明原因；Chat Completions 的「实验」标签是静态文案还是读核心结论；`已验证路由` 是否被摆成可用能力。
- **合格线**：几何/对比度/点击目标以脚本数字为准（例外项除外，见 §四）；交互链路每条给出「缺哪一环」；假开关**发现即 P1**。
- **证据形态**：审计脚本的原始输出 + 每处问题的 `selector` 与实测数值 + 涉及的规范条目号。
- **只能标未验证**：原生 macOS 窗口真实外观（Computer Use 不可用、System Events 超时）、读屏软件实际朗读、**200% 缩放**（只能按视口模拟，不等同 macOS 原生缩放）、Tab 顺序手工走查可做但需说明方法。

### 5.3 前端审核员

- **读**：`src/` 全量（`app/`、`features/`、`components/`、`contracts/`、`locales/`、`styles/tokens.css`、`dev/visual-fixture.tsx`）、`vite.config.ts`、`tsconfig.json`、`src/i18n.test.ts`。
- **验的命题**：
  1. **状态管理**：页面间共享状态（供应商、模型作用域、待应用、连接状态、主题、语言）的所有权是否清晰；有无「切换作用域后内容不跟随」这一类（历史 P0-1 已修，复核是否复发）。
  2. **i18n 双字典一致性**：`src/locales/zh-CN.ts` vs `en.ts` 键集合与占位符一致；`src/i18n.test.ts` 的守卫**覆盖面**是否够（只扫 `crates`？有没有漏 `src-tauri` 独有的 messageKey）。
  3. **a11y**：`src/app/accessibility.test.tsx` 覆盖外的缺口；正 tabindex、纯图标按钮缺名、`scope` 缺失、焦点管理（弹窗 Escape / 焦点回位）。
  4. **CSS Module 与全局样式的边界**：全局 `button`/`input` 规则与 CSS Module 权重打平的坑（历史已记录多次踩中，`2026-09-21-extension-pages-design-conformance.md` R1）。
  5. **错误处理 / 竞态 / 乐观更新**：异步命令失败是否有可见反馈；有无 fire-and-forget；乐观更新是否有回滚；卸载后 setState。
  6. **构建体积与分包**：`pnpm build` 产物里有无超大 chunk（如把 lucide 全量打入）；有无懒加载。
- **合格线**：每条给出 `文件:行` 与判定；i18n 一致性必须有可执行结论（跑守卫 + 抽查）；**发现的「核心未实现却可操作」的控件升级为 P1**。
- **证据形态**：`文件:行`、守卫输出、构建产物 chunk 表。
- **只能标未验证**：真实 WebView（WKWebView）运行时差异、真机上键盘/焦点行为。

### 5.4 后端审核员

- **读**：`crates/switch-core/src/`（`domain/`、`application/`、`codex/`、`gateway/`、`protocols/`、`credentials/`、`diagnostics/`、`storage/`、`content/`、`plugins/`、`toolhub/`）、`crates/bridge/`、`src-tauri/src/`、`crates/switch-core/tests/`、`crates/bridge/tests/`。
- **验的命题**：
  1. **域边界**：`switch-core` 是否真的不依赖窗口框架（`cargo tree` 可验）；bridge 的依赖方向。
  2. **配置写入的事务 / CAS / 原子替换**：并发写同一路径是否还安全、临时名是否唯一、`publish` 与 `write_atomic` 的先后与回滚（历史 P0-5 已修两洞，复核是否复发）。
  3. **凭据边界**：上游 Key 是否只进系统凭据库；`config.toml` 是否只出现本地 token；诊断包脱敏规则的洞（历史 P0-7 的 20–39 位无前缀密钥）是否已补。
  4. **网关鉴权与本地绑定**：是否只绑 `127.0.0.1`；是否拒绝带 `Origin` / 预检的请求；启动时是否换新 token（README `Security boundaries`）。
  5. **协议翻译的「损失记账」**：`chat.rs` 的 `losses` 是否**只有日志没有文案键**（历史 P1-14：用户看不到）；`reasoning.effort` 空集合时 chat 丢弃、responses 透传的自相矛盾是否仍在。
  6. **Bridge 的进程与路由正确性**：线程钉根、只带 threadId 的续接跟随线程、子进程错误不被打扮成成功、不留孤儿。
  7. **错误是否被伪装成成功**：Windows helper 现在是否 fail-fast（历史 P1-15 是「假装成功」）；网关失败是否一律写可读响应（历史 P0-2 是静默关连接）。
- **合格线**：每条给出 `文件:行` 与判定；**「失败被伪装成成功」或「密钥外泄路径」发现即 P0**。
- **证据形态**：`文件:行` + 相关测试名与断言 + 必要时的命令输出。**不得只复述历史结论**——必须标明是本轮新读出的还是复核确认的。
- **只能标未验证**：Windows 真机分支、钥匙串在他人机器上的行为、Intel Mac。

### 5.5 测试审核员

- **读**：`crates/switch-core/tests/`、`crates/bridge/tests/`、`src/**/*.test.ts(x)`、`tests/fixtures`、`tests/helpers`、`.github/workflows/ci.yml`、`docs/development/02-testing-and-release.md`。
- **验的命题**：
  1. **测试层次 vs 「改坏了会静默出错」的地方**：哪些地方坏掉不会被任何测试拦住（重点：上游参数遵从、模态虚报、路由身份、切换 Key 后的续接、Bridge 路由）。
  2. **AC-01~AC-17 的自动化覆盖情况**（与需求审核员口径对齐，此处给测试侧证据）。
  3. **缺口**：故障注入（journal 各阶段被杀、只读目录、Keychain 失败、磁盘满、hash 不匹配）覆盖到哪一步；协议一致性 fixture 是否覆盖 SSE 拆包/多事件/背压/取消；并发写损坏是否有回归。
  4. **CI 门禁够不够狠**：七个 job 各自拦得住什么、拦不住什么；`probes` job 只做 `node --check` 加依赖自检（语法级），**端到端探针不在 CI**——这是环境限制还是可补，给出判断。
  5. **「人工层」现在有没有人真的跑过**：`docs/development/02-testing-and-release.md` 要求发布前人工跑三个探针并留证据到 `docs/appendix/evidence-manifest.json`。核对 evidence-manifest 里**是否有 0.3.0 或 0.2.0 的真实执行痕迹**，没有就如实写「人工层无痕迹」。
- **合格线**：覆盖缺口必须**定位到具体行为**（不是「覆盖率不够」这种空话）；每条缺口写明「坏了会怎样、为什么现有门禁拦不住」。
- **证据形态**：测试名 + 断言摘要、CI job 定义 `文件:行`、evidence-manifest 的实际内容。
- **只能标未验证**：需要真 Codex / 真上游凭据才能跑的层。

### 5.6 营销方案审核员

前提：**仓库里没有营销方案**，本轮审的是「市场面叙事与转化路径」，并产出一份可落地的方案。口号不算产出。

- **读**：`README.md` / `README.zh-CN.md`、`docs/README.md`、`.github/workflows/release.yml` 的 Release 正文、`docs/research/01-reference-projects.md`、`docs/research/02-codex-feasibility.md`、`docs/audits/2026-09-20-product-review.md` §6（竞品差异化）、`docs/audits/2026-09-20-out-of-box-onboarding.md`。
- **验的命题**：
  1. **定位与说服力**：README 第一句（`README.md:5-9`）说对了「让第三方模型出现在 Codex 自己的选择器里」；但 `docs/README.md:3` 仍写「**尚未用真实第三方供应商验证过**」，与 `README.md:120-130` 的「已对真实供应商验证」**直接冲突**。逐处找出这类自述冲突并给修法。
  2. **命名一致性**：产品名 Switchelp vs 历史名 GPTSwitch；仓库名 `switchelp-macapp` vs bundle id 仍为 `app.gptswitch.desktop`（README 说明是有意保留）。评估对**新用户首次接触**造成的困惑程度，给出保留/收口的建议。
  3. **下载安装摩擦的量化**：0.2.0 已签名**未公证**，首次打开被 Gatekeeper 拦（`README.md:44-65`）。按目标人群（个人开发者、习惯 Homebrew/终端）估算流失量级，并与「公证后双击即开」的对照组比较；给出**降低摩擦的具体动作**（公证凭据、Release 正文首开指引已做，还缺什么）。
  4. **与参考项目的差异化是否讲清楚**：对照 `docs/research/01-reference-projects.md`，说明「唯一把第三方模型接进 Codex 原生选择器 + 凭据只进系统凭据库 + 写入可回滚的跨平台 MIT 工具」这条差异化**在 README / Release / 首屏里是否被人看得到**，以及「接进原生选择器未验证」这一事实对文案的约束。
  5. **交付一份可落地营销方案**：目标人群画像、渠道（如 Codex 社区、GitHub、HN、中文开发者社区）、首屏文案（中英）、信任要素（MIT、无遥测、不记正文、Key 只进钥匙串、可回滚）、以及**首发节奏**（0.3.0 发布时按什么顺序放出什么信息）。
- **合格线**：每一条判断有据（`文件:行` 或公开事实）；营销方案必须**具体到文案与渠道**，不许「加强宣传」这类口号；**不得声称未验证的能力**（尤其「模型出现在 Codex 菜单」在真机 GUI 未验前，文案必须留余地）。
- **证据形态**：`文件:行`、Release 正文原文、竞品对比表。
- **只能标未验证**：真实下载/转化数据（仓库内无公开 download 数据源时不得编造），用户测试。

---

## 六、我最担心的 5 件事（决定本轮审核优先级）

1. **「模型出现在 Codex 原生选择器」是全部价值，却连续多轮未验证，且没有 Plan B。** 如果它不成立或随 Codex 版本漂移，供应商/Key/能力/事务全部归零，产品退化成 config.toml 编辑器。**代价**：所有已投入的功能一次性贬值，且没有降级路径可以留住用户。→ 本轮要求设计与需求审核员把它列为**第一优先命题**。
2. **产品把自己介绍成「配置治理台」，而用户来这里的唯一动机是「让我的 Key 能用」。** 首屏不说价值主张、术语里漏着「目录/别名/实例/重新加载」、四态模型里有一态永远不可达。**代价**：目标用户 5 秒内判定「这不就是改 config.toml 吗」然后离开，转化在首屏就死掉。→ 营销与设计审核员必须给可执行文案与信息架构结论。
3. **在最需要确定性的三个点（应用结果 / 已加载 / 路由验证）都选了「轻」。** 应用结果只活 5 秒 Toast；「已加载」要用户自己宣布；「已验证路由」在界面里根本没有入口。**代价**：用户第一次应用后的不确定性直接决定他会不会用第二次——对一个以「可解释、可恢复」为卖点的工具，这是信任的反面。→ 设计审核员按 P1 处理。
4. **测试与门禁的覆盖缺口正好压在 P0 上，且「人工层」可能长期没人跑。** CI 七 job 拦得住语法、类型、CLI 侧管线与隐私扫描，但拦不住宿主 GUI 菜单、Bridge 界面层、真实上游质量、AC 里需要真机的条目。**代价**：这些正是「改坏了会静默出错」的地方，坏了没人知道，直到用户踩到。→ 测试审核员必须核对人工层在 evidence-manifest 里有没有真实痕迹。
5. **交付/转化路径上的硬摩擦（未公证 + 命名变迁 + 文档自述互相矛盾）会静默吃掉大部分冷启动流量。** 0.2.0 被 Gatekeeper 拦一次、`docs/README.md` 说「未验证」而主 README 说「已验证」、产品名/仓库名/包名三者不一致。**代价**：那些最该成为首批用户的个人开发者，在看到产品之前就走了。→ 营销审核员给量化与修法。

---

## 七、本轮明确不做的事

1. **任何源码或配置改动**。本轮只读、只写本文件与后续专项报告。修复是专项审核之后的事。
2. **Windows 实机验证**（本机无 Windows：安装、凭据 helper、Codex 路由、NSIS/MSI 全部标未验证）。
3. **macOS Intel / Windows ARM64 产物验证**（无机器、无产物）。
4. **公证与实际签名发布**（需要账号方的凭据，属发布执行，不在本轮审核范围）。
5. **200% 缩放的 macOS 原生验证**（只能按视口/`deviceScaleFactor` 模拟，必须在报告里标「模拟，不等同原生」）。
6. **读屏软件（VoiceOver）与原生窗口外观的真机验证**（Computer Use 不可用、System Events 超时）。
7. **PRD §6 性能与内存目标（p95、冷启动、内存预算）的实测**（无基准脚本；本轮不新建基准设施，只登记为「目标值，无数据」）。
8. **架构级重构**（信息架构 6→4 页收缩、四态模型收敛、降级形态预研等）。这些是产品决策，等本轮结论汇总后由产品总监排期，不在专项审核范围内执行。
9. **第三方依赖许可证的法务判断**（只做清单核对）。
10. **git 历史里的真实上游域名**（已推送至公开仓库，改文件无法消除，只作为已知事实记录）。

---

**章程结束。** 六位专项审核员按本文件 §五 的任务书执行，产出各自的报告；产品总监在汇总轮按 §四 的标准裁决，并与 `docs/audits/2026-09-20-audit-synthesis.md` 的历史条目做一一对照。
