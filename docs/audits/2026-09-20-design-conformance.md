# Switchelp 设计规范符合性审计（本轮：字阶、图标、控件一致性、弹窗与缩放）

日期：2026-09-20（第二轮，复核 + 加细）
范围：`src/**` 全部界面表面（六个侧栏页、首次接入向导、整页模型编辑器、供应商弹窗新建/编辑/更多菜单、
获取可用模型弹窗、添加/编辑模型弹窗（含高级配置展开）、删除模型确认、删除供应商确认、批量删除确认、
应用到 Codex 差异确认、重启 Codex 确认、待应用条、批量操作条、日志页），`src/styles/**`、各 `*.module.css`
方法：**全部结论来自实测**——Playwright 驱动本机 Chrome 渲染 `http://localhost:5173/visual.html`，
页面内读 computed style、按 alpha 合成后算对比度、量 bounding box；字体用 CDP
`CSS.getPlatformFontsForNode` 读**真实渲染字体**（不是拿 token 名猜）。凡未实测的一律进「未验证项」。
基线：工作区当前分支，dev server 已在 5173 运行（strictPort，复用未新起）。

---

## 0. 结论摘要

**上一轮的硬伤确实修掉了，但「再看好几遍还是有细微问题」的根因没有消除：字号对了、行高没对；
图标有 16/18 也有 14/15/17/20；同一语义的开关两种尺寸；间距大量落在 4/8/12/16/24/32/48 之外。**

先说通过的部分（这些都是量出来的，不是感觉）：

- **对比度：浅色主题只剩 1 处不合格**（状态栏分隔符 4.09:1）。上一轮基线记录的 4 处已全部修掉，
  复测数字：主按钮白字 5.93、accent 链接 5.93、accent 淡底小标签 5.19、选中行副标题 5.88（浅）/6.66（深）。
- **深色主题 8 个表面 0 处不合格。**
- **弹窗骨架完全合格**：底栏走 Dialog `footer` 槽（`body`→`footer` 间距 0），头部与底栏之间**没有**多余分隔线
  （确认框两条 1px 边线相邻在同一条 y 上，不再隔 24px），正文滚动 338px 底栏位置纹丝不动（Δ=0），
  Escape 关闭、焦点回到触发控件、脏表单先问「继续编辑 / 放弃修改」且默认焦点是「继续编辑」。
- **中文没有掉到后备字体**：CDP 实测 CJK 字形一律 PingFang SC，等宽元素是 PingFang SC + Menlo 混合，
  没有宋体类后备（memory 里记的那条已不成立）。
- **图标与文字对齐合格**：78 组图标+文字的中心线最大偏差 **0.5px**。
- **图标按钮全部有中文 accessible name**（23 个表面全跑，`iconButtonsWithoutLabel` = 0），点击区 32×32。
- **无正 tabindex、每页恰好一个 `<h1>`、表头 `scope="col"` 全齐**（46 次审计全为 0 违规）。
- **最小窗口 960×640 无横向滚动**，侧栏收窄到 184px，主内容内边距 24px。
- **模型编辑器：只有字段区滚动、底栏钉在卡片底部**（字段区 scrollHeight 1018 / clientHeight 407，底栏在
  1280×640 下不滚动即可见）。
- **编辑器打开时侧栏导航已修好**：干净态直接切页（h1 变「日志」），脏态先弹确认再切。

问题按严重度：

| 级别 | 数量 | 一句话 | 章节 |
| --- | --- | --- | --- |
| P0 | 1 | 切换供应商后，模型表还显示上一家的模型（详情卡标题已经变了） | §2 |
| P1 | 9 | 浅色状态栏 4.09 对比度；200% 缩放 7/8 页横向滚动；按钮行高 22；行内按钮 12px；字阶行高漂移 51%；图标尺寸 4 种越界；同语义开关两种尺寸；确认弹窗 720 不是 480；档位删除按钮 24×32 | §3.2 §4.1 §5.1 §5.2 §5.3 §5.4 §6.1 §7 §8.1 |
| P2 | 7 | 间距阶外值成片（按钮 14px、gap 10px）；间距魔数；复选框两种画法；表/标题字号；窄窗表格不折叠；待应用条遮内容；预设仍未实现 | §3.3 §5.5 §8.2 §9.1 §9.2 §10 §13.2 |

**修起来最划算的三条**：§2（一行 effect）、§6.1（一行 `width="narrow"`）、§3.2（把 12/13px 的字号声明
换成 token，行高自动跟上）。

---

## 1. 验证方法（可复现）

```bash
npm run dev                      # 5173 是 strictPort，已在跑就复用
# 浏览器打开 http://localhost:5173/visual.html（视觉夹具，合成数据，不碰真配置）
```

本轮实际跑的批次（本机 Chrome，headless，`--no-proxy-server`）：

| 批次 | 覆盖 | 次数 |
| --- | --- | --- |
| 页面内审计（`.zcode/skills/design-conformance/scripts/in-page-audit.js`） | 23 个表面 × 深/浅两套主题，root = `main` 与 `body` 各一次 | 92 |
| 字体/图标/控件/间距全量扫描 | 8 个表面 × 两套主题 | 16 |
| 弹窗结构探针 | 10 类弹窗 × 两套主题 | 20 |
| 响应式 | 10 个表面 × {960×640、200% 缩放(480×320@dsf2)、1280×840 的 200%(640×420)} | 30 |
| 定点复测 | 对比度逐元素比值、CDP 平台字体、开关/档位/复选框尺寸、脏表单键盘路径 | — |

两处相对 SKILL.md 的加固（都是为了不漏掉这一轮的发现）：

1. 每个页面除 `main` 外**额外以 `body` 为 root 跑一遍**。原因见 §14：审计脚本用
   `root.querySelectorAll('*')` 取「父容器」，**root 自己的直接子元素永远不会被两两比较**，
   于是「main 顶层的两个块互相重叠」这类问题在 `main` 口径下必然漏报（§10 的待应用条遮挡就是靠
   `body` 口径才看见的）。
2. 字体判定改用 CDP `CSS.getPlatformFontsForNode`。宽度差分法在本机测不出结论：`--font-mono` 与
   通用 `monospace` 渲染「中文测试abc」分别是 81.3 / 81.22px，差 0.08px，无法区分。

---

## 2. 【P0】切换供应商后模型表不跟着换

**现象**（供应商与模型页）：点左侧另一家供应商，右侧详情卡的 `<h2>` 换了，**模型表里的行没换**。

**实测**（同一会话连续三步，1280×840，深色）：

```
初始（示例供应商 A 选中）    h2 = ["示例供应商 A"]
                            rows = ["视觉多模态模型 vendor/vision-flash …", "deepseek-v4.1 vendor/reasoner-pro …"]
                            列表选中态 = [true, null]
点第 2 行（Example Provider B）h2 = ["Example Provider B"]           ← 标题换了
                            rows = ["视觉多模态模型 vendor/vision-flash …", "deepseek-v4.1 vendor/reasoner-pro …"]  ← 还是 A 的
                            count = "2 项（共 4 项）"
                            列表选中态 = [null, true]                 ← 高亮也换了
```

也就是说：**界面顶部宣称你在看 Example Provider B，正文列的是示例供应商 A 的两个模型**；
Example Provider B 自己的「轻量快速模型 / 长文本模型」在这一页**根本看不到**（也无法编辑、删除）。
副作用：因为 A 的两个模型都已纳入目录，行内「更多 → 删除模型」一直是 disabled 的（`title="先移出目录才能删除"`），
用户在 B 的详情卡里永远点不到可删的模型。

**代码出处**：`src/features/models/ModelsPage.tsx:44`
`const [providerFilter, setProviderFilter] = useState(providerScope ?? 'all');`
——`useState` 的初值**只在挂载时生效一次**，而 `App.tsx:275` 的
`<ModelsPage … providerScope={selectedProvider.id} embedded />` 在切换供应商时组件并未重新挂载，
`providerScope` 只被当作初值、没有同步的 effect，于是过滤条件永远停在第一家供应商。

**规范出处**：`docs/design/04-pages-and-flows.md:61`——「点击一行进入列表/详情双栏；详情页只放事实……
**与该供应商的模型表**」；`03-components.md:16`（Checkbox 一行里的原则「同一件事只有一个入口」）；
`05-patterns-and-accessibility.md:24`（模型列表失败要保留已有模型，不能显示误导内容）。

**影响**：这是本轮唯一的 P0——不是观感问题，是**内容错误**：用户以为在管理 B，实际看到的是 A 的模型；
B 的模型没有任何入口。删除/测试/编辑都可能作用在错误的供应商上。

**修法**：给过滤条件加同步——`useEffect(() => { if (providerScope) setProviderFilter(providerScope); }, [providerScope]);`，
或在 App 侧写成 `<ModelsPage key={selectedProvider.id} …>` 让它随供应商重挂载。两种都只动一行；
建议前者（重挂载会丢掉表格内的搜索/排序状态）。

---

## 3. 字体、字阶、行高与中文回退

### 3.1 中文回退：**已修好**（实测通过）

**现象**：memory 记录「等宽栈缺中文会掉宋体」。
**实测**：CDP `CSS.getPlatformFontsForNode` 逐元素读真实渲染字体。8 个表面里含中文的自有文本节点，
CJK 字形一律落在 **PingFang SC**；等宽元素（`span.text-mono`、`small`、`dd.text-mono`）渲染为
**PingFang SC + Menlo/Inter** 混合（例：`settings` 页 `span.text-muted`「a1b2c3d4 · 412 字节」=
`PingFang SC(2) + Menlo(15)`），没有出现 Songti / STSong / Arial Unicode 这类后备。
**规范出处**：`docs/design/02-icons-type-and-copy.md:45`（UI 栈含 PingFang SC）、
`:47`（「模型 ID、URL、路径、Token 和延迟使用等宽或 tabular-nums；中文说明仍用 UI 字体」）、
`src/styles/tokens.css:207-208`（等宽栈把系统中文黑体排在通用 `monospace` 之前）。
**结论**：这一条**不再成立**，不用改。

### 3.2 【P1】字阶行高漂移：395 个自有文本节点里 200 个（51%）不在字阶上

**现象**：字号确实回到了字阶（12/13/14/16/24/28），但**行高没跟上**——12px 与 13px 的文字直接继承了
`body` 的 `line-height: 22px`。
**实测**（8 个表面合并，深色，按节点数）：

| 字号/行高 | 节点数 | 规范档位 | 判定 |
| --- | --- | --- | --- |
| 14/22 | 105 | 正文 14/22 | OK |
| **12/22** | **103** | 辅助说明 12/**18** | 行高越界 |
| **13/22** | **74** | 字段标签 13/**20** | 行高越界 |
| 12/18 | 36 | 辅助说明 12/18 | OK |
| 16/24 | 17 | 卡片标题 16/24 | OK |
| 13/20 | 11 + 6 | 字段标签 / 代码字段 | OK |
| **18/22（ls −0.7px）** | **7** | 区域标题 18/**26** | 行高越界 + 字距硬编码 |
| **12/14** | **7** | 辅助说明 12/**18** | 行高越界 |
| **16/22** | **4** | 卡片标题 16/**24** | 行高越界 |
| **12/16、12/19、13/18** | 5 | — | 行高越界 |

合计 **200 / 395 = 51%** 的自有文本节点不在字阶上。具体命中：侧栏副标题「Codex 配置管理」`12/14`
（`src/app/App.module.css:12`）、品牌名 18/22 + `letter-spacing:-.7px`（`App.module.css:10`）、
顶栏与状态栏 `12/22`（`App.module.css:23`、`:110`）、页头说明 `13/22`（`App.module.css:27`）、
表格正文 `13/22`（`App.module.css:62`、`ModelsPage.module.css:31`）、供应商详情网格 `13/22`
（`App.module.css:102`）、设置页 `13/22`（`SettingsPage.module.css:7`）、
`14/20` 的 h3 与 `16/22` 的 monogram。
**规范出处**：`docs/design/02-icons-type-and-copy.md:52-59`（字阶表：区域标题 18/26、卡片标题 16/24、
控件文本 14/20、字段标签 13/20、辅助说明 12/18、代码字段 12–13/20）；`:61` 只规定「正文不小于 12 px」，
并没有放行任意行高。
**影响**：这是「看着都对、就是有点松」的主要来源——同样的 12px 文字，有的 18 行高有的 22，同一页里
节奏不齐；`letter-spacing:-.7px` 还会让中文品牌名比同级文字更挤。
**修法**：把零散写 `12px` / `13px` 的地方换成 `var(--font-size-caption)` / `var(--font-size-label)` 并配
对应 `--line-height-*`（`.text-caption` 已经是 12/18，可供抄写）；`.brand strong` 的字距若确实需要，
提到 `tokens.css` 里成为具名值，不要在组件里写 −0.7px。

### 3.3 【P2】表格单元格与三级标题用了字阶外的组合

**现象**：模型表/日志表/供应商详情/设置页正文是 13px（行高 22），供应商卡里的 `<h3>`「这个供应商的模型」
是 14/20。
**实测**：`main` 内 `<table>` 文本 13px/22px（`ModelsPage.module.css:31`、`App.module.css:62`）；
`h3` 14/20（`App.module.css:41`）。
**规范出处**：`02-icons-type-and-copy.md:53`（卡片标题 16/24）、`:56`（字段标签 13/20）、
`:59`（代码字段 12–13/20）。
**影响**：表格里「上游 ID」和说明文字同字号，扫读时层级只剩字重；卡片里的 h3 比卡片标题小两档，
和 `<h2>` 的落差过大。
**修法**：表格单元格按「数据用等宽 12–13/20、说明用 14/22」二选一；卡片内 h3 回到 16/24。

---

## 4. 图标

### 4.1 【P1】图标尺寸有 4 种越界，另有 1 处图形被压扁

**现象**：图标不是 16/18 两种，实测出现 8 种尺寸。
**实测**（8 个表面，深色；`w x h` : 个数）：

```
8.91x16 : 1    12x12 : 8    14x14 : 17   15x15 : 8
16x16  : 16    17x17 : 22   18x18 : 51   20x20 : 8
```

- 越界尺寸：**14（17 个：管理、查看全部、添加档位…）、15（8 个：去哪儿改、重新检测、重启 Codex）、
  17（22 个：刷新数据、添加供应商、更多操作…）、20（8 个：侧栏品牌 logo）**。
- **被压扁**：`?view=onboarding` 的冲突提示图标 `lucide-triangle-alert`，`width/height` 属性都是 16，
  但实测包围盒 **8.91 × 16**（横向被压掉 44%），父层 `div._conflict_106rg_43`。它是「三角警告」图形，
  压成竖条之后已经不像三角形。
  **代码出处**：`src/features/onboarding/OnboardingPage.module.css`（`.conflict` 内未给 svg `flex-shrink: 0`；
  对照 `OverviewPage.module.css:14`、`CodexConfigPage.module.css:34`、`Toast.module.css:12` 都写了
  `.note > svg / .toast > svg { flex-shrink: 0 }`）。
  **规范出处**：`docs/design/02-icons-type-and-copy.md:33`——「strokeWidth 默认 1.75，16 px 可用 1.8，
  圆头圆角，**不自行拉伸 SVG**。图标 hit target 至少 32 × 32……图案尺寸与点击尺寸分开」；
  尺寸表 `:9-31` 只给 16 与 18 两档。
  **影响**：同一行的按钮图标大小不一（14/15/17 混排），是「说不清哪里不对」的典型；被压扁的图形是明确缺陷。
  **修法**：把动作图标统一到 16、模块导航统一到 18；给 `.conflict` 内的 svg 加 `flex-shrink: 0`。

### 4.2 图标与文字的对齐：**实测合格**

**实测**：78 组「图标 + 同行文字（同一个父元素里的文本节点）」的中心线偏差最大值 **0.5px**
（出现在概览侧栏「概览」项），其余全部为 0。
**规范出处**：`02-icons-type-and-copy.md:33`（图案尺寸与点击尺寸分开）、`05:42`（Tab 顺序符合视觉阅读）。
**结论**：不做修改。

### 4.3 纯图标按钮的点击区与 accessible name：**实测合格**

**实测**：23 个表面全部跑审计，`iconButtonsWithoutLabel` = **0**；`.icon-button` 实测 32×32
（`global.css:45` 从 token 取 `--icon-button-size`）。表内「更多操作 / 编辑 / 测试」按钮实测 32 高、
46–48 宽。
**规范出处**：`03-components.md:10`（IconButton 32/40 方形，accessible name 必填）、
`02:33`（hit target ≥32×32）。
**结论**：不做修改。

---

## 5. 控件一致性（同一语义在不同页面是否长得一样）

### 5.1 【P1】行内按钮与文本按钮字号 12px，规范是 14px

**现象**：主按钮/次按钮是 14px，但表格行内「编辑 / 测试」、概览的「管理 / 查看全部」、表头排序按钮是 12px。
**实测**：

| 控件 | 实测 | 规范 |
| --- | --- | --- |
| `button.primary`（添加供应商等） | 14px / 行高 22 / 高 40 / padding 0 14px | 控件文本 14/**20**，高 40 |
| 表格行内「编辑」「测试」 | **12px** / 高 32 / padding 0 10px | 控件文本 14/20，紧凑高 32 |
| `.text-button`（管理、查看全部） | **12px** / 行高 22 / 高 32 / padding 0 | 控件文本 14/20 |
| 表头排序按钮 | **12px** / 高 32 / 宽 90 | 控件文本 14/20 |

**代码出处**：`src/features/models/ModelsPage.module.css:57`（`.rowActions > button { … font-size: 12px }`）、
`src/styles/global.css:46`（`.text-button { … font-size: 12px }`）。
**规范出处**：`docs/design/02-icons-type-and-copy.md:55`（控件文本 14 / 20 / 500）、
`01-foundations.md:77`（紧凑 32）。
**影响**：同一张表里主按钮 14、行内按钮 12，行内文字比主按钮小一档；上一轮修的「按钮 13px」只是把
13 换成了 14，行内这一批仍是 12。
**修法**：`.text-button` 与 `.rowActions > button` 的字号改 `var(--font-size-control)`；
若担心 32px 高按钮里放 14px 字变挤，按规范把行内按钮提到 40 高，或把 12px 写进字阶表。

### 5.2 【P1】按钮行高 22px，规范 20px

**实测**：`button.primary` 的 `line-height` = **22px**（`global.css:39` 未声明行高，继承 `body` 的
`--line-height-body: 22px`）；输入框/下拉同为 14px 字号、行高 22。
**规范出处**：`02-icons-type-and-copy.md:55`（控件文本 **14 / 20**）。
**影响**：控件文字的行框比规范高 2px，垂直居中看起来偏低；属于 §3.2 那 51% 的一部分。
**修法**：`global.css:39,48` 的 `button` / `input` 规则补 `line-height: var(--line-height-control)`。

### 5.3 【P1】同一个「纳入 Codex 目录」开关，两种尺寸

**现象**：供应商弹窗的模型行用 32×18，模型编辑器与添加模型弹窗用 36×20。
**实测**：

| 位置 | 实测 | 代码 |
| --- | --- | --- |
| 供应商弹窗模型行「把 X 放进 Codex 目录」 | **32 × 18** | `ProviderForm.tsx:390` 传 `size="small"`；`Switch.module.css:8` |
| 模型编辑器「加入待应用的 Codex 模型目录」 | **36 × 20** | `ModelEditorPage.tsx:172` 默认尺寸 |
| 添加模型弹窗「智能配置」 | **36 × 20** | `ModelFormDialog.tsx:115` 默认尺寸 |

**规范出处**：`docs/design/03-components.md:17`——Switch **只有 36×20 一档**。
**影响**：同一件事在三处有两个尺寸；`.small` 变体在规范里没有出处。
**修法**：去掉 `size="small"` 用法，或把它写进 `03-components.md` 并说明何时用。

### 5.4 【P1】「思考档位」的删除按钮 24 宽，低于 32 的点击区

**实测**：`button._remove` 实测 **24 × 32**（`LevelChips.module.css:10`：`width: 24px`）；
同一行的 `.pick` 45×32、`.add` 32×32 合格。
**规范出处**：`docs/design/02-icons-type-and-copy.md:33`（图标 hit target 至少 32 × 32）。
**影响**：档位 chip 里的「×」是全页最窄的点击目标，鼠标需要瞄准；外接鼠标/触控板下容易误点。
**修法**：`.remove` 宽度提到 `var(--control-height-compact)`（32）。注意 `03-components.md:16` 明确禁止
用「透明边框 + 负外边距」把图形撑大——这里应放大 chip 本身而不是内缩图形。

### 5.5 【P2】复选框两种画法

**现象**：表格/诊断/日志页用原生 `accent-color` 复选框（18×18、无边框、无圆角），
模型编辑器与添加模型弹窗用自绘方框（18×18、`border: 1px`、`border-radius: 4px`、勾号是真实元素）。
**实测**：`input[type=checkbox]` 两种指纹——`border=0px, radius=0px, bg=transparent`（表格）；
`border=1px rgb(119,119,131), radius=4px, bg=rgb(24,24,27)`（`CheckCell.module.css:19`，模型编辑器里 7 个实例）。
点击区都合格：表格外层 `.checkHit` **32×32**；CheckCell 的 `label.cell` **104 × 32**（最窄 81×32）。
**规范出处**：`03-components.md:16`（Checkbox 18 图形、点击区由外层撑到 32）——只定义了一种视觉。
**影响**：供应商弹窗里同一屏会出现两种勾选框（模型表 + 高级配置单元格）。
**修法**：让 `CheckCell` 成为唯一实现，表格也用它；或反过来把自绘样式抽成全局
`input[type=checkbox]` 规则。

### 5.6 输入框 / 下拉 / 徽章 / 提示文字：**实测一致**

**实测**（跨 6 个侧栏页 + 编辑器）：

- `input:not([checkbox])`：40 高 / padding `0 12px` / radius 8 / 14px / `--bg-input` / `1px --border-control`
  —— 7 处全部一致。
- `select`：40 高 / padding `0 34px 0 12px` / radius 8 / 14px —— 10 处全部一致。
- `.badge`：**12px / 行高 18 / min-height 22 / padding 2px 8px / radius 999**，实测 66×24 ——
  与规范逐项一致（`global.css:83`；规范 `03-components.md:18`）。
- 主按钮：40 高 / padding `0 14px` / radius 8 / 14px / 字重 500 —— 一致。
- `.field-hint`：**12px / 行高 18**（`global.css:94`），5 个实例全部 12/18 —— 与规范一致（`02:57`）。
- `.skip-link` 的 `z-index` 用 `var(--z-tooltip)`（`global.css:256`，上一轮从 100 改为 token）—— 一致。

---

## 6. 弹窗（Dialog）逐项核对

### 6.1 【P1】确认类弹窗 720 宽，规范是 480

**实测**：`[role="dialog"]` 包围盒宽度——

| 弹窗 | 实测宽 | 规范档位 |
| --- | --- | --- |
| 应用到 Codex 差异确认 | **800** | 800（差异）✓ |
| 重启 Codex 确认 | **720** | **480**（确认）✗ |
| 删除供应商确认 | **720** | **480** ✗ |
| 批量删除确认 | 720 | 480 ✗ |
| 供应商弹窗（新建 / 编辑） | 720 | 640（普通）✗ |
| 添加模型 / 编辑模型 | 720 | 640 ✗ |
| 获取可用模型 | 720 | 640 ✗ |

**代码出处**：`src/components/Dialog.module.css:7` 的 `.dialog { width: min(720px, calc(100vw - 48px)) }`，
第 8 行注释自承「`normal` 的 720 是历史值，保留以免既有弹窗跳宽」；`.narrow`（480）已定义但
确认框没传 `width="narrow"`（`App.tsx:296`、`App.tsx:313`）。
**规范出处**：`docs/design/03-components.md:21`——「Dialog 480/640/800 宽，16 圆角」
（token 也在 `tokens.css:134-136` 定义了三档）。
**影响**：「删除供应商」这种一句话确认框有 720 宽，描述文字一行只占 1/3，右侧大片空白；
三档宽度实际上退化成两档。
**修法**：确认类传 `width="narrow"`（480）；表单类把默认值从 720 改到 `--dialog-width-normal`（640），
然后逐处过一遍换行表现。

### 6.2 底栏走 footer 槽、正文滚动底栏钉底：**合格**

**实测**（1280×840）：`.dialog` 高 **714px** = 视口 840 的 85%（`--dialog-max-height: 85vh` ✓）。
`body` 与 `footer` 间距 **0px**（10 类弹窗全部）；把 `.body` 从 scrollTop 0 滚到 338 之后
footer top 仍为 **574**，Δ=**0**；`body` scrollHeight 777 / clientHeight 439（确实在滚）。
**规范出处**：`03-components.md:21`（Dialog 标题/说明/焦点陷阱/返回焦点）、
`src/styles/global.css:106` 注释（「底栏属于滚动区之外」）、`04-pages-and-flows.md:148`（操作条钉在卡片底部）。
**结论**：合格。

### 6.3 「头部与底栏之间多出一条分隔线」：**已修掉**

**实测**：确认类弹窗（重启 Codex / 删除供应商）`header` 的 border-bottom 下沿 y=441、
`form-footer` 的 border-top 上沿 y=441——**相邻在同一条线上**，两者之间没有任何空白正文带
（上一轮记录的是相隔 24px）。表单类弹窗 header→footer 距离 558px 是因为中间就是正文，属正常。
**规范出处**：`03-components.md:21`（Dialog 结构）、`global.css:106` 注释（底栏在滚动区之外）。
**结论**：修复成立。附一条实测细节：两条 1px 边线相邻，渲染出来是一道 **2px** 的深色带，
比单条线略重；若要更轻，可让 header 在有 footer 槽时去掉 border-bottom。

### 6.4 Escape / 焦点返回 / 脏表单询问：**合格**

**实测路径**（走 App 壳：供应商页 → 页头「添加供应商」）：

1. 打开后焦点落在**第一个输入框**（供应商名称），符合 `05:45`「Dialog 打开聚焦标题或首输入」。
2. 干净态按 Escape → `[role="dialog"]` 计数 **0**，弹窗关闭。
3. 在名称里输入「测试供应商改名」后按 Escape → 出现「有尚未保存的修改，确定放弃吗？」，
   按钮为「继续编辑 / 放弃修改」，**当前焦点是「继续编辑」**（autoFocus），符合 `05:34`
   「不把『放弃更改』放在默认焦点」。
4. 点「继续编辑」→ 弹窗仍在（计数 1），输入内容保留。
5. 再 Escape → 点「放弃修改」→ 弹窗关闭，焦点回到触发控件（页头「添加供应商」按钮），
   符合 `05:45`「关闭返回触发控件」。

**规范出处**：`docs/design/05-patterns-and-accessibility.md:34`、`:45`。

### 6.5 模型编辑器底栏：**合格**

**实测**（1280×640）：底栏 rect `top 534 / bottom 607`，视口高 640，**不滚动即可见**；
页面里唯一可滚动的是 `.form-fields`（scrollHeight 1018 / clientHeight 407）。
**规范出处**：`04-pages-and-flows.md:148`（只有中间字段区滚动，操作条钉在卡片底部）。
**结论**：合格。

---

## 7. 【P1】浅色主题状态栏分隔符对比度 4.09

**现象**：浅色主题下，状态栏两条状态之间的「/」偏灰。
**实测**：`span._separator_` 前景 `rgb(119,119,131)`（= `--border-control` #777783），
合成后底色 `rgb(246,246,247)`（= `--bg-canvas`），对比度 **4.09:1 < 4.5:1**（12px 正文阈值）。
6 个侧栏页 + 首次接入向导**全部命中**（共 7 个表面）；深色主题 0 处；组件级直连视图（无状态栏）不受影响。
**代码出处**：`src/app/App.module.css:113` `.separator { margin: 0 4px; color: var(--border-control); }`
**规范出处**：`docs/design/01-foundations.md:39`——「正文对比度、控件边界和焦点至少按验收检查，
不仅检查色卡值」；`01-foundations.md:26`——`border.control` 的用途是「需要明确识别的**表单边界**」，
不承担文字语义。
**影响**：只有一个字，但它出现在每个页面的右下角；同时是用边界色当文字色，属于 token 语义越界
（上一轮 `global.css:35` 的 `--status-info` 残留是同一类问题，那条已修）。
**修法**：`.separator` 改用 `var(--text-muted)`（浅色 5.84 / 深色 5.28，都合格）。

---

## 8. 响应式与缩放

### 8.1 【P1】200% 缩放：7 / 8 个页面整页横向滚动，侧栏没有抽屉化

**现象**：把最小窗口 960×640 放大到 200%（= 480×320 CSS px 布局宽度）后，整页出现横向滚动条。
**实测**：

| 视口 | 结果 |
| --- | --- |
| 960×640 | `documentElement.scrollWidth == clientWidth`（**0 处横向滚动**），侧栏 184px，主内容 padding 24px |
| 480×320（960×640 的 200%） | `scrollWidth 860 / clientWidth 480` → **溢出 380px**，7/8 个表面命中（仅无侧栏的组件级视图例外） |
| 640×420（1280×840 的 200%） | `scrollWidth 860 / clientWidth 640` → 溢出 380px（溢出量相同，因为是 min-width 造成的） |

溢出源实测：`div._shell` 的 computed `min-width` = **860px**（`src/app/App.module.css:3`），
`.workspace` 被撑到 676px；侧栏在 480 宽下仍是 **184px**，没有变成抽屉。
**规范出处**：`docs/design/01-foundations.md:84`——「200% 缩放时侧栏可抽屉化，主表单单列，**避免水平滚动**」；
`05-patterns-and-accessibility.md:52`——「200% 缩放时表单与提示不遮挡」。
**影响**：200% 缩放下用户必须左右拖动才能看到右半屏；这是规范明确点名的场景。
**修法**：删掉 `.shell` 的 `min-width: 860px`，宽度交给 flex；在 200% 等效宽度下把侧栏收成图标或抽屉
（规范已允许），并给 `.workspace` 保留 `min-width: 0` 让内部滚动容器自己横向滚动。

### 8.2 【P2】960 窄窗下模型表横向滚动 83px，没有折叠为「详情」

**实测**（960×640，供应商页嵌入的模型表）：`.tableWrap` clientWidth **444**，`<table>` scrollWidth **527**，
溢出 **83px**（`overflow-x: auto`，滚一段能看全）；表头为「模型 / 上游 ID、上下文 / 最大输出、
Codex 状态、操作」。
**规范出处**：`01-foundations.md:84`——「模型表格宽列可在窄窗折叠为『详情』，**不能截断唯一辨识模型的 ID**」。
**影响**：不截断 ID 这条守住了（`break-anywhere` + 换行），但规范给的是「折叠为详情」，现在是横向滚动；
960 宽下列表（230px）与详情卡挤在同一行。
**修法**：960–1199 把「上下文 / 最大输出」「Codex 状态」并成一列「详情」，或改由行内详情入口承载。

---

## 9. 间距与魔数

### 9.1 【P2】按钮水平内边距 14px 与 gap 10px 是两个成片的阶外值

**实测**（8 个表面，computed padding/gap/margin 计数）：

| 值 | 出现次数 | 典型位置 | 规范 |
| --- | --- | --- | --- |
| `padding-left / right: 14px` | **71 / 64** | 所有 `button`（`global.css:39`） | 不在 4/8/12/16/24/32/48 |
| `gap: 10px` | **61** | 侧栏导航、供应商卡 `providerItem`、`cardHeader h2/h3`、`.note` | 同上 |
| `padding: 14px`（上下） | 22 / 17 | 供应商卡、空态、`.note` | 同上 |
| `margin-top: 20px` | 12 | 卡片操作区、待应用条、版本列表 | 同上 |
| `margin-bottom: 18px` / `margin-top: 18px` / `margin: 22px` | 9 / 8 / 2 | 卡片头、设置页小标题 | 同上 |
| `margin-top: 2px / 3px / 6px` | 28 / 18 / 14 | 徽章、图标微调、行内按钮组 | 同上 |
| `gap: 6px` / `gap: 14px` | 14 / 9 | 行内操作组、待应用列表 | 同上 |

**规范出处**：`docs/design/01-foundations.md:88`——「间距使用 4 / 8 / 12 / 16 / 24 / 32 / 48……
**禁止同层卡片随意使用 17 / 23 / 29 等间距**」。
**影响**：单个 14px 看不出来，但全站 135 处 14px 内边距 + 61 处 10px 间距，让「间距有没有节奏」
变成了运气问题——这正是「页面上总有或大或小的细微问题」的来源。
**合格的对照**（同一批实测）：`gap: 8px` 199 处、`gap: 12px` 62 处、`padding: 16px` 56 处、
`padding: 24px` 24 处，都在阶上。`tableWrap` 的 `padding: 7px; margin: -7px`
（`ModelsPage.module.css:30`、`App.module.css:61`）是为焦点环留的补偿，属上一轮的定向修复，不算魔数。
**修法**：`0 14px` 改 `0 16px`，或把按钮内边距提成具名 token 并写进规范；`gap: 10px` 归到 8 或 12。

### 9.2 【P2】四个硬编码尺寸 / 字距

**实测与代码出处**：

- `src/app/App.module.css:3` `.shell { min-width: 860px }` —— **860** 既不在间距阶上，也不是 token
  （`--window-min-width: 960px`）。它是 §8.1 横向滚动的直接原因。
- `App.module.css:10` `.brand strong { font-size: var(--font-size-section-title); line-height: 22px; letter-spacing: -.7px }`
  —— 字号接了 token，行高与字距是硬编码（规范 18/26，token 里有 `--line-height-section-title`）。
- `App.module.css:12` `.brand span { font-size: var(--font-size-caption); line-height: 14px }` —— 12/14，规范 12/18。
- `App.module.css:5` `.brandIcon { width: 36px; height: 36px; border-radius: 10px }` —— 36 不在
  `--icon-button-size`(32) / `--icon-button-size-large`(40) 两档里；`border-radius: 10px` 与
  `--radius-nav` 同值但没引用 token。

**规范出处**：`01-foundations.md:88`（间距阶）、`:92`（圆角 token）、`02:52`（区域标题 18/26）、
`03-components.md:10`（32/40 方形）。
**修法**：`.shell` 改用 `var(--window-min-width)` 或直接去掉（见 §8.1）；品牌行高/字距回到 token。

---

## 10. 【P2】待应用条在未滚动时遮住行内控件

**现象**：配好供应商与模型之后，底部常驻的待应用条压在内容上，挡住了当前视口底部那一行的按钮。
**实测**（1280×840，深色，条高 **66px**）：

| 页面 | scrollTop = 0 时被遮住的可交互元素 | 滚到底时被遮住 |
| --- | --- | --- |
| 概览 | 「管理」（text-button）、「去哪儿改」、「查看差异并应用」（primary） | **0** |
| 供应商与模型 | 「选择 deepseek-v4.1」勾选框、「编辑」「测试」「更多操作 deepseek-v4.1」 | **0** |
| 日志 | 「生成预览」、「保存到本地」（primary）、导出清单勾选框 | **0** |
| 设置 | 「暂停新请求」 | **0** |

叠加量：`div._page` / `div._providerLayout` 与条的重叠高度 **66px**（两套主题都是 66；960×640 下为 102px）。
**代码出处**：`src/app/PendingApplyBar.module.css:10`（`position: sticky; bottom: 0`，`margin-top: 20px`）。
**规范出处**：`docs/design/03-components.md:88`——「页面底部**预留高度，不能挡住最后一行或键盘焦点**」；
同文件注释写「**不是浮层**（浮层会盖住最后一行的按钮）」。
**影响**：滚到底时确实不挡（sticky 回到自然位置，实测 0），所以不是死路；但在任何中间滚动位置，
它都会盖住当时贴着视口底部那一行的按钮，且条本身有阴影与描边，看起来像凭空浮上来的浮层。
**修法**：给滚动容器补一条等于「条高 + 间距」的 `padding-bottom` 预留，或改成不吸底、只在滚到底时
出现的普通块（规范文本更接近后者）。

---

## 11. 上一轮基线漂移复核（SKILL.md「已知基线」逐条）

| # | 基线结论 | 现在是否成立 | 本轮实测 |
| --- | --- | --- | --- |
| B1 | 深色主题对比度全部合格 | **仍成立** | 8 个表面 0 处不合格 |
| B2 | 浅色主题 4 处不合格（主按钮白字、accent 链接、accent 小标签、选中行 muted） | **不再成立（已修）** | 5.93 / 5.93 / 5.19，选中行副标题 5.88（浅）·6.66（深），全部 ≥4.5 |
| B3 | `--text-muted` 在 `--bg-selected` 上两套主题都不合格（深 4.11 / 浅 4.38） | **不再成立（已修）** | 选中行的副标题已改用 `--text-secondary`：深 **6.66**、浅 **5.88**；`--text-muted` 出现在 `--bg-elevated` 上是 4.95（深）/5.76（浅） |
| B4 | 按钮/输入字号 13（规范 14） | **不再成立（已修）** | `button` / `input` / `select` 实测 **14px**；但行内按钮与 `.text-button` 仍是 **12px**（新问题，见 §5.1） |
| B5 | `.icon-button` 36（规范 32 或 40） | **不再成立（已修）** | 实测 **32×32**，从 `--icon-button-size` 取值（`global.css:45`） |
| B6 | checkbox 16（规范 18 图形 + ≥32 点击区） | **不再成立（已修）** | 图形 **18×18**；点击区表格 **32×32**、CheckCell 的 label **104×32** |
| B7 | `.badge` 11px 无 min-height（规范 12 / 最小高 22） | **不再成立（已修）** | 实测 12px、`min-height: 22px`、实际 **24** 高（`global.css:83`） |
| B8 | `.field-hint` 行高 20（规范 18） | **不再成立（已修）** | 实测 **12px / 18px**（`global.css:94`，5 个实例全部 12/18） |
| B9 | `.text-button` 与表格行内按钮低于 32 高 | **不再成立（已修）** | `.text-button` **32** 高、行内按钮 **32** 高、排序按钮 **32** 高（字号仍 12，见 §5.1） |
| B10 | `global.css:35` 给 `a` 的 `--status-info` 被 `:39` 覆盖，属残留 | **不再成立（已修）** | `global.css:35` 现为 `a { color: var(--accent); text-decoration: none; }` |
| B11 | 模型编辑器打开时侧栏导航失效 | **不再成立（已修）** | 干净态：点「日志」→ h1「日志」、面包屑「工作空间 日志」、当前页高亮 = 日志、无弹窗；脏态：先弹「继续编辑 / 放弃并离开」，确认后才切页（`App.tsx:158-167`） |

**新增的漂移**（上一轮没记、现在有）：字号对上了但行高没对（§3.2，200/395 节点）；
图标尺寸越界 4 种 + 1 处压扁（§4.1）；按钮行高 22（§5.2）；`.shell min-width: 860px`（§8.1）；
确认弹窗 720（§6.1）；供应商切换后模型表不换（§2）。

---

## 12. 对 `docs/audits/2026-09-20-global-ui-and-dialogs.md` 修复声称的复核

| 声称 | 是否真的修掉 | 证据 |
| --- | --- | --- |
| 1.1 弹窗头部与底栏之间的多余分隔线 | **是** | 确认类弹窗 header border-bottom 下沿 y=441 = 底栏 border-top 上沿 y=441；中间正文高度 0（上一轮记录相隔 24px） |
| 1.2 差异/确认卡片改模态弹窗 | **是** | Codex 配置页点「应用到 Codex」后 `[role="dialog"]` 计数 1，宽度 **800**（= `--dialog-width-diff`） |
| 1.3 计划过期自动重新生成 | **未验证** | 夹具的 `planApply` / `executeApply` 是合成实现，`CONFIG_CHANGED` 路径需要真机 Codex 才能触发 |
| 1.4 字号回到字阶（36 处 10/11px） | **字号是，行高不是** | 各页字号集合实测 = 概览 `{12,13,14,16,24,28}`、其余侧栏页 `{12,13,14,16,28}`、模型编辑器 `{12,13,14,28}`、弹窗 `{12,13,14,18}`——**全部在字阶内**，10/11px 已不存在；但**行高**有 200/395 不在字阶上（§3.2） |
| 1.5 表格 32px 点击区被滚动容器裁掉（`.tableWrap` padding 7px） | **是** | `clipped` 在 23 个表面 × 两套主题下全为 **0**；`ModelsPage.module.css:30` / `App.module.css:61` 的 `padding: 7px; margin: -7px` 在位 |
| 1.6 批量操作条贴住表格（margin-bottom 16px） | **是** | `ModelsPage.module.css:65` `margin: 0 0 16px`；实测批量条底到表格可视顶 **16px**（wrapper 的 −7px 负边距让包围盒间距显示为 9，视觉距离是 16） |
| 1.7 实现 PendingApplyBar | **是，但引入了遮挡** | 6 个页面均出现（条高 66）；遮挡见 §10 |

**结论**：上一轮 7 条声称里 5 条实测成立，1 条部分成立（字号修了、行高没修），1 条本机验不了。
「字号全部落在字阶内」这句话本身是真的——问题是它只检查了字号，没检查行高。

---

## 13. 未验证项（这台机器上验不了，不写推断）

1. **日志「事件详情」弹窗**：夹具 `listDiagnostics` 返回 `{ items: [] }`（`src/dev/visual-fixture.tsx:147`），
   日志页实测只有空态（h3「还没有诊断事件」），`button[aria-label^="查看事件详情"]` count = 0，
   该弹窗渲染不出来。同理「清空日志」按钮因无事件而 disabled，其确认框也进不去。
2. **供应商预设 / 模板的展示**：`src/desktop/transport.ts:35` 的 `listPresets` 仍是 `async () => []`，
   全仓没有任何组件消费它（只有 `contracts/types.ts:70` 的类型定义与 `ProviderForm.tsx:111` 写回的
   `presetId` 字段）。**没有 UI 可查**，因此「展示是否清楚、有没有绑定推荐或充值」无法测量——
   能确定的只有：没有任何充值/推荐入口，因为这个入口整体不存在（不是「没绑定」，是「没实现」）。
3. **`CONFIG_CHANGED` 自动重新生成计划**（上一轮修复声称 1.3）：需要真机 Codex 改写 `config.toml` 才能触发。
4. **macOS 原生窗口外观与拖拽区**：交通灯避让、`data-tauri-drag-region` 的实际拖动、系统标题栏下
   `--macos-traffic-light-reserve` 归零的表现。
5. **真机 Codex 路由与模型选择器**：模型是否出现在原生菜单、选中后是否走对供应商与 Key。
6. **签名与公证安装包**（macOS ARM64 / Windows x64）。
7. **Windows 平台**：滚动条占位（`scrollbar-gutter: stable`）、原生按钮、字体渲染。
8. **真实浏览器缩放（而非等效视口）**：本轮的 200% 用「960×640 的 200% = 480×320 CSS px 视口」等效，
   没有用 Chrome UI 的 zoom；两者对布局的影响等价，但滚动条宽度与亚像素舍入可能不同。
9. **Tab 顺序的手工走查**：自动化只能断言结构性事实（各页无正 tabindex、焦点环规则存在）。
10. **1000 条模型的目录分页/虚拟化**（`05-patterns-and-accessibility.md:58`）：夹具 4 条模型。
11. **`Cmd/Ctrl+S` 的覆盖范围**：实测 `ModelFormDialog.tsx:54` 与 `ModelEditorPage.tsx:64` 已实现；
    供应商弹窗（`ProviderForm.tsx`）与设置页没有。`05-patterns-and-accessibility.md:44` 写的是
    「在表单中保存草稿」，是否覆盖这两处需要产品确认，本轮不判为缺陷。

---

## 14. 审计脚本本身的三处修正建议

跑这一轮时脚本的规则与 SKILL.md 的自述不一致，会分别造成**漏报**与**误报**：

1. **漏报：root 自己的直接子元素不做重叠比较。**
   `overlap` / `touching` 的实现是 `for (const parent of root.querySelectorAll('*'))`，
   而 `querySelectorAll` **不含 root 自身**。所以用 `'main'` 当 root 时，`main` 的各个直接子块
   （页面块、待应用条）之间的重叠永远查不到——§10 的遮挡在 `main` 口径下是 0，换成 `body` 才报出来。
   建议：把 root 自身也放进父容器集合。
2. **误报：`targets` 的 INPUT 豁免只看 `parentElement`。**
   现在的写法是「父元素 ≥32×32 就放过」，而 `CheckCell` 的结构是
   `label.cell(104×32) > span.box(18×18) > input(18×18)`，立即父元素是 18×18，于是 7–9 个
   勾选框被报成不合格；实际点击区是 `label.cell`（104×32），合格。建议：向上找到最近的 `label`、
   带 `for` 的元素或 `[role]` 再判尺寸。
3. **误报：`touching` 的分隔线判定只看相邻两个盒子自身。**
   Dialog 的分隔线画在底栏**内部**的 `.form-footer` 上（`border-top`），而相邻的两个盒子是
   `div._body_` 与 `div._footer_`（`flex-shrink: 0`、自身无边框），于是 10 类弹窗全部被报「紧贴 0 间距」。
   建议：判定时把相邻盒子的一级子元素也纳入 `padding` / `border` 检查。

这三条都不影响本报告已列出的结论（每条都另做了定点实测），但下一轮如果直接采信脚本输出，
会在这三处反复浪费注意力。
