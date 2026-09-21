# 全产品审核裁决（2026-09-22）

裁决人：产品总监 · 仓库：`/Users/example/Code/ProjDev/gptswitch-macapp` · 工作区版本 **0.3.0**（`git describe` = `v0.2.0-17-ge345bbd`，**未打标签、未发布**）
依据：《产品现状盘点 + 专项审核章程》§四 裁决标准（`docs/audits/2026-09-22-product-director-charter.md`）
输入：六份专项报告（需求 / 设计交互 / 前端 / 后端 / 测试 / 营销）+ 真机端到端探针运行记录（`docs/appendix/2026-09-22-e2e-probe-run.md`）+ 主控第一手复核事实
本轮仅写本文件，不改源码、不改六份报告。

---

## 一、结论先行

**0.3.0 不能算「达到 P0 承诺」，因此不能称稳定版、不能按现在的状态发布。** 三条 P0 同时成立且**都在对外路径上**：本机校验层唯一挡板（上游流截断被标成功）**已修但尚未提交**；Releases 页仍挂着会写坏用户 Codex 配置的 Windows 旧安装包；而 PRD P0 第 6 项（模型出现在 Codex 原生选择器并走对路由）自 09-19 起**连续多轮未验证**，本次真机探针只跑绿 1/4 条。

**把这句说完整：**「已修未提交」指 P0-A 的修复（`gateway/server.rs`、`protocols/chat.rs` + 新回归用例）**只在工作区里**，`git status` 显示为未提交改动；四道门是**本机**复跑全绿（558 通过），**提交后的树尚未经过一次完整 CI**。所以当前状态的准确定性是：**代码已就绪，发布链未就绪。**

**人工层（发布门禁第三层）本次第一次被真正跑过**——这是章程 §六 第 4 条「人工层可能长期没人跑」的答案，见 §七。结论是：**跑过了、留下了真实痕迹、≥1 条在真实 Codex 上端到端通过，但绝不是全绿**；另有三条因「产品端口不可配」与「探针自身的归属启发式」而未跑通/假阴性，两条都可修。

---

## 二、发布裁决

### 裁决：**no-go**

卡在三条 P0。按章程 §四，P0 一旦成立版本不得称稳定版，故不设「有条件 go」。

| # | 卡点 | 为什么它挡住发布 | 证据出处 |
| --- | --- | --- | --- |
| **P0-A** | 上游流在 SSE 帧中途被干净关闭时，网关把半截回复标成 `status: "completed"` | 章程 §四：产品对用户声称成功而实际失败 = P0。宿主会把被截断的回复当完整回复渲染，用户看不到任何错误事件 | `gateway/server.rs:582`（`saw_done` 存在）、`:645`（循环内用于提前跳出）、`:674`（修复后的判定）；`protocols/chat.rs`（生产代码原本完全不追踪 `finish_reason`）；后端报告 §7.2。**状态：已修未提交** |
| **P0-B** | Releases 页上 v0.1.1 / v0.1.2 / v0.1.3 **仍挂着可下载的** `_x64-setup.exe` 与 `_x64_en-US.msi` | 这批包带的正是主 README 自己判定为「**比没用更糟**」的行为（配置照写、界面报成功、弄坏本来可用的 Codex）。用户现在就能下到并受害 | `README.md:76-79` 自述该行为有害；Releases 页面实际附件（主控用 `gh` 核过）。营销报告 C3 |
| **P0-C** | PRD P0 第 6 项「模型在受支持版本 Codex 原生选择器可见、选中后走对应服务商和 Key」**未验证**；PRD §7 的双平台真机验收未完成 | 这是产品的全部价值主张，也是 PRD 自己写下的 P0 与发布条件 | 需求报告 §1 第 6 行、§6；`README.md:131-132` 自述 GUI 未验证；探针记录：`probe-catalog` 通过（app-server 层），`probe-apply-pipeline` / `probe-full-loop` 未跑通（端口 18765 被占），路由与 GUI 层仍无证据 |

### 三条「从 P0 承诺区滑走」的项 —— 由我裁决，不再作为 0.3.0 的 P0

需求审核员把这三条交上来定性（需求报告 §2）。作为产品总监，我的裁定是：

1. **供应商预设** ——**正式移出 0.3.0 的 P0**。理由：它需要一份「谁进清单、怎么描述」的内容决策，不是实现缺口；PRD §3 的 P0 文本已自述「尚未实现」，批次 E 有决策记录（`docs/audits/2026-09-20-batch-E-deviations.md`）。降为 P1 待办，最小形态限定为「用户自存的地址模板」，仍不得绑定推荐或充值。**下一轮动作：改写 PRD §3 的 P0 文本**（本轮不动 PRD）。
2. **Chat Completions 工具调用门禁** ——**正式接受「已标注实验」为 0.3.0 口径**。理由：门禁产物必须来自真实上游凭据，无凭据写出来只会是一份恒真检查；「标实验」这半条已落到应用前警告（`apply.rs:286-303` + 用例 `apply_service.rs:1158`）。降为 P1 待办。**下一轮动作：PRD §3 同步改写口径。**
3. **Windows x64 签名安装包** ——**0.3.0 明确声明为 macOS-only 首发，Windows 移出 P0**。理由：章程 §四 已把「Windows 被前置检查拦下、不写配置」定为正确行为；缺签名的 Windows 包对用户是负价值。**下一轮动作：PRD §3/§7 同步改写，并把 README/Release 的平台划界写成硬声明。**

---

## 三、本轮已修清单（4 项，**全部未提交**）

全部在工作区里，`git status` 显示为 modified；证据形态逐条给出。

| # | 修了什么 | 证据形态 | 复核方式 |
| --- | --- | --- | --- |
| 1 | **P0-A**：网关区分「正常结束」与「被截断」。判定改为「`[DONE]` 或非 null 的 `finish_reason` 二者之一」；都没有则发 `error.upstreamStreamIncomplete`、**不补完成事件** | `crates/switch-core/src/gateway/server.rs:674`（`if !saw_done && !translator.saw_terminal()`）；`protocols/chat.rs` 新增终态追踪；`crates/switch-core/tests/gateway_server.rs` 新增复现用例（**先失败后通过**） | 主控复跑四道门：`cargo test -p switch-core` **558 通过 / 0 失败 / 3 ignored** |
| 2 | **i18n messageKey 原样上界面 + 守卫盲区**：补齐 5 条 `instance.*`/`capability.*` 文案（中英各 5）；把 `capability`/`instance` 加进 `CORE_PREFIXES`；`CodexConfigPage` 渲染改走 `t()` | `src/i18n.test.ts:32`（前缀集合实含 `capability`/`instance`，当场核过）；`CodexConfigPage.tsx:364`（已是 `t(selected.blockedReasonKey)`，不再是裸拼）；`src/locales/{zh-CN,en}.ts` 各 5 处命中 | 主控当场 grep + 实测守卫：新键缺文案时 `pnpm test` **失败 2 用例**，补完转绿 |
| 3 | **侧栏最小窗口溢出 + 「设置」入口被压成 22px** | `src/app/App.module.css:9`（`.sidebar { overflow-y: auto }`）、`:17`（`.sidebar > * { flex-shrink: 0 }`），带成因注释 | 主控复测：侧栏可滚、各块禁止收缩、「设置」入口回到声明的 **44px**、页面级溢出消失（`scrollHeight == clientHeight`）；1280×720 下完全可见。**残留**：960×640 下该入口 44px 中有 20px 需滚 20px 才看全（顶端可见可点）→ 记 P2 |
| 4 | **供应商搜索框图标被 flex 压扁（14.68×16）** | `src/app/App.module.css:213`（`.providerSearch > svg { flex-shrink: 0 }`），带规范出处注释 | 当场核过该声明存在 |

**顺带确认的一条机制事实（值得记进裁决）**：`src/i18n.test.ts` 的前端守卫**真的有效**——Rust 侧新增 messageKey 若在两本字典里缺文案，`pnpm test` 直接失败（主控实测一次）。这条守卫是本项目目前**唯一**能自动拦住「核心产出键泄漏到界面」的门禁，其覆盖面（前缀白名单）应当被视为受保护资产，不能随手删。

---

## 四、本轮未修清单（P1 / P2 待办，六份报告去重合并）

### 已登记的例外与噪音（**不计入**，见章程 §四与主控第 6/8 条）

- 表单控件静息态描边低于 3:1（`--border-field`）：**2026-09-21 的产品决定**，登记在 `docs/design/01-foundations.md:89-116` 与 `docs/development/02-testing-and-release.md:111-116`。不算缺陷。
- 960×640 下供应商页表格横向滚动 83px：`.tableWrap { overflow-x: auto }`（`ModelsPage.module.css:35`、`App.module.css:106`）是设计内的正常行为，**不算缺陷**。
- 章程 §四 的「有意范围排除」（账号轮换 / OAuth / 语音栏 / 插件市场云端目录等）：不算缺陷。

### P1（本轮必须修）

| # | 现象 | 证据 | 归属角色 | 建议动作 |
| --- | --- | --- | --- | --- |
| P1-1 | **假开关**：内容中心「设置 GitHub 令牌」走 `window.prompt`，真机点了很可能无反应（无报错、无提示），而按钮与徽章都宣称可用 | `ContentPage.tsx:178-180`；wry 0.55.1 的 `WKUIDelegate` 未实现 alert/confirm/prompt 三方法（实测依赖源码），未实现时 WebKit 不弹面板、`prompt()` 返回 null，代码把 null 当「取消」直接 return | 前端（设计交互 co-sign） | **先真机点一次取证**（一步即可定性），确认后改用仓内已有的 `Dialog` 组件，与其它所有输入一致 |
| P1-2 | **a11y 明文规范违反**：破坏性行操作后无焦点管理 | `docs/design/05-patterns-and-accessibility.md:45` 明文要求；`ModelsPage.tsx` 批量删除 / 单行删除、`ProviderForm.tsx:353-364` 改完数据后无落点；全仓库 6 处 `.focus()` 无一处涉及列表变更 | 前端 | 列表变更后聚焦相邻行或新增入口；顺带核 `Dialog` 的 `previousFocus` 是否落在已卸载节点（推演，需真机走查） |
| P1-3 | **图标尺寸越出五档**：实测存在 11 / 13 / 15px（规范为 12/14/16/18/20，「其余值一律不收」） | 设计：`ContentPage.tsx:220,234`（13）、`:224,302`（15）、`:378`（11）；`PluginHubPage.tsx:268,273,278,301,400`；`ToolsPage.tsx:119,235,243,248`。前端独立复算出同批 18 处（两角色各报一次，**合并为一条**） | 设计（前端提供源码清单） | 按语义归位：行内 `external-link`/`triangle-alert` → 14；`plus`/`search`/`refresh-cw` 按钮图标 → 16；微型容器 11 → 12 |
| P1-4 | **协议「损失记账」用户看不到**：README 宣称「记录为 losses 而不是假装生效」，工程上成立、界面上不成立 | 核心已带 `message_key`（`protocols/mod.rs:23-37`），但网关只把 `feature` 拼进事件元数据（`gateway/server.rs:459-481`）；`src/locales/` 里 `loss.*` / `result.*` **命中 0**（本轮当场 grep 为 0）；i18n 前缀集不含二者；`LogsPage.tsx:174,242` 直接渲染键名 | 后端（前端配合补文案） | 把 `message_key` 带进事件元数据；补 `loss.*`/`result.*` 两族文案；`LogsPage` 对 `resultKey` 查表；把 `loss`/`result` 加进 `CORE_PREFIXES` 让守卫接住 |
| P1-5 | **应用结果仍只活 5 秒**：阶段卡已常驻（这一半是修复），但重启的三结论与 `quitForced` 强制重启警示**只存在于 5 秒 Toast**，离开页面不可回看 | 设计实测：`CodexConfigPage.tsx:199-210` 只 `showToast`、无 `useState` 保存 report；`Toast.tsx:31` 成功 5s 消失 | 设计交互（产品决策） | 把重启结论与 `quitForced` 警示升级为页面级常驻结果面板；Toast 只做跳转入口 |
| P1-6 | **对外自述直接冲突**：`docs/README.md:3` 仍写「**尚未用真实第三方供应商验证过**」，与 `README.md:120` 的「已对真实第三方供应商验证过」相反 | `docs/README.md:3`（当场核过原文）；`README.md:120-130` | 营销（产品配合） | 改为「机制已端到端跑通，并已在 macOS 上用真实第三方供应商验证过一次完整推理；**Desktop GUI 选择器的视觉渲染未验证**」 |
| P1-7 | **验证通道被固定端口堵死**：网关端口 18765 是硬编码常量、探针断言也写死它，于是本机跑着自家应用时，端到端验证层**根本无法执行** | `crates/switch-core/src/gateway/mod.rs:24`（`DEFAULT_PORT` 硬编码，探针记录核过）；探针记录：`probe-apply-pipeline` 第 2 步 `EADDRINUSE 18765`、`probe-full-loop` 卡在 `waitForGateway`（`error.portInUse`） | 产品 + 测试 | 二选一：给网关端口加可配置能力（并同步写进 Codex 配置）；或至少给探针一条 `GPTSWITCH_GATEWAY_PORT` 逃生口。**注意**：应用「拒绝自动换端口」本身是正确行为，缺的是用户/CI 的出口 |
| P1-8 | **`coexist-check.mjs` 判据假阴性**：脚本以「id 以 `gs/` 开头 == 我们的」判归属，但本机真实 `~/.codex` 已被用户自己的 Switchep 接管，原生条目也带 `gs/` 前缀，导致 3/6 断言误报；且脚本写死 `~/.codex` 为 native home | 探针记录：两次独立运行输出逐字相同；托管 alias 每次随机 UUID，探针两次都挑中同一条**用户原生 alias**；`/tmp/g0-coexist/bridge-check.log` 显示 `why=model-is-native` 是正确判定 | 测试 + 后端（bridge） | 判据改为按本次运行生成的 alias 白名单（而非前缀），并让 native home 可覆写；重跑应 6/6 |
| P1-9 | **自动化层三处缺口**：① 协议一致性 fixture 缺 4 类（多个并行 tool / 工具 output 错序 / 上游断流 / 背压）；② 故障注入缺 2 项（journal 各阶段真杀进程 / 磁盘满，现有恢复用例是手工改写事务记录伪造崩溃点）；③ AC 覆盖空洞（**AC-13 整条无覆盖**、AC-15 网关生成路径无针对性用例、AC-01「失败不删旧记录」/ AC-10 前端连点 / AC-16 失败隔离 / AC-12「登录与历史不动」关键半边无断言） | 测试报告 §2、§3.1、§3.2（全部带用例名与 grep 证据） | 测试 | 优先补 AC-13 与「上游断流」fixture（与 P0-A 同一族，已修的行为需要固定下来）；其次补多 tool 交错与 journal 杀进程 |
| P1-10 | **CI `probes` job 断言零执行**：只做 `node --check` + 依赖自检，探针里的断言从不运行，改坏了照样过 | `.github/workflows/ci.yml:119-139`（`:130` 语法、`:132-138` 依赖自检） | 测试 | 把 `mock-provider.mjs` / `catalog.mjs` / `rpc.mjs` 承载的契约（不需要 Codex 的部分）抽成 Node 测试，在 `probes` job 里跑 |

### P2（记入待办）

**版面 / 设计**：间距阶回归（`gap:6px`/`20px`、`padding:0 14px`、38/34px 等，`01-foundations.md:166` 之外的实测值）；「API Key」标签与输入框 0 间距；字阶字重小偏差（18/26 的 w700、24/32 的 w500、2 处 12/20）；200% **模拟**下长 URL 不换行；960×640 下「设置」入口 20px 需滚。

**前端**：D1「有没有新版本」两份真值（`App.tsx:148` vs `SettingsPage.tsx:30`）；D2 设置页镜像 `gateway.paused`；**D3 概览「模型调用」写死「未测试」、不读 `connectionStore`（三个重复真值里最严重的一个，两处表面互相矛盾）**；幂等键两份实现（`PendingApplyBar.tsx:150-152` 重复 `idempotency.ts`）；清空 GitHub 令牌 fire-and-forget 无 catch；安装目标拉取失败被显示成「没有」；CSS 平手失效 2 处（同行 13px/14px）；5 个页面缺卸载守卫；`role="tab"` 缺 `tabpanel`/方向键；禁用能力原因只在 `title`（与 P1-3 之外的 PDF/视频项）；核心中文自由文本进英文界面 + 用中文子串做语义判定；外部链接两套打开机制；Chat Completions「实验」静态文案（门禁落地后必须改读核心结论）；chunk 告警应去文档化而非做懒加载。

**后端**：Bridge 路由锁中毒静默回落原生、不留痕（`lib.rs:866-869`）；`revoke_auth_helper` 无任何调用点却注释承诺退出时清理；脱敏阈值残余（20–23 位、非混排 24–39 位仍漏，当前无承载路径）；`OverviewPage` 的「模型调用」面不读真实数据。

**需求 / 文档**：托盘行为代码（`main.rs:374-379` 隐藏到托盘）与 `docs/appendix/01-source-index.md:265` 自述相反；`act(...)` 警告仍在。

**营销（方案已给成稿，见营销报告 Part B）**：命名收口三处可见面（Releases 页新旧名并存、`How it works` 图里的 `gptswitch`、Release 正文缺命名说明）；首屏按 B3 成稿重写（信任要素上提 + 已证实/未证实分块）；Release 正文补差异化块；补「卸载 / 还原原生」用户文档；加 GitHub topics 与 Homebrew Cask；确认 0.3.0 的 `latest.json` 会让老用户更新到 0.3.0。

**未验证（不得当作已通过）**：**签名 bundle 与裸二进制是否等价（单列，见 §7.4——本轮探针全程只用裸二进制，「应用壳层」这一层没有被真正验过）**；真实 WKWebView 运行时差异；读屏软件；真机 Tab 顺序；macOS 原生 200% 缩放；Windows / Intel Mac；真实上游质量；PRD §6 全部非功能目标（无数据）。

---

## 五、对历史审计的对照总表

### 5.1 本轮**推翻**（历史结论已不成立，不得再报为问题）

| 历史条目 | 历史结论 | 本轮结论 | 出处 |
| --- | --- | --- | --- |
| P0-1 多 Key 不闭环 | 无法新增 / 改名 / 删除 / 禁用 | **已修复** | 需求 §5.1；`credential.rs:268`、`workspace_service.rs:571,613` |
| P0-2 供应商搜索是死代码 | 无输入框 | **已修复** | 需求 §1 行 1b；`App.tsx:354` |
| P0-4 模型编辑器缺协议字段 | `ModelDraft` 无 protocol | **已修复** | 需求 §1 行 4；`ModelEditorPage.tsx:144-146`、`apply_service.rs:1104` |
| 09-20 设计 P0 切换供应商后模型表不跟随 | 跨轮未修 | **已修复且未复发**（派生写法） | 前端 §4；`ModelsPage.tsx:50-51,63` |
| 09-20 P0-4 向导中途消失 / `onboardingForced` 死状态 | 无重开入口 | **已修复** | 前端 §4；`App.tsx:120,156`、`SettingsPage.tsx:151` |
| P0-2 网关静默失败 | 未知版本等不写响应就关连接 | **已修复**（4 条针对性测试） | 后端 §7.1(b) |
| P0-5 写盘原子性两洞 | 临时名固定 + 发布不可回滚 | **已修复**（改法与历史建议相反：写前发布 + 写后回滚，两洞均闭合） | 后端 §2.1、§8 |
| P1-14 前半 `reasoning.effort` 空集合时 chat 丢弃 / responses 透传 | 自相矛盾仍在 | **已修复**（两边行为一致） | 后端 §5.1 |
| P1-15 Windows helper 假装成功 | 配置照写、界面报成功 | **已修复**（fail-fast 一路传导到拦下应用） | 后端 §7.1(a) |
| P1-11 许可证文案 / P1-5、P1-6 控件尺寸与弹窗三档 / 09-19 浅色 4 处对比度 / 09-21 两处 `touching` / 概览五格等高 / 待应用条跨页吸底 / private-patterns 路径 / README 下载表 | 分别记为缺陷 | **均已修复** | 前端 §4；设计 §5；营销 A7 |
| 09-20 隐私：设置页「保留所有权利」 | 与 MIT 矛盾 | **已修复** | 营销 C5 |
| 测试 `zz_audit_probe.rs` 残留 | 应清理 | **已修复** | 测试 §6 |

### 5.2 本轮**复核确认仍存在**（历史已认账，**非新发现**）

- P0-3 供应商预设未实现；P0-6 `已验证路由` 无独立状态；P0-7 Windows 签名包不存在、脱敏规则部分修复。
- P1-13 panic 面（`expect("锁未被污染")`）仍在。
- 「应用结果只活 5 秒」——**部分修复**（阶段卡已常驻，重启结论仍只活 5s）。
- 「已加载需用户自己宣布」（`CodexConfigPage.tsx:434`）；「路由验证无 UI 入口」。
- 术语泄漏（部分收敛，messageKey 一类本轮已修）；`act(...)` 警告仍在；无卸载文档；托盘行为文档与代码相反。
- i18n 守卫「只扫 crates」这一猜想**被推翻**——`src-tauri` 早已扫（本轮实测收 10 键），真正的盲区是前缀白名单。

### 5.3 本轮**新增**（历史未登记）

| 类别 | 条目 |
| --- | --- |
| **对外有害（P0-B）** | Releases 页仍挂着 0.1.1–0.1.3 的 Windows 安装包，其行为被本项目自己判定为「比没用更糟」 |
| **产品缺陷（已修）** | P0-A 上游流截断被标 `completed`；侧栏最小窗口溢出 + 设置入口 22px；i18n messageKey 泄漏 + 守卫盲区；搜索图标被 flex 压扁 |
| **产品缺陷（未修）** | 假开关 `window.prompt`；破坏性行操作无焦点管理；图标越五档 11/13/15；间距阶回归；PDF/视频禁用原因只在 tooltip；losses 不可见（定位到具体断点）；三条重复真值来源 D1/D2/D3 |
| **协议 / 门禁** | 协议 fixture 缺四类；故障注入缺两项；AC-13 无覆盖、AC-15 网关分支无用例；`probes` job 断言零执行 |
| **后端** | Bridge 锁中毒静默回落；`revoke_auth_helper` 无调用点 |
| **验证通道** | 固定端口 18765 堵死端到端探针；`coexist-check.mjs` 判据假阴性；**人工层第一次留痕（1/4 绿，`probe-catalog` 在真实 Codex 上端到端通过；痕迹已进 `evidence-manifest.json` 的 `e2e_probe_runs`，见 §七）**；`~/.codex` 可疑但未定性的 SQLite 变动 |
| **市场面** | 冷启动基线实测：v0.2.0 附件下载 **0**、外部引荐 **0**、star **1** ——当前第一障碍不是转化率而是**没有流量**；Release 正文不含任何差异化与信任要素；命名困惑的第三落点（Releases 页新旧名并存） |

---

## 六、下一轮的三个最重要动作

1. **把已修的落进版本，并清掉对外有害项。** 提交工作区里的 5 项改动（P0-A + i18n + 侧栏 + 搜索图标），在**提交后的树**上完整跑一遍 CI；同时处理 P0-B：给三个旧 Release 加「已撤回，请勿下载」声明或直接删附件，并在 README 的 Windows 段注明 ≤0.1.3 的 Windows 包已作废（~15 分钟）。**这是唯一一条会主动伤害用户的现存问题，优先级在一切之上。**

2. **把「能不能发布」这个问题真正回答掉：让验证层跑起来并拿到 PRD P0 第 6 项的结论。** 三件小事：给网关端口一条可配置出口（P1-7）、修 `coexist-check.mjs` 的判据（P1-8）、在用户退出自己那份应用后重跑三个探针；然后**真机看一次 Codex 的模型选择器**，逐条落进 `docs/appendix/evidence-manifest.json`。这一条不解决，P0-C 永远开着。

3. **修一批对外可感知的短平快 P1**：`window.prompt` 假开关（先真机一步取证）、破坏性行操作后的焦点管理、losses 的文案与日志页查表、图标回归五档。四件事都局限在少数文件，且都是用户能直接看到或摸到的。

> 顺序理由：动作 1 关掉**正在伤害用户**的路；动作 2 关掉**决定产品是否有意义**的问号；动作 3 关掉**下一批用户会立刻碰到**的毛刺。三者之间无依赖，可以并行，但缺任何一条都不该对外宣传 0.3.0。

---

## 七、人工层（发布门禁第三层）本次验证结果

**「人工层现在到底有没有被真正跑过？」——跑过了。这是第三层第一次留下真实痕迹，但没有一条能当作「发布门禁已过」。**

原始材料：`docs/appendix/2026-09-22-e2e-probe-run.md`；证据已追加进 `docs/appendix/evidence-manifest.json` 的顶层 `e2e_probe_runs`（`research_date 2026-09-22`、`project_version 0.3.0`、`commit e345bbd`、`status` 不再是 `documentation_only` 意义上的空白）；原始输出留档在 `/tmp/e2e-probe-2026-09-22/`。环境：macOS 26.6.2 arm64，真实 Codex `/Applications/ChatGPT.app/Contents/Resources/codex` = `codex-cli 0.155.0-alpha.9.2`。

### 7.1 四条探针的判定

| 探针 | 退出码 | 判定 |
| --- | --- | --- |
| `probe-catalog.mjs` | 0 | **通过**（4 条断言全过） |
| `probe-apply-pipeline.mjs` | 1 | **未跑通** —— `EADDRINUSE 18765`（第 1 步管线成功，第 2 步起 mock 上游撞端口） |
| `probe-full-loop.mjs` | 1 | **未跑通** —— 应用网关 `error.portInUse` 18765；**裸二进制本身可用**（它起来了、跑了启动恢复，然后**按设计**拒绝换端口） |
| `coexist-check.mjs` | 1 | **失败，但 6 条断言里 3 条是环境假阴性**；bridge 路由其实正确 |

### 7.2 三条必须记住的事实

1. **真绿那一条含金量高。** 真实 `codex 0.155.0-alpha.9.2` 在 `limit:1` 翻页下 `model/list` 恰好返回 2 条（`gptswitch/probe-a` / `gptswitch/probe-b`），`inputModalities` 分别为 `['text']` 与 `['text','image']`，思考档位也对上——**手写 `model_catalog_json` 的契约在真实宿主上成立**。这是本项目证据清单里此前从未有过的一层（此前全部证据停在自造夹具与 app-server 的合成调用上）。但探针自己也写明：**Desktop UI 与实际路由仍未验证**。
2. **两条被同一个固定端口挡住，根因是产品侧不可配。** `gateway::DEFAULT_PORT` 是**硬编码常量、没有 env 覆盖**（`crates/switch-core/src/gateway/mod.rs:24`），探针断言也写死 `18765`；而用户**自己安装的 `/Applications/Switchelp.app`（PID 14823，已运行 1h46m）正占着它**。执行者**没有杀用户的应用**——这是正确的克制。**按「可测性缺陷」入表（P1-7）**：只要用户的应用在跑，应用层端到端探针就跑不了，而这一层是发布前必须人工跑的一层。
3. **`coexist-check.mjs` 的失败是探针自身的测试设计缺陷，不是产品缺陷。** 这台机器真实的 `~/.codex/config.toml` **自己就是共存模式**（`model = "gs/767e7394-…"`），于是 native 那根也返回 `gs/` 前缀 alias，而探针用「以 `gs/` 开头即我方」做启发式判定。铁证：探针当成「我们的」那条 `gs/57745d8a-…/14f3fca6-…` 与用户真实目录里的 alias **逐字相同**，而探针自己的托管 alias 是每次随机的；两次复跑输出逐字一致。bridge 判 `model-is-native` **是正确的**。**按「测试/探针缺陷」归类（P1-8）**，修法：不能用前缀启发式判归属（改用本次运行生成的 alias 白名单），且脚本写死 `~/.codex` 作为 native home，应支持 `GPTSWITCH_BRIDGE_NATIVE_HOME` 覆盖。

**一处必须如实点出的自律违背**：因上述假阴性，探针的 `thread/start` 被**正确**路由到了 **native**（即用户真实的 `~/.codex`），违背了脚本「不真起原生线程」的自律。实际影响经核查为**零**：未跑回合、无 rollout 落盘，`thread/resume` 返回 `no rollout found`，该 threadId 在整个 `~/.codex` 里搜不到。

### 7.3 非干扰核查（执行者独立做的，计入「本轮验过什么」）

- **钥匙串无残留**：跑完实测 `security find-generic-password -s app.gptswitch.desktop -a <新 secretRef>` → `EXIT=44` item not found；仍剩下的 4 条全部属于用户真实 provider，未被触碰。
- **`~/.codex` 未被写坏**：11445 个文件全量 hash 前后对比；探针起过的 threadId 在整个 `~/.codex` grep 不到；`sessions/` 60 分钟内零改动；用户的 `config.toml` 原样未动。
- **一处「已记录的可疑但未定性项」（不得写成「确认无写入」）**：某个窗口里几个 SQLite（`logs_2`、`thread_history_1`、`goals_1`、`queue_1`、`memories_1` 及 `-wal`/`-shm`）出现变动。75 秒空闲对照**零变动**、复跑 coexist **零变动**，因此更可能是**并发的 ChatGPT 桌面端**写入（那两个库分别有 313 MB / 398 MB，是活跃使用中的库）；**但不能完全排除**是 `codex app-server` 以真实 home 启动时的记账写入。如实记录，不下结论。

### 7.4 单列的一条遗留未验证：签名 bundle 与裸二进制是否等价

本轮探针**全程只用裸二进制** `target/debug/gptswitch`（57 MB）——因为本会话对 debug 包做 `codesign` 会卡在 `SecKeyCreateSignature`（钥匙串授权阻塞），`target/debug/bundle/macos/Switchelp.app` 是**未完成构建的残骸**（缺 `Contents/_CodeSignature`、残留 `.cstemp`）。因此**「应用壳层」这一层这次并没有被真正验过**，不得混进「探针通过」。已发布的 `dist-release/Switchelp-0.3.0-arm64.zip` 签名有效（`codesign --verify --deep --strict` 通过、满足 Designated Requirement），但如文档所载被 Gatekeeper 以 `Unnotarized Developer ID` 拦下。

### 7.5 这一层要变绿，需要什么

① 用户退出自己运行中的 `/Applications/Switchelp.app`（或给出端口可配置能力，P1-7）；② 用一个**不在共存模式**的 native home 跑 `coexist-check.mjs`（P1-8）；③ 之后重跑，并把输出贴进 `evidence-manifest.json` 的 `e2e_probe_runs`。这三件都已在 §六 动作 2 里排上。

---

**裁决结束。** 六份报告与探针记录的全部结论已按章程 §四 去重、定级并归因；历史已认账项只做「是否已修复」的复核，未重复计数；章程 §四 的已登记例外（表单静息态描边）与主控指出的噪音（表格横向滚动）已排除。
