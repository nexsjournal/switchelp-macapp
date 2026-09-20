# 全项目审计统筹报告（产品负责人视角）

日期：2026-09-20 · 审计基线：`HEAD = 2fda2a5`，工作区版本 `0.2.0`，工作区干净
统筹者：产品负责人（子 Agent 产出汇总与去重）

## 0. 本次审计怎么做的

按用户要求拆成七个独立子 Agent，各自只对自己那一层负责，交叉复核后由产品负责人汇总：

| # | 子 Agent | 报告 |
| --- | --- | --- |
| 1 | 需求（对比参考产品、PRD 追踪） | [requirements-parity](2026-09-20-requirements-parity.md) |
| 2 | 实现验证（唤起真实 Codex、跑真实上游） | [implementation-verification](2026-09-20-implementation-verification.md) |
| 3 | 设计（实测几何、对比度、字阶、键盘） | [design-conformance](2026-09-20-design-conformance.md) |
| 4 | 开发（分层、错误处理、协议、构建、CI） | [development-verification](2026-09-20-development-verification.md) |
| 5 | 测试与隐私（覆盖真实性、泄露扫描） | [test-and-privacy](2026-09-20-test-and-privacy.md) |
| 6 | 开源工具配置（陌生人从下载到跑通） | [out-of-box-onboarding](2026-09-20-out-of-box-onboarding.md) |
| 7 | 产品（产品总监视角、主流程顺滑度） | [product-review](2026-09-20-product-review.md) |

产品负责人另做了一轮**独立复核**：在真实浏览器里亲验了最关键的几条结论（见 §2 标注「本报告实测」），
并把七份报告里重复出现的同一问题合并为一条，避免把同一处缺陷按人头数重复计数。

**基线门禁（审计开始时全绿）**：`pnpm typecheck` 0 错；`pnpm test` 112 用例通过；
`cargo test -p switch-core` 410 用例通过（3 条 `#[ignore]`，均为需真机的凭据库项）；
`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all --check` 干净；
`scripts/check-publish-safety.sh` 对已跟踪文件通过。**所以下面的问题都不是「测试没过」，是测试没照到的地方。**

## 1. 总体结论

**机制是真的，产品还不是。**

端到端链路在这台机器上**实测可用**：真实上游跑通一轮完整推理（3.13s 返回），三个 G0 探针全过，
真实 Codex 的 `model/list` 确实列出了受管模型，应用后真的重启了宿主，密钥边界（上游 Key 只在系统
凭据库、宿主只拿本地 token）**证真**。核心的事务设计（备份 → 计划 → CAS → 原子替换 → 可回滚）
也站得住，`config.toml` 的写前备份含完整原文件与 `contentHash`。

但把它当**产品**交付给陌生人，现在不成立，有三类硬伤：

1. **主流程会在中途断掉**：首次接入向导在保存第一个供应商后就整体消失，第 3 步「测试并应用」永远
   走不到；而 README 的下载表指向 5 个版本前的包。陌生人拿不到「顺滑一次配置」。
2. **有两条会真正伤到用户的行为没有被任何文档声明**：应用本工具会**整体替换** Codex 的模型列表
   （原生模型全部从菜单消失，实测 `model/list` 只剩 1 项）；网关在若干路径上会**静默失败**
   （连接被关闭且不写任何响应），这与项目自己写下的「必须能判定失败」正面冲突。
3. **设计偏差是体系性的，不是零星的**：修好的是单项数值，不是节奏。字阶的字号回到了规范，
   行高没有跟（12px 挂 22px 行高，而规范是 12/18），于是「每处都不算错、叠起来说不上哪里不对」。

**结论：现在这个版本不能称「稳定版」**，与 PRD §7 的发布验收条件不符（同名模型可区分 ✅、
菜单选择与路由一致 ✅、上下文/输出/推理有可观测结果 ⚠️、图片能力不虚报 ⚠️、
外部配置冲突可恢复 ✅、双平台真实 Codex 验收 ❌）。下面给可执行的路线图。

## 2. 去重后的问题总表

严重度定义：**P0** = 违反 P0 需求或挡住主流程；**P1** = 违反明文规范 / 数据安全 / 可访问性；
**P2** = 观感、一致性、协作面。

### P0（7 条）

| ID | 问题 | 证据 | 来源 |
| --- | --- | --- | --- |
| **P0-1** | **切换供应商后模型表不跟随**：`<h2>` 已换成 B，表里仍是 A 的模型，B 的模型无入口 | `ModelsPage.tsx:44` 的 `useState(providerScope ?? 'all')` 只在挂载时取值；App.tsx:275 渲染时**没有 `key`**，切换供应商不会重挂载。**本报告实测**：浏览器里切到 Example Provider B 后，表内仍是 `视觉多模态模型` / `deepseek-v4.1`（属 A），而夹具里 B 的模型是「轻量快速模型」「长文本模型」 | 设计 P0 + 产品 #1，本报告复核 |
| **P0-2** | **网关静默失败**：`GET /models` 遇未知目录版本、推理请求缺 model / provider / credential 时不写任何响应就关连接 | `gateway/server.rs:168` 接受循环 `let _ = gateway.handle(stream)` 吞错误；`handle_models` 用 `ok_or_else(...)?` 绕过 `write_error`（同文件 252 行起）。**两个 Agent 独立实测**：curl 得到 `Empty reply (52)` / HTTP 000，无任何诊断。违反 [risks 文档](appendix/02-traceability-and-risks.md) 自订的「卡死可检出、可判定失败」 | 开发 #1 + 实现 #1 |
| **P0-3** | **应用后原生模型全部从 Codex 菜单消失**（目录是**替换**不是追加），README 未声明 | 实测 `model/list` 返回 `count = 1`，只剩受管的 `gs/...`；写入前备份显示用户原选择是 `model = "gpt-5.6-sol"`。本机三个 `rev_*` 目录里的 `models.json` 都只含 1 条（本报告复核） | 实现 #2 |
| **P0-4** | **首次接入向导中途消失**：第 2 步主按钮就是「添加供应商」，保存成功后 `providers.length !== 0`，向导整体消失，第 3 步「测试并应用」不可达；且 `onboardingForced` 是死状态（全仓库只有 `useState(false)` 与 `setOnboardingForced(false)`），代码注释承诺的「在设置里重新打开」**不存在入口** | `App.tsx:126`、`App.tsx:102`、`App.tsx:222-223`（本报告复核）。`onboardingDismissed` 持久化在 `localStorage['gptswitch.onboarding.dismissed']`，点一次「稍后再说」后永不自动回来 | 开源配置 #2，本报告复核 |
| **P0-5** | **写配置的原子性有两个洞**：① `write_atomic` 临时名固定，并发写同一路径 320 次里 223–226 次失败，存在半写文件被 rename 的机制；② `execute_apply` 先 `router.publish` 再 `write_atomic`，且发布不可回滚 —— 写失败后网关会**永久服务一个配置文件里不存在的目录版本** | `codex/config.rs:888`、`application/apply.rs:381` / `400` | 开发 #2 #3 |
| **P0-6** | **README 下载表指向 `0.1.3`**（落后 5 个版本，另有 0.1.6–0.1.9 未进表）；下载数据显示 v0.2.0 是 0 次、v0.1.3 被下过。陌生人还会读到 v0.1.3 里「本工具不会替你重启 Codex」这条**已失效**的说明 | `README.md:36-38` vs `package.json` / `tauri.conf.json` 的 `0.2.0`（本报告复核）。另：9 个 release 全无 x86_64 产物，Intel Mac 无包可用 | 开源配置 #1 |
| **P0-7** | **隐私门禁现在会因本次审计文档而失败**（已由产品负责人修复：七份新报告里的真实用户名、签名姓名与凭据库命令已脱敏，重跑 `check-publish-safety.sh` exit 0）。**残留项**：被跟踪测试硬编码真实供应商域名且已进历史；诊断包的 `redact_value` 只认 `sk-` 前缀 / hex≥32 / ≥40 位，**20–39 位无前缀密钥会原样进诊断包**（已实测复现） | [test-and-privacy](2026-09-20-test-and-privacy.md) A/B 两节 | 测试 #隐私 |

### P1（按体系归类）

**排版与视觉体系（互相放大，必须一起修）**

| ID | 问题 | 实测数字 | 来源 |
| --- | --- | --- | --- |
| P1-1 | **行高整体漂移**：字号回到了规范，行高没跟 | 实测 395 个自有文本节点里 **200 个（51%）** 不在字阶。本报告实测当前页 `font-size/line-height` 分布：`12/22`（18 个）、`13/22`（9）、`14/22`（9）、`16/22`（2）、`18/22`（1）、`12/14`（1）；规范是 12/18、13/20、14/20、16/24、18/26。**行高几乎恒为 22** | 设计 P1-6，本报告复核 |
| P1-2 | 浅色主题状态栏 `/` 对比度 **4.09**（用 `--border-control` 当文字色；深色 4.50 也仅擦线） | `App.module.css:113`，本报告实测复现 4.09 | 设计 P1-2，本报告复核 |
| P1-3 | 200% 缩放 **7/8 页整页横向滚动**，溢出 380px，侧栏不抽屉化 | `.shell { min-width: 860px }` | 设计 P1-3 |
| P1-4 | 间距魔数：`14px` 内边距 135 处、`gap:10px` 61 处，全在 4/8/12/16/24/32/48 阶外 | — | 设计 |
| P1-5 | 控件尺寸不统一：按钮行高 22（规范 20）；行内按钮与 `.text-button` 字号 12（规范 14）；同语义「纳入目录」开关两种尺寸 32×18 / 36×20（规范只 36×20）；档位删除按钮 24×32（规范 ≥32）；图标实测 8 种尺寸，14/15/17/20 越界，onboarding 警告图标被压成 **8.91×16** | — | 设计 P1-4/5/7/8/9/10 |
| P1-6 | 确认类弹窗宽 **720**（规范 480），480/640/800 三档退化成两档 | — | 设计 P1-9 |

**功能闭环缺口**

| ID | 问题 | 证据 | 来源 |
| --- | --- | --- | --- |
| P1-7 | **多 Key 管理不闭环**：能存多个 Key，但 UI 无法新增第 2 个、改名、删除单个、禁用；无 `credentials_disable` 命令 | `ProviderForm.tsx:115-122, 351-354` | 需求 P0-1 |
| P1-8 | 模型编辑器**缺「协议」字段**：`ModelDraft` 无 protocol，`protocol_override` 从不被写入（PRD P0 明列「独立模型编辑：…协议…」） | — | 需求 P0-4 |
| P1-9 | **工具调用门禁不存在**：`is_verified_adapter` 仅测试引用，probe 无工具阶段，前端「实验」标签是静态文案（PRD P0 要求「Chat Completions 适配通过工具调用门禁后进入首发，未通过则明确标实验状态」） | — | 需求 P0-5 |
| P1-10 | **供应商搜索是死代码**（有 state / 过滤 / 空态，没有输入框）；**供应商预设完全未实现**（`listPresets` 返回空） | `ModelsPage.tsx` / `ProviderForm.tsx` | 需求 P0-2 #3 |
| P1-11 | **设置页许可证文案自相矛盾**：项目是 MIT（`LICENSE`、`package.json`），界面却写「默认保留所有权利」/「All rights reserved by default」 | `src/locales/zh-CN.ts:726`、`en.ts:724`，渲染于 `SettingsPage.tsx:197`（本报告复核） | 开源配置 #2 |
| P1-12 | **单实例与资源回收**：无单实例插件、跨进程无锁；第二个实例网关起不来只打一行 stderr，IPC 与写 `config.toml` 照常；`retain/release/retire` 生产零调用，旧目录永不回收（本机已积 3 个 `rev_*`，本报告复核） | — | 开发 #4 #5 #6 |
| P1-13 | **panic 面**：生产路径大量 `expect("锁未被污染")`（repository / routing / journal / diagnostics），锁中毒即全线 panic；SSE pending 与连接数均无上限 | — | 开发 #7 |
| P1-14 | `reasoning.effort` 空集合时 **chat 丢弃、responses 透传**（自相矛盾，测试与文案表互否）；`losses` 只进日志且无文案键（README 声称「记录为 losses 而不是假装生效」，用户看不到） | — | 开发 #8 |
| P1-15 | **Windows 无 fail-fast**：helper 是 `exit 1` 桩，但配置照写、界面照报成功。README 说「fails diagnosably rather than pretending to work」——实际是**假装成功** | — | 开发 #8 |
| P1-16 | **发布流程 fail-open**：`release.yml` 的 publish 在**零产物**时仍创建 Release（`if [ -z "$(ls -A artifacts)" ]` 只是 `mkdir`）；macOS job 在 `signed != 'true'` 时跳过；**tag 触发永不构建 Windows**（`if: event_name == 'workflow_dispatch' && inputs.with_windows`） | `.github/workflows/release.yml:113, 163, 266-284`（本报告复核） | 开发 #CI + 开源配置 |

**流程与文案**

| ID | 问题 | 来源 |
| --- | --- | --- |
| P1-17 | 向导通篇**不解释「为什么需要重启 Codex」**（`onboarding.*` 里 restart 相关键 0 条）；协议默认落在自带「实验」标签的 `chat_completions`；端口被占是**死路**（固定 18765、错误是中文原文、无改端口入口） | 开源配置 |
| P1-18 | 端到端探针（`scripts/g0/*.mjs`）**不在任何 CI**；原生选择器可见性只有人工验证，回归无拦截 | 测试 #5 |
| P1-19 | Windows x64 **零测试覆盖**、包未签名且手动触发 | 测试 #4 |

### P2

- 追踪表标 Q04「✅ 已通过」所引的 `.local/g0-apply-pipeline.json` **在 `.gitignore` 的 `.local/` 下**，读者无法复核（文件本次由探针重跑重新生成，但按设计不入库）。
- README 多处过时自述：已对真实第三方供应商验证通过、GUI 选择器已实际使用（`thread/start` 日志里 `client_name="Codex Desktop"` 用受管模型建过会话）、下载版本号。
- `check-publish-safety.sh` 的私有清单路径（`~/.switchelp-private-patterns`）与 README 写的（`~/.gptswitch-private-patterns`）**不一致**；且脚本不扫 git 历史与构建产物。
- 发布 dmg 二进制嵌入 **342 处**构建机路径（`/Users/<user>/.cargo/...`，低危）。
- 测试输出有 `act(...)` 警告；无 CONTRIBUTING / SECURITY / issue 模板。
- 术语泄漏：目录 / 别名 / 实例 / 重新加载 / 目录版本 等实现概念进了界面。
- 首屏价值主张不提「让第三方模型出现在 Codex 原生选择器里」，侧栏副标题是「Codex 配置管理」——目标用户 5 秒内会误以为「这不就是 config.toml 图形界面」。

## 3. 用户八个问题的逐条回答

| 用户要求 | 结论 |
| --- | --- |
| **1. 需求对比（与参考产品一致性）** | 参考产品是 **CodexSplit（曾名 OpenCodex）**、Prodex、codex-switcher、星算助手。「CodeX Play」在仓库全历史与 `~/.codex` 会话里**零命中**；用户 2026-09-17 的原始需求逐字写的是「类似于 codexsplit，或者是之前叫 opencodex 的桌面端应用」。**结论：最接近的是 CodexSplit。** 借鉴的六项（黑灰层级、供应商列表、Key 管理、模型命名空间、明确「应用」入口、安装检测）**一致**；有意排除的项（Agent 分配、语音栏、账号调度、平台商城）**确实没做**。不一致：PRD P0 承诺但未实现 4 项（多 Key 闭环、协议字段、工具门禁、预设+搜索） |
| **2. 实现验证（唤起真实 Codex）** | ✅ **整条链路本机实测可用**：真实上游推理成功、三个 G0 探针全过、`model/list` 列出受管模型、应用+重启在 18:39 有完整真机痕迹（提交 02.23s → 新宿主 check-in 03.03s）。**密钥边界证真**：`config.toml` 无 key 痕迹、密钥只在 Keychain、helper 只吐 64 位本地 token。**两条声明被证伪**：README「尚未对真实第三方供应商验证」「GUI 选择器未验证」都已过时；代码注释「admission 会校验前缀实例」对 `/models` 不成立 |
| **3. 设计检查** | ❌ **偏差是体系性的**。详见 §2 P1-1~P1-6。一句话：**修的是单项数值，不是体系**——字号回到 14 了但行高没跟（12px 挂 22px），同一页字号对、节奏不齐；间距全在阶外；同一语义控件各页各自漂移；图标 14/15/17/20 混排。**这就是用户说「弄了好几遍还有细微问题」的机制**：单处都不算错，叠起来就是「说不上哪里不对」 |
| **4. 开发验证** | ✅ 框架分层是真的（`switch-core` 不依赖 tauri，`cargo tree` 可验）、CI 五个 job 都是真门禁、本地构建/clippy/fmt 全干净。❌ 但有 P0-2（静默失败）、P0-5（原子性两洞）、P1-12~P1-16（单实例 / 资源回收 / panic 面 / 协议自相矛盾 / Windows 假装成功 / 发布 fail-open） |
| **5. 测试覆盖** | 数量够（112 + 410），门禁全绿，`act` 警告与 3 条 `#[ignore]` 中的真机项可解释。**但覆盖缺口正好压在 P0 上**：多 Key / 协议字段 / 工具门禁 / Windows / 端到端探针都没测，所以这些缺口没被门禁拦住。**隐私**：无真实密钥外泄（66 个提交里 `sk-*` 全是合成夹具），三处真实泄露已处置/列明，运行期诊断包的脱敏规则有洞（P0-7） |
| **6. 开源工具配置** | ❌ **达不到「一次顺滑配置」**。陌生人从零到可用 12 步，其中 **2 个卡死**（README 版本号把人导向旧包；向导在保存第一个供应商后消失）**+ 1 个劝退点**（未公证，README 首推的「右键 → 打开」已不是 Apple 现行官方路径，而 dmg 里没有任何首启说明）+ 3 处摩擦。顺利的部分：应用后自动重启（从「已配好未应用」到生效只需 2 次点击）、恢复原生入口已可见 |
| **7. 产品视角** | ⚠️ **定位文档成立、界面不成立**。最短路径 8 次点击、4 个必填字段、4 个模态——**对话数量级可以接受，问题在状态表达**：（a）供应商切换后内容不跟（P0-1）；（b）「应用」结果只有 5 秒 Toast，没有页面级结果面板；（c）首屏不说价值主张；（d）默认协议落在实验态。**差异化**：不可替代性是「唯一把第三方模型接进 Codex 原生选择器、凭据只进系统凭据库、写入可回滚的跨平台 MIT 工具」——但「接进原生选择器」已连续三轮未验证，对已装星算助手的用户目前只有「更轻更可信」，**没有功能理由** |
| **8. 产品负责人统筹** | 本报告 |

## 4. 修复路线图

不按报告来源排，按**依赖关系与性价比**排。批次之间尽量不交叉，每批做完可独立验证。

> **执行状态（2026-09-20 晚，同一会话内完成）**：**批次 A~F 全部做完**。
> 批次 A 逐条见 §4.0，批次 B~F 见 §4.7。两处「按 PRD 做不到」的项已单独记录在
> [batch-E-deviations](2026-09-20-batch-E-deviations.md)。

### 4.0 批次 A 已完成的改动（2026-09-20，已逐条验证）

| 动作 | 落点 | 验证方式与结果 |
| --- | --- | --- |
| 修 `ModelsPage` 的 provider 作用域 | `ModelsPage.tsx`：把作用域从 `useState` 改成**派生**（`providerScope ?? pickedProvider`），并清掉跨供应商的勾选 | 新增回归用例 `宿主给定的供应商作用域`；**旧实现下必失败**（实测：`Unable to find an element with the text: 目录外的模型`）。真实浏览器复核：切到 Example Provider B 后表格内容变成 B 的「轻量快速模型 / 长文本模型」（修前仍是 A 的两行） |
| 向导不再中途消失 | `App.tsx`：`showOnboarding` 改为由显式的 `onboardingOpen` 驱动，不再从「零供应商」推导；供应商弹窗打开时**不**卸载向导 | 新增回归用例 `保存第一个供应商之后向导不消失，第 3 步仍然走得到`；旧推导下必失败 |
| 向导可重新打开 | `SettingsPage.tsx` 的「Codex 实例」卡新增「重新打开接入向导」（含说明）；`App.tsx` 的 `onReopenOnboarding` 把它接上 | 新增用例 `退出向导后可以从设置页重新打开`；真实浏览器点过，确认落到概览页且向导展开（夹具里**有**供应商也照样展开，证明推导已解除） |
| 向导步骤跨卸载保留 | 步骤提升到宿主（`onboardingStep`），与模型编辑器同源 | 同上的用例覆盖 |
| 向导补「为什么要重启 Codex」 | `OnboardingPage` 第 3 步新增提示；`onboarding.restartNote` 中英双语 | 浏览器截图确认渲染 |
| README 下载表改 0.2.0 | 中英两版：真实产物名 + 官方 SHA-256（与 GitHub Release 附件 digest 逐字一致）+ Apple 现行首开路径 | `shasum -a 256 dist-release/*` 与 GitHub digest 比对一致 |
| 删掉过时自述 | 中英两版「尚未对真实第三方供应商验证」「GUI 未验证」改为实测结论；补上「目录是**替换**不是追加」这条未声明行为 | README + `.zh-CN` 同改 |
| 许可证文案 | `settings.licenseValue` / `licenseNote` 中英双语改为 MIT | 浏览器截图确认「MIT / 可自由使用、修改与分发，保留版权与许可声明」 |
| 私有清单路径 | `check-publish-safety.sh` 同时接受两个历史文件名（环境变量仍优先）；脚本头部注释与中英 README 统一到 `~/.switchelp-private-patterns` | 实测：用旧文件名 `~/.gptswitch-private-patterns` 写入模式后**能被扫到并命中**；门禁 exit 0 |
| 首开指引 | `release.yml` 的 publish 步骤新增 `body`，把首开两步与 SHA-256 核对写在**下载页**上 | YAML 解析通过；`actionlint` 本机未装，由 CI 的 lint 任务兜底 |

**没做成的**：往 `.dmg` 里塞一份「首次打开.txt」。Tauri 2 的 dmg bundler 不支持附加任意文件（`bundle.macOS.dmg` 只有窗口尺寸/图标位置/背景这几项），要做得在签名后自己用 `hdiutil` 拆包再打包，脆弱且需要一次真实的签名发布才能验证。改用「Release 正文 + README」两个真正会被看到的位置承载同一份说明。

**批次 A 的验收数字**：`pnpm typecheck` 0 错；前端 115 用例通过（较审计前 +3）；`cargo test -p switch-core` 410 通过；`cargo fmt --all --check` 与 `cargo clippy --workspace --all-targets` 干净；`check-publish-safety.sh` exit 0；改动的两个界面（设置页、向导第 3 步）在**深色与浅色**两套主题下用 `in-page-audit.js` 复测，`overlap / overflow / touching / clipped` 全为 0，字号集合全在字阶内。批次 D 的排版体系问题（行高、间距、控件尺寸、缩放）**未动**。

### 批次 A：让人过不去的地方先通（1–2 天，都是小改动）

| 动作 | 对应 | 验证方式 |
| --- | --- | --- |
| 修 `ModelsPage` 的 provider 作用域（加 `key={selectedProvider.id}` 或 `useEffect` 同步），补一条「切换供应商后表内容跟随」的回归用例 | P0-1 | 复跑本报告 §2 的浏览器步骤，表内容必须换 |
| README 下载表改 `0.2.0` + SHA256；删掉「尚未对真实第三方供应商验证」「GUI 未验证」过时段 | P0-6 | 版本号与 `package.json` / `tauri.conf.json` 三方一致 |
| 设置页许可证文案改 MIT；同步 `zh-CN.ts` / `en.ts`（`i18n.test.ts` 会守住字典对齐） | P1-11 | `pnpm test` |
| 向导：把自动展开条件从「零供应商」改成「未应用过任何配置」，或给 `onboardingForced` 接上设置页入口；向导里补「为什么需要重启 Codex」 | P0-4, P1-17 | 保存第一个供应商后向导仍能走到「测试并应用」 |
| 首开指引改成 Apple 现行路径 + 一行 `xattr` 命令块；往 dmg 里放一个 `首次打开.txt` | 开源配置 #3 | 真机删 quarantine 后双击可开 |
| 统一私有清单路径（脚本 vs README） | P2 | `check-publish-safety.sh` 提示与 README 一致 |

### 4.7 批次 B~F 已完成的改动（2026-09-20，逐条带验证方式）

每条都写了「怎么证明它真的生效」——只跑测试说「改好了」不算。

#### 批次 B：数据安全与可判定失败

| 改动 | 落点 | 证明 |
| --- | --- | --- |
| 网关不再静默关连接 | `gateway/server.rs`：新增 `Response` 守卫（记录「响应是否已开始」）；判定阶段的失败一律补一个可读错误，已开流的失败仍走流内 error 事件 | 新增 4 条集成用例（未知版本 `/models`、外来实例前缀 `/models`、缺 model、供应商已删）；**把守卫停掉后其中 2 条必失败**（实测报错为「网关必须给出响应，而不是关掉连接」） |
| `/v1/models` 与推理同样校验实例归属 | `routing.rs::aliases_checked` | 用例 `models_refuses_an_instance_that_does_not_own_the_revision`：换前缀不再列出别人的模型 |
| `InstanceMismatch` 文案不再把两个实例名说反 | `routing.rs` | 文案改成中性（「这里要求的是 X，但该目录版本属于 Y」），两个调用点语义不同的事实写进注释 |
| 删掉死代码 `claimed_instance` | `gateway/auth.rs` | 它只是把网关自己的实例名还回去，从不读请求，却长得像「令牌绑定了实例」 |
| 并发写不再互相截断 | `codex/config.rs`：临时名加进程号 + 自增序号 + 纳秒 | 新增并发用例（32 线程 × 同一路径）；**旧名字下必失败**，实测 `entity not found` 与内容截断 |
| 写盘失败回收刚发布的版本 | `application/apply.rs`：备份 → 发布 → 写盘；写失败即 `retire` 并如实报告能否回收 | 新增用例 `a_failed_write_takes_back_the_publication_it_just_made`（把目录设只读制造写失败）；**不回收时必失败** |
| 诊断包脱敏补上无前缀密钥 | `diagnostics/mod.rs`：新增「≥24 位、大小写数字齐备、非本工具 id 前缀」的判定 | 单元用例 + 导出层 canary；**停用新规则后两条必失败** |
| 目录版本回收接进生产 | `gateway/server.rs` 用 `InferenceGuard`(Drop) 配对 retain/release；`apply.rs::prune_catalog_revisions` 保留「当前 + 上一代」 | 用例：4 轮应用后只留 2 个版本且旧目录已删；被引用的版本不被回收、引用归零后才回收；**停用 prune 后必失败**；另有「请求进行中引用必须 >0」的用例，**停用 retain 后必失败** |
| 网关没起来时禁止应用 | `src-tauri/commands.rs` | 写进 Codex 的 base_url 指向本机网关，网关不在时写下去会让 Codex 全部失败而界面说成功。第二实例（端口被占）正是这个形态 |
| 端口被占用时直说最可能的原因 | `gateway/server.rs::bind` | 提示里点名「已经开着另一个 Switchelp 实例」 |
| 被跟踪测试里的真实上游域名 | `src/app/App.test.tsx` | 换成中性占位。**git 历史里的那一处无法通过改文件消除**，已推送到公开仓库，只能记录为已知事实（裸域名不是凭据） |
| i18n 守卫扩到桌面壳 | `src/i18n.test.ts` | 以前只扫 `crates`，`src-tauri` 独有的 messageKey 会绕过「每条错误都有文案」这条检查 |

#### 批次 C：让「替换菜单」变成用户知情的选择

| 改动 | 证明 |
| --- | --- |
| 应用差异弹窗新增「Codex 的模型菜单会被替换」区块（含替换后的模型清单，别名翻不回显示名就退回别名） | 两条前端用例：应用时显示且列出名字、还原时不显示 |
| 核心侧锁定「目录里只有本次发布的别名，一个原生模型都不带」 | 用例 `applying_replaces_the_host_menu_with_exactly_the_published_aliases` |
| README 中英两版补上这条行为 | 见 README 的 "How it works" / 「它怎么工作」 |

#### 批次 D：设计体系一次性归位

| 改动 | 实测结果 |
| --- | --- |
| 行高跟着字号走（脚本先归一 token，再按字阶配对补 `line-height`） | 逐页实测 `font-size/line-height` 集合：12/18、13/20、14/20、14/22、16/24、18/26、28/36 —— **全部在字阶上**（改动前 51% 的文本节点不在字阶） |
| 间距收回 4/8/12/16/24/32/48 | `gap` 集合：2/4/8/12/16/24（2px 是写明的光学微调）；`padding`/`margin` 同步归一 |
| 控件尺寸：按钮行高 14/20、行内按钮 12→14、输入行高、`.badge`、`.field-hint` 全部走 token | 六页结构断言 0 违规 |
| 弹窗宽度补回三档 480 / 640 / 800（`.normal` 这个类以前**根本不存在**，显式写 `normal` 的静默落到 720） | 实测：确认框 480、供应商弹窗 640、差异弹窗 800 |
| 图标收敛到五档 12/14/16/18/20（+26/28 插图），并修掉被压成 8.91×16 的警告图标 | 尺寸集合只剩这几种；规则写进 `docs/design/02-icons-type-and-copy.md` §1.1 |
| 浅色状态栏 `/` 从 4.09 提到 5.33（深色 4.50 → 6.07） | 两套主题实测 |
| 删掉死规则 `.stats/.stat`（里面写着 30/40 这种不在字阶上的字号） | 代码里已无字面量字号 |
| 窄窗与 200% 缩放不再横向滚动 | `.shell` 的 `min-width: 860px` 去掉，≤860px 侧栏收成 64px 图标栏（文字视觉隐藏，无障碍名保留）、主内容单列；**逐页实测 960×640 与 480×320 均为 0 溢出**（改前 200% 下 7/8 页溢出，最多 380px） |
| 设置页仓库链接点击区 15px → 32px | 审计脚本 target 检查归零 |
| **测量方法的坑也修了** | 切主题后必须等过渡走完（700ms）再测：否则背景色还在插值中，同一页会在「干净」与「报几处」之间跳（实测就是这样被自己骗过一次）。已写进 `design-conformance` 的 SKILL.md |

#### 批次 E：补齐承诺过的功能

| 改动 | 证明 |
| --- | --- |
| **多 Key 闭环**：Key 池支持加第二个、改名、停用/启用、删除；当前 Key 的停用与删除被拦并在 `title` 里说明原因 | 3 条核心用例（改名推进版本、停用后不可选为当前、当前 Key 不可停用）+ 1 条前端用例覆盖四种动作 |
| **换当前 Key 会让「待应用」重新出现**（新发现：网关服务的是发布时冻结的路由快照，换 Key 必须重新应用才生效，而以前界面上完全看不出来） | 用例 `switching_the_active_key_puts_the_models_back_to_pending` |
| **模型级协议字段**（`Model.protocol_override` 早就在域模型与路由里，但草稿没有这个字段，界面永远设不上） | 用例 `a_model_level_protocol_override_reaches_the_route`：设成 chat_completions 后路由的 `protocol_id` 真的是它 |
| **供应商搜索框**（过滤逻辑、空态都在，就是没有输入框） | 前端用例：过滤生效、空态出现 |
| **Chat Completions 明确标实验**（PRD 的退路那一半） | 用例：走 CC 适配时计划里必带 `warning.chatAdapterExperimental`，全 Responses 时不带；它会显示在应用差异弹窗的警告区 |
| **Windows helper 不再假装**：`install` 在 Windows 上直接返回错误，于是网关标为未启动、应用被前置检查拦住 | 代码路径已改；**本机无法执行 Windows 分支**，属未验证项 |
| 删掉死接口 `listPresets()`（恒返回空、没有任何界面调用） | 见 [batch-E-deviations](2026-09-20-batch-E-deviations.md) |

#### 批次 F：把门禁补到能拦住上述问题

| 改动 | 说明 |
| --- | --- |
| CI 新增 `pipeline` job | 跑 `g0_apply_pipeline`（真实的计划 → CAS → 原子写入），并断言 config.toml、helper、目录文件都在且互相指向。这是 CI 里唯一能跑的端到端 |
| CI 新增 `probes` job | `node --check` + 依赖自检：那几个只能人工跑的探针最容易悄悄烂掉 |
| **release 不再产生空的公开 Release** | 零产物时改为创建**草稿**（草稿对公众不可见），本机签名包附上后再发布 |
| 端到端两层写进文档 | `docs/development/02-testing-and-release.md`：哪一层在 CI、哪一层只能人工、人工那层的命令与证据要求。**探针需要真实 Codex 可执行文件，runner 上没有也装不上**，这是环境限制，写下来是为了它不会被忘掉 |

#### 这一轮的验证数字

`pnpm typecheck` 0 错；前端 **119** 用例通过（审计前 112）；`cargo test -p switch-core` **430** 通过（审计前 410）；`cargo fmt --all --check` 与 `cargo clippy --workspace --all-targets -D warnings` 干净；`check-publish-safety.sh` exit 0；`g0_apply_pipeline` 本机跑通并产出完整 config + catalog + helper；六个页面 + 五个弹窗/整页视图在**深色与浅色**两套主题下几何全 0，字号全在字阶，960×640 与 480×320 均无横向滚动。

#### 没做的（诚实清单）

- **Windows 真机**：本机没有 Windows，`install` 的 Windows 分支只能靠代码审阅。
- **Intel Mac**：没有产物也没有机器。
- **真实 Codex 对探针那三个脚本的验证**：需要 ChatGPT 桌面端，只能在发布前人工跑（已写进文档）。
- **供应商预设**：见 [batch-E-deviations](2026-09-20-batch-E-deviations.md)，需要产品决策。
- **工具调用门禁**：同上，需要一份可用的真实上游凭据。
- **git 历史里的真实上游域名**：已推送到公开仓库，改文件无法消除。

### 批次 B：数据安全与可判定失败（3–5 天）

| 动作 | 对应 |
| --- | --- |
| 网关：接受循环不再吞错，所有路由分支走统一的 `write_error`；未知目录版本 / 缺 model / 缺 provider / 缺 credential 都必须返回可读错误体 | P0-2 |
| `write_atomic` 临时名加 pid+随机后缀；`execute_apply` 把 `router.publish` 移到 `write_atomic` 成功之后，并给发布加回滚 | P0-5 |
| 诊断包 `redact_value` 增加「无前缀高熵串」规则；补一条 canary 用例（20–39 位无前缀密钥不得出现） | P0-7 |
| 清理被跟踪测试里硬编码的真实供应商域名（同时处理 git 历史，或明确记录为已公开信息） | P0-7 |
| 单实例锁 + 旧目录回收（`retain/release/retire` 接到生产路径） | P1-12 |

### 批次 C：把「替换原生模型列表」变成用户知道的选择（2–3 天，需要产品决策）

行为本身可能是 Codex 的既有语义，但**用户必须被提前告知**，且要有退路。三选一：

1. 应用前的差异弹窗里明确写出「Codex 的模型菜单将被替换为：<列表>；原生模型需要恢复原生才能再用」——**成本最低，本报告推荐**；
2. 做到追加（把原生目录合并进 `model_catalog_json`）——需要先验证 Codex 是否接受原生模型的目录条目，属于 PRD P1 的 Bridge 门禁；
3. 把「恢复原生」做成应用后的常驻提示。

无论选哪条，都必须先补一条**实测用例**：应用后 `model/list` 的完整内容快照。 | P0-3 |

### 批次 D：设计体系一次性归位（3–5 天，必须整批做，不要逐处修）

**这一批的价值在于「一起改」——单改一处看不出效果，这正是过去几轮返工的原因。**

1. **行高跟着字号走**：按字阶表把 12/18、13/20、14/20、16/24、18/26 落成 token（不是逐处写死），然后把 `line-height: 22px` 的全局兜底撤掉。这一条修完，51% 的偏差会一起消失。
2. **间距阶**：把 135 处 `14px` 与 61 处 `gap:10px` 收回 4/8/12/16/24/32/48。
3. **控件尺寸单一来源**：图标按钮 32/40 二选一并统一；`.icon-button` 36→32/40；checkbox 图形 18 + 点击区 ≥32；`.badge` 12px / min-height 22；`.field-hint` 行高 18；行内按钮与按钮字号回到 14；开关统一 36×20；档位删除按钮 ≥32。
4. **弹窗宽度收回 480 / 640 两档**。
5. **图标尺寸收敛到 2–3 档**（如 14/16/20），修掉 onboarding 被压扁的警告图标。
6. 浅色 `/` 改用 `--text-muted` 并实测 ≥4.5；`.shell` 的 `min-width: 860px` 改成断点 + 侧栏抽屉，过 960×640 与 200% 两关。

验收方式固定：跑 `design-conformance` 技能的脚本，**两套主题 + 960×640 + 200% 各一遍**，逐页 `overlap/overflow/touching/clipped/contrast/点击目标` 全 0，并复核 SKILL.md 末尾「已知基线」9 条。

### 批次 E：补齐承诺过的功能（按 PRD P0 缺口，1–2 周）

多 Key 闭环（新增第 2 个 / 改名 / 删除单个 / 禁用 + `credentials_disable`）→ 模型编辑器协议字段 →
工具调用门禁（或明确降级为「Chat Completions 标注实验」并写进差异弹窗）→ 供应商搜索框（逻辑已在，只缺 input）
→ 供应商预设（或从 PRD 移除该承诺）→ Windows helper 真正的 fail-fast（现在是假装成功）。

### 批次 F：把门禁补到能拦住上述问题（与 E 并行）

端到端探针进 CI（至少 macOS runner 上跑 `probe-catalog` 与 `probe-apply-pipeline`）；
补「切换供应商后表跟随」「`model/list` 完整快照」「诊断包 canary」「并发写不损坏」四条回归；
`release.yml` 去掉 fail-open（零产物必须失败）、tag 触发也构建 Windows（或在 README 明确 Windows 只手动触发）。

## 5. 最可能让这个产品失败的三件事（产品负责人判断）

1. **押注一个未验证的宿主行为，且没有 Plan B。** 「模型出现在 Codex 原生选择器」是全部价值主张，但它依赖 Codex 的内部契约，而 Codex 迭代很快。现在的应对是「指纹 + 白名单」，但**没有降级路径**：一旦 Codex 改了目录格式，产品就整体失效。建议把「诊断 + 恢复原生」做成第一等公民，并准备一个「自绘模型列表 + 路由」的降级形态。
2. **用配置治理的完备性换掉了个人工具的轻。** 六个侧栏页里四页在讲「可解释 / 可恢复」，这是给审计者看的，不是给用户看的。目标用户是要「快点让我的中转 Key 能用」。**建议把「供应商与模型」做成唯一主页**，诊断与日志收进抽屉，Codex 配置页与待应用条合并。
3. **在最需要确定性的三个点（应用结果 / 已加载 / 路由验证）都选择了「轻」。** 应用结果只活 5 秒 Toast，是本次审计里最突兀的产品决策——用户花 4 个模态换来一次写入，结果 5 秒后就不知道发生过什么。建议升级为页面级结果面板（含改了哪些字段、宿主是否已重载、下一步该做什么），这也是把 PRD §5 那套「已保存 / 已应用 / 已加载 / 已验证路由」真正变成用户能力的地方。

## 6. 未验证项

本报告与七份子报告都**没有**验证下面这些，不要在后续文档里当成已通过：

- **Windows 真机**：安装、凭据 helper、Codex 路由、NSIS/MSI 行为全部未验（本机无 Windows）。
- **Intel Mac**：9 个 release 全无 x86_64 产物，无法验。
- **macOS 原生窗口外观**、签名安装包的公证流程、Gatekeeper 在他人机器上的实际提示文案。
- **Codex Desktop GUI 模型选择器的视觉核验**：本次证据来自 app-server 的 `model/list` 与 `logs_2.sqlite` 的 `thread/start`，**没有**人眼确认下拉菜单里那一行的样子。
- **读屏软件**（VoiceOver）实际朗读、Tab 顺序的手工走查：设计 Agent 只做了结构性断言。
- **200% 缩放**是按视口/`deviceScaleFactor` 模拟的，不等同于 macOS 原生缩放。
- **性能与非功能目标**（PRD §6）：p95 ≤ 300ms 切换、网关附加延迟 ≤ 30ms、冷启动 ≤ 2s、内存预算——**全部无实测数据**，仍是目标值。
- **无关的第三方依赖许可证兼容性**：只做了清单核对，未做法务判断。
- 本报告对 P0-3「原生模型消失」的描述基于**本机** `model/list` 与备份文件；不同 Codex 版本的行为可能不同，未做多版本矩阵。
