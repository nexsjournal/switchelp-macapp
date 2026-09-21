# 前端专项审核报告

日期：2026-09-22 · 角色：前端审核员（向产品总监汇报）
仓库：`/Users/example/Code/ProjDev/gptswitch-macapp` · 工作区版本：0.3.0
任务书：`docs/audits/2026-09-22-product-director-charter.md` §5.3
本报告**只读源码 + 只跑只读命令**，未改任何源码；除本文件外没有写入仓库内任何文件。

---

## 0. 方法与证据边界

本轮我没有重跑整套 CI（章程已给出基线：typecheck 通过、187 用例全过、build 成功、553.08 kB / gzip 161.43 kB）。我跑了下面这些，全部只读：

| 命令 | 目的 | 结果 |
| --- | --- | --- |
| `npx vitest run --reporter=basic` | 复核用例总数与逐文件分布 | **Test Files 20 passed / Tests 187 passed**（与章程基线一致）；stderr 仍有 `act(...)` 警告 |
| `npx vitest run src/i18n.test.ts src/i18n.locale.test.tsx` | 跑 i18n 守卫 | 2 文件 / 9 用例全过 |
| node 脚本比对两本字典 | 键集合 / 占位符 / 空文案 | zh 1147 · en 1147，集合全等，占位符 0 处不一致，0 条空文案 |
| node 脚本复刻 `i18n.test.ts` 的 `collect()` | 守卫**覆盖面**实测 | crates 收集 164 键、src-tauri 10 键；**`instance.*` 与 `capability.*` 一族收集不到**（见 §2.2） |
| `npx vite build --config /tmp/vite.chunk-report.config.ts`（outDir 指到 `/tmp`，**未写 dist/**） | 给主 chunk 做归因 | 见 §2.6 分块表 |
| 构建产物 `dist/assets/index-BVAfjzw6.css` 偏移比读 | 层叠顺序实测 | 模块规则在前、`global.css` 在 94961 字节处（最后一段），平手时全局赢 |
| `~/.cargo/.../wry-0.55.1/src/wkwebview/**` | 查真机 WebView 是否实现 JS 面板 | `WKUIDelegate` 只实现了文件上传 / 媒体权限 / 新窗口三个方法，**没有 alert/confirm/prompt** |

**结论分两档标注**：
- 「实测」= 命令输出或构建产物比读得到的数字/事实；
- 「静态判定」= 源码 + 字典 + 契约三处一致推出的结论，未在真机复现（凡此类均在该条内写明）。
- 真机 WKWebView 运行时差异、真机键盘/焦点行为：**一律未验证**（见 §5）。

---

## 1. 结论先行

| # | 命题 | 判定 | 一句话理由 |
| --- | --- | --- | --- |
| 1 | 状态管理所有权 | **基本合格，3 处重复真值来源** | App 持有供应商/模型/网关/摘要四组真值并向下传，派生而非复制（P0-1 的修法仍在）；但 `update`、`paused`、`connection` 三处出现「同一事实两份」 |
| 2 | i18n 双字典一致性 | **字典合格；守卫有系统性盲区（P1）** | 1147/1147 全等、占位符与空文案 0 问题；但守卫的前缀白名单漏掉 `instance.` / `capability.`，已有键真的缺、且已经显示到界面上 |
| 3 | a11y | **结构面合格，缺口集中在焦点回位与 tabs 语义** | 无正 tabindex；表格 `scope`/`aria-sort` 齐；弹窗 Escape + 焦点回位有实现且有 1 条用例；缺口见 §2.3 |
| 4 | CSS Module 与全局样式的边界 | **老坑已按共识修法修好，仍有 4 处平手失效** | `.providerSearch`/`.searchRow` 用双类名提到 0,2,1（有注释）；`.addSource input`、`.addRow input` 等仍以 0,1,1 平手输给后置的 `global.css` |
| 5 | 错误处理 / 竞态 / 乐观更新 | **无乐观更新、无回滚需求；3 处丢错误 + 竞态守卫不一致** | 所有写操作都等后端回值，没有先翻本地状态再回滚的写法；但 1 处 fire-and-forget 无 catch、5 个页面缺卸载守卫 |
| 6 | 构建体积与分包 | **告警与 lucide 无关；不值得为它做懒加载** | lucide 只占 25.01 kB（57 个图标，node_modules 里 3502 个文件 / 14 MB）；主 chunk 是 React 143 kB + 业务代码 330 kB，且产物里没有任何动态 import |
| 7 | 假开关 | **1 处（P1）**：GitHub 令牌的输入走 `window.prompt`，真机很可能静默无反应 | 见 §3 |

---

## 2. 逐项

### 2.1 状态管理

**所有权是清晰的，而且是「派生而非复制」这条正确路线。**

- 宿主 `App`（`src/app/App.tsx:100-149`）持有：`providers` / `models` / `credentialsByProvider` / `gateway` / `summary` / `selectedProviderId` / 各弹窗与编辑器状态 / `onboarding*`。页面都是它的受控子节点，没有第二处真值。
- 历史 P0-1（切换供应商后模型表不跟随）的修法**仍在**，且是派生写法而非同步写法：
  - `src/features/models/ModelsPage.tsx:50-51`：`const [pickedProvider, setPickedProvider] = useState('all'); const providerFilter = providerScope ?? pickedProvider;`——嵌入时的作用域来自宿主，**不再进 state**；注释解释了原故障。
  - `src/features/models/ModelsPage.tsx:63`：`useEffect(() => { setSelected([]); }, [providerScope])`——换供应商清勾选（批量条不会去删看不见的行）。
  - 真实回归用例在 `src/app/App.test.tsx`（含「切换动作后表格跟随」一族，24+30 条里）。
  - **判定：未复发。**
- 连接状态是「会话级单例 + 订阅」：`src/features/models/connectionStore.ts:18-39`（模块作用域表 + `useSyncExternalStore`），理由（切页面不丢、持久化会让用户对着过期绿点做判断）写在文件头。

**重复真值来源（3 处）**

| # | 事实 | 两处落点 | 后果 |
| --- | --- | --- | --- |
| D1 | 「有没有新版本」 | `src/app/App.tsx:148` 的 `update` state（启动时查一次，`App.tsx:200-202`）与 `src/features/settings/SettingsPage.tsx:30` 的 `update` state（设置页自己查一次，`SettingsPage.tsx:62`） | 两处互不通知：在设置页点「检查更新」查到新版本时，侧栏左上角的更新胶囊**不会出现**（它属于 App 那份，仍是启动时的结论）；反过来 App 侧有新版本、设置页未查过时显示「尚未检查」。同一事实两份，且两处都在同一屏可见 |
| D2 | 「网关是否已暂停」 | App 的 `gateway.paused`（`App.tsx:111,176`）与 `SettingsPage.tsx:39-40` 的本地镜像 `paused`（`useEffect` 同步） | 托盘菜单改了暂停态后，设置页要等一次 `refresh` 才跟上。**注**：开关本身没有乐观翻转（`SettingsPage.tsx:42-46` 用后端返回值 `setPaused(await …)`），这一点是对的 |
| D3 | 「测试连接的结论」 | `connectionStore`（唯一写入方 `ModelsPage.tsx:158`）与概览页 `src/features/overview/OverviewPage.tsx:170` 的**写死文案** `t('overview.untested')` | 概览「连接状态」卡里的「模型调用」永远是「未测试」，从不读 `connectionStore`。用户在网关页把一行测成绿的、切到概览，那张卡仍说未测试——同一事实两个表面互相矛盾 |

D3 的补充实测：`useConnections()` 的消费方**只有** `ModelsPage.tsx:59`（`grep -rn "useConnections" src` 只命中 store 自身、ModelsPage 与它的测试）。也就是说 `connectionStore` 的「切页面不丢」只对网关页成立，概览页没接上。

**幂等（`src/features/codex/idempotency.ts`）**

- 规则本身只有 8 行，注释明确要求「Codex 配置页与待应用条都会发起提交，两处必须用同一个规则」。
- **实测：这条要求没被遵守。** `src/app/PendingApplyBar.tsx:150-152` 又本地定义了一份 `newIdempotencyKey()`，并未 import 那份模块。两份实现今天逐字相同，所以行为一致；但「同一事实两份代码」正是它注释里警告的事。
- 连点幂等靠 `busy` 禁用：提交按钮在 `busy` 期间 `disabled`（`src/features/codex/ApplyConfirmDialog.tsx:110-113`、`src/app/App.tsx:451`、`src/features/models/ModelsPage.tsx:304`），且 `newIdempotencyKey()` 每次调用生成**新**键——所以真正的重复保护来自核心的 planHash/CAS 与 `error.operationAlreadyFinished`，前端只做到「按钮在忙时点不动」。这一点符合 `docs/design/05-patterns-and-accessibility.md:38`「UI 不乐观宣称成功」，不构成缺陷；**未验证**的是真机上一次极快双击是否可能抢在 React 重渲染之前发出两次（jsdom 下未复现，真机未测）。

### 2.2 i18n 双字典一致性

**字典本身：合格（实测）**

```
zh keys 1147 · en keys 1147 · 集合相等: true
占位符不一致: []
空文案: []
```
抽查 6 条带占位符的键（`codex.catalogReplacesList` / `overview.stepApplyPending` / `shell.awaitingHostBadge` / `tools.summary.counts` / `settings.updateFailed` / `models.bulkBody`），中英占位符逐一对应、无空串、无回退到另一种语言。守卫 `src/i18n.test.ts` 5 条不变量全过（含「字典里没有没人用的键」）。

> 口径说明：本报告写作期间 `src/locales/{zh-CN,en}.ts` 正被**另一路审计并发修改**（`git status` 显示两者各 +1 行）。上面的 1147/1147 是我测量时的快照；交稿前复核为 **1148/1148，对称性不变**，且 §2.2 点名的三个缺失键在复核时**仍然各 0 命中**。

**守卫覆盖面：有系统性盲区（本条是本轮 i18n 的主要结论）**

章程的假设是「只扫 crates？有没有漏 src-tauri」。**实测：`src-tauri` 已经扫了**（`src/i18n.test.ts:82`，批次 B 补的，注释也写了理由），所以那一半不成立。真正的问题在**前缀白名单**：

- `src/i18n.test.ts:32`：`const CORE_PREFIXES = new Set(['action','compat','credential','error','group','host','probe','reason','stage','warning'])`；Rust 侧的扫描（`:79`、`:82`）只收「第一段在白名单里」的字符串。
- 复刻 `collect()` 实测：crates 收 164 键、src-tauri 收 10 键；而 `'instance.reason.noCli'`、`'capability.reason.upstreamUnsupported'` **都不在其中**（我让脚本直接打印了这两个键的 membership = false）。
- 这两族键是**真实存在**的核心产出：
  - `crates/switch-core/src/codex/detect.rs:244,246`：`Some("instance.reason.noCli")` / `Some("instance.reason.configRootMissing")`，经 `CodexInstance.blocked_reason_key` 上报。
  - `crates/switch-core/src/domain/capability.rs:198,202,206`：`capability.reason.upstreamUnsupported` / `.hostCannotSend` / `.unverified`。
- 它们在字典里**都不存在**（实测：`instance.reason.noCli` 在 zh-CN.ts / en.ts 各命中 0 次；`capability.reason.*` 同理），而界面**已经在显示它们**：
  - `src/features/codex/CodexConfigPage.tsx:364`：`{t(`compat.${selected.compatibility}`)}{selected.blockedReasonKey ? ` · ${selected.blockedReasonKey}` : ''}`——**原样拼接**，没有过 `t()`。CLI 未找到时这一栏就是「未验证 · instance.reason.noCli」。
  - `src/features/codex/CodexConfigPage.tsx:342-345`：共存卡片的 `coexist.blockedReason` 走了 `t()`，但缺键时 `t()` 按设计返回键本身（`src/i18n.ts:97-109`），所以 CLI 存在、`~/.codex/config.toml` 不存在时（`src-tauri/src/commands.rs:698` 把 `instance.blocked_reason_key` 直接当 `blocked_reason` 返回，`ready = blocked_reason.is_none()`）这一行同样会显示 `instance.reason.configRootMissing`。
  - 第二处还留了一个无意义的重复表达式（`:344` 的三元两支相同），说明这段当时是在「怕漏文案」的心态下写的，而不是靠守卫保证。
- 现有守卫为什么拦不住：`src/i18n.locale.test.tsx:19` 的 `KEY_SHAPED` 正则前缀表同样**没有 `instance`**，所以那条「两种语言下都不把文案键摆到界面上」的用例也照不到（而且它只渲染概览与设置页，从不进 Codex 配置页）。
- **修法必须两头改，否则改不动**：往字典补键之后，「字典里没有没人用的键」这条（`src/i18n.test.ts:104-107`）会**失败**——因为 `required` 集合永远不会包含这两族键。正确顺序是：把 `instance`/`capability` 加进 `CORE_PREFIXES`（若希望它们被强制存在）或加进 `DYNAMIC_KEYS`（`:39-56`），再补两本字典，最后把 `CodexConfigPage.tsx:364` 改成经 `t()` 取文案。
- 判定依据：`docs/design/02-icons-type-and-copy.md` §4.1 的负面清单把「内部标识、协议字段名直接上界面」列为已修整的缺陷类型之一（该轮改了 60+ 处）。**把 messageKey 摆到界面上属于同一类，按章程 §四属违反明文规范 → P1。**

**另一处与 i18n 相关但不是键问题的**：核心的自由文本直接进界面。`crates/switch-core/src/application/apply.rs:658` 生成 `format!("{} 已被外部修改，将保留当前值", …)`（中文），前端 `src/features/codex/ApplyConfirmDialog.tsx:88` 用**中文字串包含**来把它归类成「冲突警告」：`plan.warnings.filter(raw => raw.includes('已被外部修改'))`。两个后果：英文界面下这条警告正文是中文（`warningOf()` 翻不出来，会配一个通用标签「警告」+ 中文细节），且分组标题依赖核心措辞不变。P2。

### 2.3 a11y（`src/app/accessibility.test.tsx` 之外）

先确认**已覆盖且合格**的部分（`src/app/accessibility.test.tsx`，5 条 + 相关用例）：
- skip link 可达、`main` 有 `tabIndex={-1}`（`App.tsx:275,306`）；全仓库**没有正 tabindex**（`grep -rn "tabIndex" src` 只命中 `App.tsx:306` 的 `-1`）。
- 表格：`th` 全部带 `scope="col"`（`ModelsPage.tsx:212,222,223` 与排序表头 `:166` 还带 `aria-sort`；`ApplyConfirmDialog.tsx:142-145`），有断言。
- 纯图标按钮：我用脚本枚举了「内容只有一枚图标元素」的 `<button>`，全部带 `aria-label`（例如 `App.tsx:317`、`ToolsPage.tsx:105`、`LogsPage.tsx:136`、`ContentPage.tsx:224` 实际是文字按钮）。**未发现缺 accessible name 的图标按钮。**
- 弹窗：Radix 提供焦点陷阱；`src/components/Dialog.tsx:34-41` 记录 `previousFocus` 并在关闭时归还；Escape 关闭走 `onOpenChange → close()`，`busy` 时不可关（`Dialog.tsx:35`），脏表单先问「继续编辑 / 放弃修改」，默认焦点在「继续编辑」（`Dialog.tsx:54`）——与 `docs/design/05-patterns-and-accessibility.md:34` 一致，且 `src/app/App.test.tsx:29-43` 有断言（Escape → 确认 → 焦点回触发控件）。
- 表格行不是「无语义 clickable div」；行操作是按钮 + `RowMenu`（`src/components/RowMenu.tsx:86-100`，`aria-haspopup`/`aria-expanded`/`role=menu|menuitem`/方向键/Escape 回焦）。

**缺口（按可执行性排序）**

| # | 缺口 | 证据 | 判定 |
| --- | --- | --- | --- |
| A1 | **破坏性行操作后没有焦点管理**。设计明文要求「批量删除后聚焦相邻行或新增入口」（`docs/design/05-patterns-and-accessibility.md:45`）。全仓库 `.focus()` 只出现在 6 处，**没有任何一处在列表变更之后**（`App.tsx:275` skip link、`ProviderForm.tsx:165` 校验失败回输入框、`Dialog.tsx:40`、`RowMenu.tsx:55/74/82`）。`ModelsPage` 的批量删除（`:121-140`）、单行删除（`:268-273`）、供应商弹窗里的删除模型（`ProviderForm.tsx:353-364`）改完数据后都没有落点 | 静态判定 | **P1**：违反明文规范；键盘用户删除后 Tab 顺序从头开始。附带一条**推演**：`Dialog` 的 `previousFocus` 可能是刚被卸载的菜单项（`ModelsPage.tsx:254-275` 从 `RowMenu` 打开确认框，菜单项随 `setOpen(false)` 卸载），`focus()` 落在已脱离文档的节点上是空操作，于是关闭确认框后焦点会留在 `body`。**未在真机走查**（本环境按键注入不可用，历史审计同源记录：`2026-09-19-design-conformance.md:138-139`） |
| A2 | **`role="tab"` 缺 `tabpanel` 与方向键**。`src/components/SegmentedTabs.tsx:31-43` 输出 `role=tablist`/`role=tab`/`aria-selected`，但全仓库没有 `role="tabpanel"`、没有 `aria-controls`、没有 roving tabindex 与左右方向键 | 实测（`grep -rn "tabpanel\|role=\"tab" src`） | **P2**：键盘路径没断（Tab + Enter 可用），但读屏会念「选项卡 2/3」而方向键无反应，与 ARIA tabs 的承诺不符 |
| A3 | **禁用控件的「为什么」只在 `title` 里**。`src/components/CheckCell.tsx:22,29` 把 `hint` 放在 `<label title>` 上；`ModelEditorPage.tsx:154` 传入的正是「PDF 与视频当前链路不能原生发送……」（`zh-CN.ts:292`）。该文案不会成为复选框的 accessible description，也不是可见文字 | 静态判定 | **P2**：与 §1.1「Tooltip 不能成为唯一…说明」及 `docs/design/05:52`「重要差异不得只 hover 可见」的精神不符，且 PDF/视频禁用正是规范点名要可见可解释的项 |
| A4 | `RowMenu` 的 `role="menu"` 没有 Home/End，也没有 tab 出菜单后的收敛；`aria-sort` 只在模型表出现（日志表无排序，符合设计）。属可忽略项 | 静态判定 | 记 P2，不值得单独立项 |

### 2.4 CSS Module 与全局样式的边界

**先量清楚机制（实测，来自构建产物）**：`dist/assets/index-BVAfjzw6.css` 里模块规则集中在前面，`global.css` 落在 94961 字节处（最后一段）——因为 `src/main.tsx:7-8` 在 `App`（`:4`，会带出所有 module CSS）**之后**才 import `tokens.css`/`global.css`。所以**同权重时全局赢**，与历史记录（`docs/audits/2026-09-21-extension-pages-design-conformance.md:97-106`）一致。

**已按共识修好的两处（复核通过）**：`.providerSearch.providerSearch input`（`App.module.css:203-209`，注释写明了 0,1,1 平手与后果「外框里面又出现一个框」）、`.searchRow.searchRow input`（`ToolsPage.module.css:10-13`）。**这两个类名写两遍的 hack 就是本项目对这条坑的既定修法**，没有用 `!important`（全仓库 `!important` 只在 `global.css:269-272` 的 `prefers-reduced-motion` 里）。

**仍在平手失效的 4 处**（我把构建产物里所有「单个类 + 裸 `input`」且位于全局规则之前的规则枚举出来，只有这 4 条）：

| 规则 | 想声明的 | 实际生效 | 影响 |
| --- | --- | --- | --- |
| `PluginHubPage.module.css:21` `.addSource input`（0,1,1） | `width: 260px`；`font-size: --font-size-label`(13) | 全局的 `width: 100%` 与 14px（`global.css:55`） | 「添加订阅源」那个输入框不是 260 宽，而是撑满；同一行的 `<select>`（`:19`，0,1,1 **赢**了裸 `select`）是 13px，于是**同一行两种字号** |
| `ContentPage.module.css:54` `.addRow select, .addRow input` | 同上（select 侧赢，input 侧平手输） | input 保留全局 14px | 内容中心「添加订阅源」一行里 select 13px / input 14px |
| `ContentPage.module.css:68` `.grow input{width:100%}` | `width:100%` | 与全局同值 | 无害，仅属死规则 |
| `ProviderForm.module.css:86` `.keyRename input{flex:1;min-width:0}` | `flex` | 全局不设 `flex`，能生效 | 无害 |

**结论**：这条坑没有「满血复发」，但**修法是逐处手写的、没有机制约束**——4 处里 2 处已经造成「同一行两种字号」这种用户可见的不一致。可行的低成本收口（供总监决策，我不改代码）：把 `global.css` 的表单控件规则改成挂在 `.field-input` 一类显式类上，或给所有手写搜索/内联输入框统一走一个 `.input-plain` 组件；再退一步，至少把上面两处按既有 hack 补成双类名并加注释。

**另一类「被全局接管」的坑这次是干净的**：`PluginHubPage.module.css:40-43` 显式写了 `height:auto; justify-content:stretch; justify-items:stretch; align-content:start; text-align:left`，`ToolsPage.module.css:45` 的 `.toggle` 也显式 `text-align:left` + 关掉全局 hover 底色——两处注释都点名了「不声明就会被全局 `button` 接管」，并给出了当时的实测症状。这两处属**已修且有据**。

### 2.5 错误处理 / 竞态 / 乐观更新

**乐观更新：没有需要回滚的地方。** 全文扫描后，所有写操作都是「await 后端 → 用返回值/重读数据更新界面」：`ProviderForm` 的 `toggleCatalog`/`switchKey`/四个 Key 动作（`ProviderForm.tsx:253-319, 344-350`）、`PluginHubPage.toggleEnabled`（`:216-225`，用返回的 record 替换）、`ContentPage.toggleSource`（`:154-165`，写完 `await load()`）、`SettingsPage.togglePaused`（`:42-46`，用后端返回的布尔）、`CodexConfigPage` 的 apply/restore/coexist（`:159-249`）。**没有「先翻本地状态、失败再回滚」的写法，因此也没有缺回滚的缺陷。** 这符合 `docs/design/05:38`。

**反馈出口**：动作结果统一走 Toast（`src/components/Toast.tsx`，失败档不自动消失 `:31`，最多 3 条 `:29`），页面状态留在页面内（`App.tsx:321-323` 的注释写明了这条口径）。失败档的写法基本齐整。

**丢错误 / 静默（3 处）**

| # | 位置 | 问题 | 判定 |
| --- | --- | --- | --- |
| E1 | `ContentPage.tsx:361` `onClick={() => void client.setContentGithubToken(null).then(() => setTokenConfigured(false))}` | **fire-and-forget，无 `.catch`**：清除令牌失败（例如凭据库锁定）时既无 Toast 也无内联错误，徽章继续显示「已配置」，用户读到的现象是「点了没反应」。同一文件里其它写操作（`:154,167,178`）都有 catch 与 Toast，这一处是漏的 | **P2**（可执行：补一个 catch 走 `showToast(…, 'danger')`） |
| E2 | `PluginHubPage.tsx:75-78` `listSkillTargets().then(...).catch(() => setTargets([]))` | 拉取安装目标失败被静默降级成空列表，界面随后按「没有可用的目标工具」（`error.pluginNoTargets` 一族文案）呈现——**把失败显示成「没有」**，与 `docs/design/05` §2「不做：显示大量不可用品牌卡」同源原则相悖 | **P2** |
| E3 | `ContentPage.tsx:89` `contentGithubTokenStatus().catch(() => setTokenConfigured(false))`、`App.tsx:201` 更新检查、`App.tsx:208-210` 更新结果、`UpdateDialog.tsx:44-47` 进度订阅 | 都是**有意静默且写了理由**的（查不到就是没有按钮 / 进度不是关键路径），我复核后认为可接受，仅登记 | 记录，不计缺陷 |

**竞态与卸载**

- 竞态该有的地方都有，且是**逐页手写**的：`ConnectionPage.tsx:85-106,124` 用 `pendingLoad` 序号做票据（「切换供应商时后返回的旧响应不会覆盖新选择」有独立用例）、`App.tsx:225-229,232-239`、`CodexConfigPage.tsx:87-94`、`SettingsPage.tsx:82-88`、`ProviderForm.tsx:113-118`、`UpdateDialog.tsx:42-50` 都有 `alive/current` 守卫。**历史 P0-1 一类的「旧响应覆盖新选择」未复发。**
- 不一致：**5 个页面在挂载期发起的异步加载没有卸载守卫**——`ContentPage.tsx:87`、`ToolsPage.tsx:54`、`LogsPage.tsx:67`、`PluginHubPage.tsx:73`、`ConnectionPage.tsx:119`（后者只有竞态票据、无卸载作废）。React 18 已不再为此报警，所以没有可见症状；但同一份代码里两种写法并存，属**P2**（统一加守卫或统一由 store 承接）。
- **幂等/连点**见 §2.1 末段。

### 2.6 构建体积与分包（chunk 告警的结论）

**先回答最关心的那条：没有把 lucide 全量打进来。**

- `src` 里从 `lucide-react` 具名导入了 **57 个**图标；`node_modules/lucide-react/dist/esm/icons` 下有 **3502** 个文件 / **14 MB**。
- 我用临时配置（`outDir=/tmp/dist-chunk-report`，未改仓库、未覆盖 `dist/`）跑了一次带 `manualChunks` 的构建，得到**实测**分块（gzip 用 `gzip -c` 量）：

| chunk | 原始 | gzip | 说明 |
| --- | --- | --- | --- |
| `index-*.js`（业务代码） | 329.97 kB | 93.28 kB | 全部页面、组件、契约、字典（1147×2 条）都在这里 |
| `react-*.js` | 143.33 kB | 45.92 kB | react + react-dom + scheduler |
| `radix-*.js` | 38.67 kB | 13.08 kB | 只有 `@radix-ui/react-dialog` |
| `lucide-*.js` | **25.01 kB** | 5.42 kB | 57 个图标，tree-shaking 有效 |
| `tauri-*.js` | 15.10 kB | 3.85 kB | `@tauri-apps/api` 用到的部分 |
| 合计 JS | 552.08 kB | 161.55 kB | 与章程的 553.08 / 161.43 一致（差异来自分包边界） |
| `index-*.css` | 101.33 kB | 14.91 kB | 单文件 |

- **懒加载：没有。** 全仓库没有 `React.lazy` / 动态 `import()`（实测 grep 无命中），1707 个模块全部静态进单一 entry；页面也都是 `App.tsx:8-25` 顶部静态 import。
- 产物干净：`src/dev/visual-fixture.tsx` 与 `visual.html` **不在生产产物里**（实测：`dist/` 只有 `index.html` + assets；`grep -c "视觉多模态模型" dist/assets/index-*.js` = 0）。夹具只在 `vite dev` 下可用，这也解释了设计审核员为什么必须走 `dev` 服务器。

**结论与建议**：告警的成因是「React 143 kB + 业务 330 kB 同处一个 chunk」，与图标库无关；且这是一个**从本地打包资源加载的桌面 WebView 应用**（Tauri 把前端产物打进包内，不存在跨网络的按需下载），懒加载/分包对用户几乎没有收益，只会引入 Suspense 边界与加载态。**判定：不值得为消掉这条告警做懒加载或路由级分包。** 若要收口，成本最低的是二选一：在 `vite.config.ts` 里加 `build.chunkSizeWarningLimit`（例如 700）并写明理由；或加一条 `manualChunks` 把 react/radix 拆成 vendor（总量不变，只是读起来清楚）。两者都是「让告警说真话」，不是性能优化——**这一点建议写进 `docs/development/02-testing-and-release.md`，否则下一轮还会有人从这条告警出发去猜 lucide**。

---

## 3. 「核心未实现却可操作」的控件（假开关）

按章程 §四，发现即 P1。本轮**找到 1 处**，另有 1 处同源（未验证）与 2 处「静态声明」需登记。

### P1（假开关）：内容中心「设置 GitHub 令牌」走 `window.prompt`，真机很可能点了没反应

- 落点：`src/features/content/ContentPage.tsx:178-180`，`const value = window.prompt(t('content.token.prompt')); if (value === null) return;`。这是**设置/更新**令牌的唯一路径（清空走另一条 API，`:361`）。
- 为什么这是假开关：整个应用是 Tauri + WKWebView（`src-tauri/Cargo.toml:12` 只有 `tauri` 与 `tray-icon`，`package.json` 没有 dialog 插件）。**实测依赖源码**：`~/.cargo/registry/src/*/wry-0.55.1/src/wkwebview/class/wry_web_view_ui_delegate.rs` 里的 `WKUIDelegate` 只实现了 `webView:runOpenPanelWithParameters:`（文件上传）、`requestMediaCapturePermission…`、`createWebViewWithConfiguration…`（新窗口），**没有** `runJavaScriptAlertPanelWithMessage:` / `runJavaScriptConfirmPanel…` / `runJavaScriptTextInputPanelWithPrompt…`。WebKit 在该方法未实现时不会显示输入面板，`prompt()` 拿到 null；而代码把 null 当作「用户取消」直接 return。
- 后果：用户点「设置令牌」→ 什么都没发生、没有报错、没有任何提示；而按钮文案与状态徽章都在宣称这个动作可用。这既是「核心能力不可用却摆成可操作」，也是 §2.5 E1 的同族问题（无反馈）。
- **未验证**：我没有真实 WKWebView 可跑，所以「prompt 返回 null」是按依赖源码 + WebKit 语义推出的**静态判定**，未在真机复现。请在真机上点一次这个按钮取证后再定性；若确认，修法是把这一处改成应用内 `Dialog`（仓里已有该组件），与其它所有输入一致。
- 同源的 P2：外部链接有**两套打开机制**——`client.openReleasePage`（走 Rust 拉起系统浏览器，`src-tauri/src/commands.rs:1097-1102`）被更新弹窗使用，而内容中心/工具管理/插件中心用 `window.open(url,'_blank','noreferrer')`（`ContentPage.tsx:374,406`、`ToolsPage.tsx:89`、`PluginHubPage.tsx:399`）。wry 实现了「新窗口」回调，所以 JS 的 `window.open` 在真机上是被接管的，**能不能落到系统浏览器未验证**；两套机制并存本身就该收口。

### 登记（不算假开关，但属于「界面上说的和核心结论不同源」）

1. **Chat Completions 的「实验」标注是静态文案，不读核心结论。** 两处：`ProviderForm.tsx:429`（表单里选到 `chat_completions` 就显示一段固定说明）与 `App.tsx:398`（供应商详情行显示 `providers.chatPending` = 「Chat Completions · 待验证」）。核心真正给出的结论是应用计划里的 `warning.chatAdapterExperimental`（只在差异弹窗警告区出现）。今天两者结论一致（工具调用门禁本来就没实现），所以**不构成谎报**；但一旦门禁落地，这两处静态文案必须改成读核心结论，否则会与计划互相矛盾。判 **P2**（另：`providers.chatAdapterExperimental` 与 `warning.chatAdapterExperimental` 是两条独立的字典键，内容相关但用途不同，允许并存，不要合并成一处）。
2. **输入能力 PDF / 视频：合格。** `src/features/models/policy.ts:20-24` 的 `BLOCKED_INPUT_KINDS = {'pdf','video'}` 与核心 `crates/switch-core/src/domain/capability.rs:93-95` 的 `is_host_projectable()`（`Text|Image|Audio`）**逐一对应**（audio 两边都允许）。界面上它们是「可见、灰态、点不动」（`ModelEditorPage.tsx:152-158`、`CheckCell.tsx:29-31`），符合需求 R22「可见但不启用」；唯一欠账是「为什么」只在 `title` 里（§2.3 A3）。
3. **`已验证路由`：界面上没有入口，也没有假替代品。** 全仓库没有 `routeVerified` 一类标识（实测 grep 只命中 PRD 与设计文档的表述）。概览「连接状态」卡的三行里，前两行是网关事实与探测提示，第三行是「Codex 加载」——用的是 `overview.loadStateVerified`（`OverviewPage.tsx:34-35`，由 `summary.stage === 'verified'` 驱动），并明确注释「不猜宿主行为」。**没有把「已验证路由」摆成可用能力**，与历史结论一致，只是它本身仍无处可做（属需求面，不重复计）。

---

## 4. 与历史审计的对照

| 历史条目 | 本轮结论 | 证据 |
| --- | --- | --- |
| **P0-1** 切换供应商后模型表不跟随（`2026-09-20-audit-synthesis.md` §2 P0-1） | **已修复，且未复发**（复核通过） | 修法是派生而非同步：`ModelsPage.tsx:50-51`；且有「换作用域清勾选」`ModelsPage.tsx:63` 与之配套 |
| **P0-4** 首次接入向导中途消失 / 死状态 `onboardingForced` | **已修复**（本轮复核）：向导由显式 `onboardingOpen` 驱动（`App.tsx:120,156,221-223`），步骤提升到宿主（`App.tsx:125`），设置页有唯一的重开入口（`SettingsPage.tsx:151`） | 源码复核 + 用例「保存第一个供应商之后向导不消失」「退出向导后可以从设置页重新打开」 |
| **P1-11** 许可证文案自相矛盾（MIT vs 保留所有权利） | **已修复**（复核）：`settings.licenseValue` 中英均为 MIT 口径 | 字典抽查 |
| **P1-5 / P1-6** 控件尺寸、弹窗三档（480/640/800） | 复核：`Dialog.module.css:15-17` 三档齐备且 `.normal` 已存在（原来的「类不存在」已修） | 源码复核 |
| **批次 D** 「图标尺寸只剩五档 12/14/16/18/20（+26/28）」（`2026-09-20-audit-synthesis.md` §4.7） | **部分修复，源码里仍有越界值**（新增证据，但我把归属留给设计审核员，不重复计 P1）：`grep -rhoE "size=\{[0-9]+\}" src` 实测分布为 16(33) / 14(29) / 18(28) / **15(12)** / **13(5)** / 12(5) / 20(3) / 26(1) / 28(1) / **11(1)**；越界值集中在三个扩展页——`ToolsPage.tsx:119,235,243,248`、`PluginHubPage.tsx:268,273,278,301,400,416,444`、`ContentPage.tsx:220,224,234,302,305,344,378`（11 是 `ExternalLink`）。`docs/design/02-icons-type-and-copy.md:35-46` 明确「其余值（11、13、15、17、19、22…）一律不收」。**含义**：批次 D 的「只剩这几种」是在当时量过的六页上成立的结论，不是全源码结论 |
| **批次 B** 「i18n 守卫扩到桌面壳（`src-tauri`）」 | **成立，但不够**：`src-tauri` 确实已扫（`i18n.test.ts:82`，实测收 10 键），**推翻章程里「只扫 crates」的猜想**；真正漏的是前缀白名单（§2.2） |
| **`2026-09-20-design-conformance.md` §6.4** 「Escape / 焦点返回 / 脏表单询问：合格」 | **复核仍合格**（`Dialog.tsx:34-41` + `App.test.tsx:29-43`）；本轮**新增**的是「破坏性行操作后无焦点管理」（§2.3 A1，历史未登记） |
| **历史 P2** 「术语泄漏：目录 / 别名 / 实例 / 重新加载 等实现概念进了界面」 | **仍存在，且有一处更严重**：messageKey 原样上界面（§2.2）。按同一口径（实现细节泄漏）历史定为 P2；我把「键泄漏」定为 **P1**（§4.1 负面清单已把这一类列为已整修的缺陷类型，且可造成非中文/非英文的裸标识符），若总监认为应与术语泄漏同档，请下调并记明 |
| `2026-09-21-extension-pages-design-conformance.md` R1（搜索框内外两层框） | **已修复且修法一致**：`.providerSearch.providerSearch input`（`App.module.css:203-209`）与 `.searchRow.searchRow input`（`ToolsPage.module.css:10-13`）都用「类名写两遍」并有注释；**新增**：同类写法还剩 4 处未收口（§2.4） |
| `2026-09-20-audit-synthesis.md` §2 P2「测试输出有 act(...) 警告」 | **仍存在**（实测 `npx vitest run` stderr 仍打印多条 `act(...)`，来源指向 `Toast.tsx` 与 `CodexConfigPage.tsx`），不重复计 |

---

## 5. 未验证项

以下是本轮**没有**验证、后续文档不得当作已通过的：

1. **真实 WKWebView 运行时差异**：`window.prompt` 是否真的静默返回 null（§3 P1 的定性前提）、`window.open` 是否落到系统浏览器、`client.openReleasePage` 与 `window.open` 的行为差异、Clipboard API 在 WKWebView 下的权限行为（`ModelsPage.tsx:255`、`ConnectionPage.tsx:178`、`ToolsPage.tsx:240` 都走 `navigator.clipboard`，且都是可选链调用，失败时的表现未测）。
2. **真机键盘 / 焦点行为**：A1 的焦点丢失、Tab 顺序手工走查、`RowMenu` 的方向键在真机的表现。本环境的按键注入无法推进焦点（历史审计已记录同一限制：`2026-09-19-design-conformance.md:138-139`）。
3. **读屏软件（VoiceOver）实际朗读**：`role="tab"` A2、`CheckCell` 的 `title` A3 的实际朗读结果未测（属原生/真机层）。
4. **真实上游 / 真实插件仓库下的前端反馈**：内容中心抓取失败、插件安装部分失败等界面的真实性依赖真实网络与凭据，本轮只读代码与用例。
5. **未在真机复现 messageKey 泄漏**：本机装了 Codex CLI 且 `config.toml` 存在，`blockedReasonKey` 为 null，所以我只能给出「源码 + 字典 + 构建产物一致」的静态判定（§2.2）。
6. **200% 缩放 / 最小窗口**：属设计审核员口径，本轮未量。
7. **`pnpm build` 我一次都没重跑**（dist/ 保持 CI 的产物）；§2.6 的分块数字来自 `/tmp` 的临时构建，它会产出与正式构建边界不同的 chunk 名，**总量与 gzip 口径可比，但不要引用那串 hash 文件名**。

---

## 6. 问题清单（按章程 §四 口径）

### P1（本轮必须修）

1. **【i18n / 明文规范】messageKey 原样上界面 + 守卫盲区**
   `CodexConfigPage.tsx:364`（裸拼 `selected.blockedReasonKey`）、`CodexConfigPage.tsx:342-345`（`t()` 缺键时回退成键本身）；键值来自 `detect.rs:244,246`、`capability.rs:198,202,206`；字典里都没有（实测各 0 命中）；守卫 `i18n.test.ts:32,79,82` 的前缀白名单不含 `instance`/`capability`，`i18n.locale.test.tsx:19` 的同名正则也不含。修法顺序见 §2.2（先扩 `CORE_PREFIXES`/`DYNAMIC_KEYS`，再补字典，再改渲染）。
2. **【假开关】内容中心「设置 GitHub 令牌」= `window.prompt`**
   `ContentPage.tsx:178-180`；真机极可能点了无反应（wry 0.55.1 无 prompt 面板，§3）。**若总监要求先取证，本项在真机点一次即可定性。**
3. **【a11y / 明文规范】破坏性行操作后无焦点管理**
   `docs/design/05-patterns-and-accessibility.md:45` 明文要求；`ModelsPage.tsx:103,121-140,268-273`、`ProviderForm.tsx:353-364` 无任何 `focus()`；全仓库 6 处 `.focus()` 无一处涉及列表变更。

### P2（记入待办）

4. **D1** 「有没有新版本」两份真值：`App.tsx:148,200-202` vs `SettingsPage.tsx:30,62`。
5. **D3** 概览「模型调用」写死「未测试」、不读 `connectionStore`：`OverviewPage.tsx:170`（消费方只有 `ModelsPage.tsx:59`）。
6. **D2** 设置页镜像 `gateway.paused`：`SettingsPage.tsx:39-40`。
7. **幂等键两处实现**：`PendingApplyBar.tsx:150-152` 重复定义了 `idempotency.ts:1-8` 的函数（注释里明说必须同源）。
8. **E1** 清空 GitHub 令牌 fire-and-forget 无 `catch`：`ContentPage.tsx:361`。
9. **E2** 安装目标拉取失败被显示成「没有」：`PluginHubPage.tsx:75-78`。
10. **CSS 平手失效 2 处（有可见后果）**：`PluginHubPage.module.css:21`（`width:260px` 死掉 + 同行 select 13px / input 14px）、`ContentPage.module.css:54`（同行 13px / 14px）；另 2 处无害（`ContentPage.module.css:68`、`ProviderForm.module.css:86`）。
11. **卸载守卫不一致**：`ContentPage.tsx:87`、`ToolsPage.tsx:54`、`LogsPage.tsx:67`、`PluginHubPage.tsx:73`、`ConnectionPage.tsx:119` 无守卫，其余 6 处有。
12. **`role="tab"` 缺 `tabpanel`/方向键**：`SegmentedTabs.tsx:31-43`。
13. **禁用能力的原因只在 `title`**：`CheckCell.tsx:22,29` + `ModelEditorPage.tsx:154`（PDF/视频）。
14. **核心中文自由文本进英文界面 + 用中文子串做语义判定**：`apply.rs:658` ↔ `ApplyConfirmDialog.tsx:88`。
15. **Chat Completions「实验」是静态文案**：`ProviderForm.tsx:429`、`App.tsx:398`（门禁落地后必须改为读核心结论）。
16. **外部链接两套打开机制**：`window.open`（`ContentPage.tsx:374,406`、`ToolsPage.tsx:89`、`PluginHubPage.tsx:399`）vs `client.openReleasePage`。
17. **图标尺寸越界值 18 处**（列表见 §4 的对照行）：归属设计审核员，本轮只提供源码侧清单与实测分布。
18. **`CodexConfigPage.tsx:344`** 的三元两支相同（无意义表达式，说明该处缺文档保证）。

### 供总监决策的一条（不是缺陷）

**chunk 告警**：建议不改代码，而是把「产物 553 kB 的成因是 React + 业务代码、lucide 只占 25 kB、产物无动态 import，桌面 WebView 不做懒加载」这条事实写进 `docs/development/02-testing-and-release.md`（或加 `chunkSizeWarningLimit` 并写明理由）。否则这条告警会被反复重新发现并误判成图标库问题。

---

**报告结束。** 六项判定已逐条给出证据形态（命令输出 / `文件:行` / 实测数字）；三处 P1 中两处为静态判定并已标明取证方式，第三处（假开关）已给出真机一步取证路径。
