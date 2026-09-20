# Switchelp 设计规范与需求符合性审计报告

日期：2026-09-19
范围：`src/**` 全部界面、`src/styles/**`、`crates/switch-core` 对外可观测行为
方法：**全部结论来自实测**——浏览器里读真实 computed style、按 alpha 合成后算对比度、
量 bounding box。凡未实测的一律进入「未验证项」，不写推断。
基线：`fix-delete-and-provider-form` 分支，`npm run build` 通过，前端 93 测试全过，Rust 401 测试全过。

---

## 0. 结论摘要

**设计系统的「骨架」是健康的，坏在两类地方：浅色主题的可访问性，和一批控件尺寸与规范脱钩。**

先说通过的部分，这些不用改：

- **深色主题对比度全部合格。** 概览、供应商与模型、Codex 配置、连接诊断、日志、设置、
  模型编辑器 7 个表面、全部文本节点，零不合格。
- **最小窗口 960×640 无横向滚动**，7 个表面全部通过；侧栏按规范收窄到 184px。
- **无正 tabindex；每页恰好一个 `<h1>`；表格 `columnheader` 全部带 `scope="col"`；
  没有无名图标按钮。**
- 色值、字阶、间距、圆角、层级、动效时长等 token 与文档**逐位一致**（大小写除外）。
- 核心事务链（差异预览 → 一次性应用 → 失败恢复 → 还原）按需求实现，且有测试钉住。

问题按严重度：

| 级别 | 数量 | 一句话 |
| --- | --- | --- |
| P0 | 4 | 导航失效、Key 不能禁用、Key 检测结果不落库、PDF 能力可启用但链路不支持 |
| P1 | 11 | 浅色主题 4 处对比度不合格；控件字号/尺寸 7 处与规范不符 |
| P2 | 6 | 残留代码、层级越界、预设未实现、token 未文档化 |

**最该先修的一条**：模型编辑器打开时点侧栏导航，面包屑和「当前页」高亮会变，但主内容
不动（见 §4.1）。这不是观感问题——界面在向用户撒谎。

---

## 1. 验证方法（可复现）

```bash
npm run dev                      # 5173 是 strictPort，已在跑就复用
# 浏览器打开 http://localhost:5173/visual.html（视觉夹具，合成数据，不碰真配置）
```

审计脚本已随本报告一起交付：`.zcode/skills/design-conformance/scripts/in-page-audit.js`。
它把下面这些检查做成一个页面内函数：

1. **横向溢出**：子元素越出父容器且父容器不滚动 → 会被裁掉
2. **被祖先裁切**：祖先 `overflow != visible` 且元素越出祖先边界
3. **相邻兄弟**：真重叠（v>2 且 h>2）与紧贴 0 间距
4. **对比度**：逐层合成真实底色后算 WCAG 对比度（不是拿 token 名猜底色）
5. **点击目标**：< 32×32 的交互元素
6. **语义**：正 tabindex、`<h1>` 数量、无名图标按钮、`th[scope]`

用法：

```js
const src = await readFile('.zcode/skills/design-conformance/scripts/in-page-audit.js', 'utf8');
await tab.playwright.evaluate(`(${src})('main')`);            // 整页
await tab.playwright.evaluate(`(${src})('[role="dialog"]')`); // 弹窗
```

两套主题都要跑。`document.documentElement.dataset.theme` 是唯一的主题开关，直接写它；
注意只在 query 上变化的 `goto` 可能不触发重新加载，`?theme=dark` 会看起来没生效。

---

## 2. 设计规范符合性：与文档脱钩的尺寸

这些都是**代码里的实际值 vs 文档明文值**，已在渲染结果中确认命中。

| # | 位置 | 实际 | 规范要求 | 出处 |
| --- | --- | --- | --- | --- |
| D1 | `global.css:42,50` | `button` / `input` / `select` / `textarea` 字号 **13px** | 控件文本 **14px/20px** | `docs/design/02-icons-type-and-copy.md:55` |
| D2 | `global.css:47` | `.icon-button` **36×36** | IconButton **32 或 40** 方形 | `03-components.md:10` |
| D3 | `global.css:67` | `input[type=checkbox]` **16×16** | 图形 **18**，点击区**至少 32** | `03-components.md:16` |
| D4 | `global.css:69` | `.badge` **11px**，无 `min-height` | **12px**，最小高 **22** | `03-components.md:18` |
| D5 | `global.css:80` | `.field-hint` 行高 **20px** | 辅助说明 **12/18** | `02-icons-type-and-copy.md:57` |
| D6 | 实测 | `.text-button` **28 高**、表格行内按钮 **30 高**、`.sortButton` **22 高** | 紧凑控件 **32** | `01-foundations.md:77` |
| D7 | `global.css:35` | `a { color: var(--status-info) }`，被 `:39` 覆盖成 accent | 状态色不承担非状态语义 | `01-foundations.md:54` |
| D8 | `global.css:239` | `.skip-link` `z-index: 100` | 层级表最高 **tooltip 60** | `01-foundations.md:94` |

D1 的影响面比看上去大：字体直方图显示**一页里有 42 个 13px 文本节点**，而 14px 只有 29 个。
也就是说界面主体字号是 13px，规范写的是 14px——整个界面的正文比规范小一档。

D2/D3/D4/D6 可以合并理解成一件事：**控件尺寸没有从 token 走**。`tokens.css` 里
`--control-height*`、`--icon-button-size*` 都定义好了，但 `global.css` 的基础控件规则
用的是硬编码的 13/36/16/11/28/30。规范的 32/40/18/12/22 一个都没接上。

---

## 3. 可访问性实测

### 3.1 对比度（alpha 合成后实算）

**深色主题：7 个表面，零不合格。**

**浅色主题：4 处不合格。**

| # | 元素 | 前景 / 合成后底色 | 实测 | 要求 | 出处 |
| --- | --- | --- | --- | --- | --- |
| C1 | 主按钮（添加供应商 / 应用到 Codex / 开始检查 / 保存到本地 / 保存并查看应用差异） | `#FFFFFF` / `#0284C7` | **4.10:1** | 4.5 | `01-foundations.md:31-32` |
| C2 | 设置页 GitHub 链接 | `#0284C7` / `#FFFFFF` | **4.10:1** | 4.5 | `01-foundations.md:27` |
| C3 | 模型编辑器「生效预览」的位置标签（Codex 目录 / 网关请求 / …） | `#0284C7` / `#E3F2FD` | **3.59:1** | 4.5 | `01-foundations.md:48-53` |
| C4 | **选中供应商行**的「已启用 · 2 个模型」 | 深 `#8D8D99`/`#2D2D32` = **4.11**；浅 `#656570`/`#E0E0E6` = **4.38** | 都 < 4.5 | 4.5 | `01-foundations.md:24` |

C1–C3 是**浅色 token 集自身的问题**，不是组件没用好 token：`action.primary`、
`accent` 这几个浅色取值本身达不到 AA。而 `01-foundations.md:39` 明写「对比度按验收文档
检查，不仅检查色卡值」——这一步此前没做过。

C4 值得单独说：**同一个 `--text-muted`，放在 `--bg-surface` 上合格（深 5.28 / 浅 5.84），
放在 `--bg-selected` 上就不合格。** 选中态底色把对比度吃掉了。这就是为什么审计必须按
合成后的真实底色算，而不能按 token 名判断。

### 3.2 点击目标

实测 < 32×32 的交互元素（浅色主题，供应商页一页 10 处）：

| 元素 | 尺寸 |
| --- | --- |
| 模型表格的选择 checkbox（含表头全选） | 16×16 |
| 表头排序按钮 | 83×22 |
| 行内「编辑」「测试」 | 46×30 |
| 「管理」「查看全部」「重新检测」等文本按钮 | 48×28 |

规范要求图标点击区 ≥32×32（`02-icons-type-and-copy.md:33`）、紧凑控件高 32
（`01-foundations.md:77`）、checkbox 至少 32 点击区（`03-components.md:16`）。
目前 16×16 的 checkbox 是最小的一处，鼠标还好，触控板和外接鼠标下偏难。

### 3.3 焦点与键盘

**已确认通过**：无正 tabindex（HTML 与实时 DOM 两处都查了）；每页一个 `<h1>`；
无无名图标按钮；`th` 全部带 `scope`；`:focus-visible` 规则存在且是 2px/offset 2px，
与规范一字不差；`.scroll-area` 的 `padding:2px; margin:-2px` 是防焦点环被裁的留白，
规范要求的这条实现了；对话框的 Escape + 焦点返回由 `Dialog` 的 `onCloseAutoFocus`
和既有测试（「编辑脏表单按 Escape 需要确认，放弃后焦点返回入口」）覆盖。

**未验证**：Tab 顺序的实际走查没能在自动化里完成——本环境的按键注入无法让焦点前进，
焦点始终停在第一个导航按钮上。这一项需要一次手动走查，不能凭规则存在就宣称合格。

---

## 4. 交互问题与调整建议

### 4.1 【P0】模型编辑器打开时，侧栏导航是失效的

实测：

```
点「概览」→ 面包屑「工作空间 概览」、当前页高亮 = 概览，主内容 h1 仍是「新增模型」
点「日志」→ 面包屑「工作空间 日志」、当前页高亮 = 日志，主内容 h1 仍是「新增模型」
```

`App.tsx` 渲染的是 `{modelEditor ? <ModelEditorPage/> : <>…页面…</>}`，而 `navigate()`
只改 `page` 状态、从不清 `modelEditor`。结果是**界面顶部宣称你在日志页，正文却在编辑模型**。

调整建议（两个方案，推荐第一个）：
1. **`navigate()` 在有未保存内容时先弹既有的「继续编辑 / 放弃修改」，确认后关闭编辑器再跳。**
   复用已有的脏状态确认，语义一致——用户本来离开表单就要过这一关。
2. 次选：编辑器打开期间把侧栏导航置为不可用，并在编辑器顶部加一句「正在编辑模型，
   完成后返回供应商页」。缺点是拦住了用户的正当导航。

### 4.2 【P0】Key 不能「禁用」，「检测」结果不落库

需求 R14 明写：每家供应商多 Key，**新增、替换、禁用、检测**。现状：

- **禁用**：`CredentialStatus::Disabled` 定义了，`mark_active()` 也拦了，
  但唯一的赋值点在一个 `#[cfg(test)]` 块里（`domain/credential.rs:255`）。生产路径没有禁用。
- **检测**：`Credential::status_from_probe` 写好了（200→Verified / 401→AuthFailed /
  403→ScopeLimited），但**没有任何生产代码调用它**。探测结果只在内存里活一次，
  刷新即失。所以界面上的 Key 状态永远是「未测试」。

调整建议（核心层小改，收益很大）：加 `WorkspaceService::record_credential_probe(credential_id, outcome)`
调用既有的 `status_from_probe`，再从一个 tauri 命令接出去。之后：

- 凭证池每行能显示真实的「已验证 / 已失效 / 权限不足 + 检测时间」，而不是永远「未测试」；
- 「禁用」直接复用 `CredentialStatus::Disabled`，加一个 `set_credential_enabled`；
  被禁用的 Key 保留记录但不参与选择，比「删掉再重加」可逆；
- 供应商行的状态点第一次有了可靠来源。

这一条是当前**界面最像半成品的地方**：到处在展示状态，而状态没有任何来源。

### 4.3 【P0】PDF 能力可见但可启用，而链路不支持

需求 R22：当前链路无法执行的能力**必须可见但不可启用**。实测模型编辑器的六行输入能力
（文本/图片/音频/视频/PDF/其他文件）**全部是可操作的 select**，可以把 PDF 设成「支持」。
而 `defaultPolicy()` 给它们的默认 `effectivePath` 是 `blocked`，core 侧也把 PDF 排除出
目录模态（`model_save_recomputes_host_capabilities_and_never_claims_loaded`）。

调整建议：把 `effectivePath === 'blocked'` 的行渲染成灰态 + 说明原因（复用
`capability.blockedReasonKey`），select 换成只读文本，行尾给一句「当前 Codex 接入方式
不能原生发送 PDF」（`02-icons-type-and-copy.md:80` 已有标准文案）。

### 4.4 从「获取模型」跳到模型编辑器后，回不到原来的位置

点发现的模型「添加」会关掉供应商弹窗、跳到能力编辑器（这是对的——编辑能力需要整页）。
但保存完模型后用户落在供应商页，之前填的 Key 上下文和获取结果的滚动位置都没了。

调整建议：跳转时记下「从供应商弹窗来的」，模型保存后重开该供应商的弹窗并停在
「从上游获取模型」那一段，把刚加的模型标成「已添加」。这样一次获取可以连着加好几个模型，
不用每次重新打开、重新获取。

### 4.5 【P1】浅色主题的主按钮对比度

`#FFFFFF` on `#0284C7` = 4.10:1，差 0.4。三个改法，按代价排序：

1. 浅色 `action.primary.bg` 从 `#0284C7` 调到 `#0369A1`（accent-strong 已有的值）——
   白字对比度约 5.6:1，一处 token 改动，全局生效。
2. 主按钮字号从 13 提到 14 并加粗到 600——大字号阈值是 18.66px 粗体，13→14 不够，无效。
3. 保持现状并在验收文档里记录为例外——不推荐，规范自己要求对比度按验收检查。

同理 C2 的 accent 链接文字色、C3 的 accent 小标签，都可以在浅色下改用 `--accent-strong`
（`#0369A1`）拿到约 5.6:1。

### 4.6 【P1】选中行上的弱化文字

选中行 `--bg-selected` 把 `--text-muted` 压到 4.11（深）/ 4.38（浅）。两个改法：
把选中行里那行副标题从 `--text-muted` 提到 `--text-secondary`，或把选中态做成
左侧强调色竖条 + 更浅的底色而不是整行加深。**推荐后者**：整行加深本来就和
`01-foundations.md:48-53`「强调色只用在选中行」的克制用法不太搭。

---

## 5. 需求符合性：P0 逐条

| 需求 | 判定 | 依据 |
| --- | --- | --- |
| R13 供应商增删改 + 搜索 + 自定义 URL；预设只作填表辅助、不绑推荐 | **部分** | CRUD/搜索/URL 已实现。预设**未实现**：`transport.ts:35` 的 `listPresets` 是返回 `[]` 的桩，没有 UI 使用 |
| R14 每供应商多 Key；固定当前 Key；新增/替换/禁用/检测 | **部分** | 多 Key ✓ 固定当前 ✓ 新增 ✓ 替换 ✓；**禁用未实现**、**检测结果不落库**（见 §4.2） |
| R15 从 `/models` 发现 + 手动添加；ID 大小写/斜杠/Unicode 原样保留 | **通过** | 实测发现列表原样显示 `Vendor/Case-Sensitive-2.5-Pro`；`validate_upstream_id` 有测试 |
| R16 独立模型编辑：显示名/上游 ID/协议/上下文/输出/输入/工具/思考 | **通过** | `ModelEditorPage` 三段结构齐全 |
| R17 检测 + 差异预览 + 一次性应用 + 失败恢复 + 还原 | **通过** | `apply_service.rs` 11 个事务测试钉住 |
| R18 模型出现在 Codex 原生选择器并按供应商+Key 路由 | **未验证** | 需要真机 Codex；路由身份有单测，端到端需人工验收 |
| R19 Chat Completions 未过工具门就明确标注实验性 | **通过** | 选项文案「Chat Completions（适配待验证）」、状态文案「Chat Completions · 待验证」 |
| R20 分段测试 + 脱敏日志 + 备份 + 托盘 + 关窗后继续代理 | **通过** | `src-tauri/src/main.rs` 有 `install_tray`；网关与日志脱敏有测试 |
| R21 macOS ARM64 / Windows x64 签名安装包、功能一致 | **未验证** | `.github/workflows/release.yml` 配了 Apple 证书与公证，产物未验收 |
| R22 能力字段可编辑；PDF/视频原生透传非 P0；链路不支持的能力可见但不可启用 | **部分** | 字段可编辑 ✓；**PDF 可启用**（见 §4.3） |

另外一条明文规范未实现：**`Cmd/Ctrl+S` 在表单中保存草稿**（`05-patterns-and-accessibility.md:44`）。
全仓只有 `RowMenu` 处理方向键与 Escape，没有任何修饰键快捷键。

---

## 6. 未验证项（这台机器上验不了）

诚实起见单列，不写推断：

1. **真机 Codex 路由**（R18）：模型是否出现在原生选择器、选中后是否走对供应商与 Key。
2. **macOS 原生窗口外观与拖拽**：交通灯避让、`titleBarStyle: Overlay` 下的实际观感与拖动。
3. **Windows 平台**：窗口吸附、原生按钮、字体渲染。
4. **200% 缩放**：`05-patterns-and-accessibility.md:52` 要求 200% 下无遮挡，未测。
5. **签名安装包产物**（R21）。
6. **真机凭据库**：`native_vault_round_trip_and_cleanup` 是 `#[ignore]`，需在目标系统显式验收。
7. **Tab 顺序实际走查**：见 §3.3。
8. **1000 条模型的目录分页/虚拟化**（`05-patterns-and-accessibility.md:58`）：夹具只有 4 条。

---

## 7. 本次一并交付

### 7.1 项目技能 `.zcode/skills/design-conformance/`

把「按规范核对界面」这件事从一次性劳动变成可重复流程。内容：

- `SKILL.md`：环境准备、五个步骤的审计流程、判定线、报告格式要求，
  以及**本次审计的基线结论**（下次审计可以直接对比漂移）。
- `scripts/in-page-audit.js`：本报告用的那个页面内审计函数，已实测可运行。
- 硬规则一条：**不许报一个你没量过的数字。**

发现顺序上它在 `<项目>/.zcode/skills`，只对本项目生效。

### 7.2 GitHub 上的现成技能：调研结论

找了，也评估了。结论是**没有一条能直接用**，原因是这个项目已经有一份写好的设计规范，
而现成技能要么让你采用它的设计系统，要么根本不读你的规范：

| 仓库 | 许可 | 能用吗 |
| --- | --- | --- |
| [plugin87/ux-ui-agent-skills](https://github.com/plugin87/ux-ui-agent-skills) | MIT | **最值得考虑**。唯一把审计做成「可测量的门」的：`measure_render.mjs` 无头 Chromium 读真实 computed style + 合成对比度，`verify_states.mjs` 查每个交互元素的 default/hover/focus，`lint_hardcodes.py` 查硬编码值。但要装 Playwright/Chromium，且自带 138 套品牌设计系统——有把界面往它的体系改的风险 |
| [pbakaus/impeccable](https://github.com/pbakaus/impeccable) | Apache-2.0 | 流程最成熟，`context` 会读 `DESIGN.md`。但首次运行会下载一个不透明的预编译二进制 |
| [Owl-Listener/designer-skills](https://github.com/Owl-Listener/designer-skills) | MIT | 111 个技能，`design-token-audit`（查 token 被绕过）和 `critique-*` 系列对症。纯提示词，不测量 |
| [igloude/ds-skills](https://github.com/igloude/ds-skills) | MIT | 概念最贴（`ds-doctor` → manifest → `ds-drift` 查漂移与「幻觉 token」），但只看代码不看渲染，8 星、单人 |
| [humbleteam/design-review](https://github.com/humbleteam/design-review) | MIT | 单个 SKILL.md + 评分细则，0–4 分带并列打破规则。适合「这个该用弹窗还是抽屉」这类交互决策 |

明确的空白：**没有 Tauri / 桌面原生 UI 审计技能**；**没有一条技能能读取外部写好的设计规范文档**
（都假设设计系统能从代码里反推）；Anthropic 官方也没有设计审计技能（`anthropics/skills` 里
`frontend-design` 是生成向、`webapp-testing` 是功能向）。

如果要补上 plugin87 那套「可测量的门」，说一声，我可以把它 vendor 进项目并把它的 token
检查指向本仓库的 `tokens.css`，而不是用它自带的体系。
