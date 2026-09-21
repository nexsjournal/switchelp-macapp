# Switchelp 设计与交互体验审核（2026-09-22 专项）

日期：2026-09-22 · 审核员：设计与交互体验审核员（向产品总监汇报）
仓库：`/Users/example/Code/ProjDev/gptswitch-macapp` · 工作区版本 0.3.0
依据：`docs/audits/2026-09-22-product-director-charter.md` §5.2 任务书 + §四 裁决标准
方法：**全部结论来自实测**——headless Chrome（153.0.8010.50，CDP 驱动）渲染 `http://localhost:5173/visual.html`，
页面内读 computed style、按 alpha 合成真实底色算 WCAG 对比度、量 bounding box。
审计函数为项目自带的 `.zcode/skills/design-conformance/scripts/in-page-audit.js`；对比度用 `body` 口径（含侧栏与底栏）。
两套主题各自**冷启动**（`?theme=light` / `?theme=dark`）后测量，不在热切主题后的页面上读数。
凡未实测一律进「未验证项」。

**未改动任何源码。** 环境：dev server（strictPort 5173）已在跑，复用。

---

## 0. 结论摘要

**版面骨架与可访问性底线是健康的；本轮新增 3 条 P1，全部集中在「越出明文规范」而不是「坏掉」。**

先说通过的部分（这些不用改）：

- **几何四类问题基本为零**：9 个表面（概览 / 网关 / Codex 配置 / 工具管理 / 插件中心 / 内容中心 / 连接诊断 / 日志 / 设置）
  × 两套主题 × 1280×720，`overlap / overflow / touching / clipped` 全为 0；960×640 下同样全为 0，
  唯一例外是模型表格横向溢出到可滚动的 `.tableWrap`（可达，属滚动区正常工作）。
- **对比度零不合格**：`body` 口径、两主题、9 个表面、960×640 与 1280×720 全部 `contrast = []`。
  09-19 审计的浅色 4 处不合格（主按钮 / 链接 / 位置标签 / 选中行弱文字）已随强调色改青绿消除。
- **弹窗三档全部落在规范值**：重启确认 **480**（`CodexConfigPage.tsx:447` width="narrow"）、
  供应商弹窗与模型弹窗 **640**、应用差异 **800**（`ApplyConfirmDialog.tsx:103` width="wide"）——
  逐个数与 `01-foundations.md:160` 的 480/640/800 一致，且四个弹窗两主题几何/对比度/点击目标全 0。
- **最小窗口 960×640 无横向滚动**，侧栏收窄到实测 **184px**（`01-foundations.md:149` 要求 184）。
- **语义与键盘**：每页恰好一个 `<h1>`；`positiveTabindex = 0`；无缺名图标按钮；`th` 全部带 `scope`。
- **概览「接入进度」五格等高**：实测 5 格均 **92 / 92**（历史记录的 110 vs 92 已修，`04-pages-and-flows.md:81-87`）。
- **控制尺寸回到 token**：实测按钮字号集合 = `{14px}`（不再是 13px）；输入/选择器高 **40**；
  按钮高集合 = `{32, 40, 44}`（+ 卡片式单元格 92/104，属列表行自动增高）。

问题按严重度：

| 级别 | 数量 | 一句话 |
| --- | --- | --- |
| P0 | 0 | —— |
| P1 | 3 | 侧栏在最小窗口溢出且「设置」入口被压到 22px；图标尺寸越出五档（11/13/15）；供应商搜索图标被 flex 压扁 |
| P2 | 5 | 间距阶回归；「API Key」标签与输入框 0 间距；PDF/视频禁用原因只在 Tooltip；字重小偏差；200% 模拟下长 URL 不换行 |

**假开关：未发现。** 逐项核对结论见 §4。
**交互链路**：Toast「唯一出口」成立，但「应用结果只活 5 秒、重启结论不可回看」**只部分修复**（见 §3）。

---

## 1. 逐条符合性（实测数字）

### 1.1 抽样规则（写清楚，不假装全量）

| 维度 | 覆盖 |
| --- | --- |
| 表面 | 概览 / 网关(providers) / Codex 配置 / 工具管理 / 插件中心 / 内容中心 / 连接诊断 / 日志 / 设置（9 个，全量） |
| 主题 | 浅色、深色各一次**冷启动** |
| 窗口 | 1280×720 与 960×640（`setDeviceMetricsOverride` 设为 1280×720 / 960×640） |
| 弹窗 | 供应商弹窗（新建 / 编辑）、模型弹窗（新建 / 编辑）、模型编辑器（整页）、重启确认（480）、应用差异（800） |
| 200% 缩放 | 按 `05-patterns-and-accessibility.md:52` 只能**视口模拟**（480×320），已标「模拟」，不等同 macOS 原生缩放 |
| 未覆盖 | 原生窗口外观、VoiceOver、真实 Tab 键走查、Windows、真机 200% |

> 说明：`?view=settings` 无法用 `?theme=light` 覆盖主题——`SettingsPage.tsx:34` 在挂载时执行
> `applyTheme(readThemePreference())`，把主题改写回存储偏好（默认 dark）。因此浅色下的设置页
> 是通过先写 `localStorage['gptswitch.theme']='light'` 再导航得到的（实测 `dataset.theme = "light"`）。
> 这不是产品缺陷（真实运行中主题的唯一来源就是存储偏好），但它是**夹具 / 覆盖机制的脆弱点**，记录在此。

### 1.2 几何四项 + 横向滚动

| 表面 | 主题 | 窗口 | overlap | overflow | touching | clipped | 横向滚动 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 9 个表面全部 | 浅 / 深 | 1280×720 | 0 | 0 | 0 | 0 | 无 |
| 9 个表面全部 | 浅 / 深 | 960×640 | 0 | 0（网关页 1*） | 0 | 0 | 无 |

\* 网关页 960×640 的 `overflow` 1 处是 `table` 越出 `.tableWrap` **83px**，父容器为横向滚动区（可达，非裁切）。

### 1.3 对比度

按 alpha 合成后真实底色计算，判定线：正文 4.5:1，≥24px 或 ≥18.66px 粗体 3:1。
两主题冷启动各跑一遍，口径 `body`（含侧栏底栏）。**9 个表面 × 2 主题 × 2 窗口：不合格 0 处**
（`contrast = []`）。四个弹窗同样 0 处。

### 1.4 点击目标

> 判定线：图标点击区 ≥32×32、紧凑控件高 32（`02-icons-type-and-copy.md:33`、`01-foundations.md:155`）。

全局仅 **1 处**不合格，且成因是**同一个**：侧栏底部「设置」入口。

| 表面 | 主题 | 窗口 | 元素 | 实测 |
| --- | --- | --- | --- | --- |
| 9 个表面全部 | 浅 / 深 | 1280×720 | `button._settingsEntry_ku9fy_164` | **183 × 22** |
| 9 个表面全部 | 浅 / 深 | 960×640 | 同上 | **151 × 22** |
| 200% 模拟 | 浅 | 480×320 | 同上 | **47 × 20** |

（勾选框图形 18，外层点击区 ≥32，符合「点击区」口径；表格排序按钮、行内按钮本轮均 ≥32。）

### 1.5 字阶（字号 / 行高成对）

文字节点的 `fontSize/lineHeight/weight` 组合，与 `02-icons-type-and-copy.md:87-97` 对照：

| 实测对 | 规范档 | 判定 |
| --- | --- | --- |
| `28/36 w650` | 页面标题 28/36 600-650 | 符合 |
| `18/26 w700` | 区域标题 18/26 **600** | 字重偏高一档（P2-7） |
| `16/24 w600` | 卡片标题 16/24 600 | 符合 |
| `14/22 w400` · `14/20 w400/500` | 正文 14/22、控件 14/20 | 符合 |
| `13/20 w400/500/700` | 字段标签 13/20 500 | 存在 w400/w700 变体（P2-7） |
| `12/18 w400/500` | 辅助说明 12/18 | 符合 |
| `12/20 w400`（插件中心 2 处） | 辅助说明 12/18 | 行高偏大一档（P2-7） |
| `24/32 w500`（概览 1 处） | 数值强调 24/32 **600** | 字重偏低一档（P2-7） |

**字号集合恒为 {12,13,14,16,18,24,28}，未引入新字号；行高基本成对。**

### 1.6 间距阶

规范：4 / 8 / 12 / 16 / 24 / 32 / 48（`01-foundations.md:166`），且 `:179` 声明「现已全部收回阶内」。
本轮实测**阶外值仍复现**（详见 P2-4）：`gap:6px`、`gap:20px`、`padding:0 14px`、`padding:0 6px`、
`padding-left/right:38/34px`、`margin-top:20/40/6px`。已登记例外（0 / 2px 光学微调 / 7px 补偿 /
`--card-padding` 24 / 控件横向 16）不计。

### 1.7 图标五档

规范：12 / 14 / 16 / 18 / 20（+ 插画 26/28），「其余值（11、13、15、17、19、22…）一律不收」
（`02-icons-type-and-copy.md:35-48`）。实测**存在 11 / 13 / 15 三种档外值**（详见 P1-2），
另有 1 处 16px 图标被压成 **14.67 × 16**（P1-3）。

### 1.8 控件尺寸与圆角

| 项 | 实测 | 规范 | 判定 |
| --- | --- | --- | --- |
| 标准控件高 | `--control-height: 40px`，实测按钮/输入/选择器均 40 | 40 | 符合 |
| 紧凑控件 | 实测 32（多处） | 32 | 符合 |
| 大操作 | 实测 44（多处） | 44 | 符合 |
| 图标按钮 | 实测无 <32 的方形图标按钮 | 32 或 40 | 符合 |
| 卡片圆角 / 弹窗圆角 | `--radius-card: 14px` / `--radius-dialog: 16px`，实测卡片 14 | 14 / 16 | 符合 |
| 卡片内边距 | `--card-padding: 24px` | 24 | 符合 |
| 弹窗宽 | 480 / 640 / 800（实测） | 480 / 640 / 800 | 符合 |

---

## 2. 发现（按 P0 / P1 / P2）

### P1-1 【新发现】侧栏在最小窗口溢出、底部整块不可达；「设置」入口被压到 22px

**现象**：在规范规定的最小窗口 960×640（`01-foundations.md:147`）下，侧栏内容比窗口高，
`body` 又是 `overflow: hidden`，于是底部两块被推到窗口外且**没有任何滚动路径**；同时「设置」入口
被 flex 压扁到 22px。

**实测数字**（headless Chrome，视口 960×640，浅色）：

```
documentElement.scrollHeight = 755  /  clientHeight = 640   → 溢出 115px
body.overflow = hidden  (无滚动条)
aside._sidebar: h = 640, scrollHeight = 755, overflow = visible (无内部滚动)
  ├ brand           96px
  ├ nav            480px
  ├ button.settingsEntry  top 616 → bottom 638  h = 22   ← 应 44
  ├ div.sidebarBottom     top 638 → bottom 737  ← 整块在窗口(640)之外
  └ div.version           top 737 → bottom 755  ← 在窗口之外
```

同一缺陷在 1280×720 复现（`version` 715→733 不可见，`settingsEntry` 22px）；在默认窗口 1280×840
**不**复现（`scrollHeight = 840`，`settingsEntry` h = 44）；200% 模拟 480×320 下为 **47 × 20**。
`settingsEntry` 的 CSS 声明为 `height: 44px`（`src/app/App.module.css:164`），被 flex-shrink 压缩。

**规范出处**：`docs/design/01-foundations.md:147`（最小窗口 960×640）、`:149`（侧栏 216 / 窄 184）、
`:155`（紧凑控件高 32）；`docs/design/05-patterns-and-accessibility.md:52`（主要任务在最小窗口仍可完成）。

**影响**：在最小窗口下，侧栏底部的「本地配置 / 凭据只存系统安全存储」说明块与版本行**完全看不到也够不着**；
「设置」入口点击区 22px 高（低于 32 的底线，触控板 / 外接鼠标下偏难），200% 下 20px。
导航本身仍完整可见（96 + 480 = 576 < 640），主流程可完成，故定为 P1 而非 P0。

**修法**（两处，均小）：① `.sidebar` 让 nav 区可滚动——`nav { min-height: 0; overflow-y: auto }`，
或在矮窗口隐藏 `sidebarBottom`/`.version`（`@media (max-height: 760px)`）；② 给 `.settingsEntry`
显式 `flex-shrink: 0`，使其在任何窗口高度都保持 44px。

---

### P1-2 【新发现】图标尺寸越出「五档」（11 / 13 / 15px）

**现象**：多个页面的 JSX 直接写 `size={11}` / `size={13}` / `size={15}`，渲染结果即这三种档外尺寸。

**实测数字**（1280×720，浅色；`getBoundingClientRect`）：

| 表面 | 元素（Lucide 名） | 实测 | 源码出处 |
| --- | --- | --- | --- |
| 内容中心 | `triangle-alert` ×2 | **13 × 13** | `src/features/content/ContentPage.tsx:220,234` |
| 内容中心 | `refresh-cw` | **15 × 15** | `.../ContentPage.tsx:224,302` |
| 内容中心 | `external-link` ×2 | **11 × 11** | `.../ContentPage.tsx:378` |
| 插件中心 | `plus`、`external-link` | **15 × 15** | `src/features/plugins/PluginHubPage.tsx:301,400` |
| 插件中心 | `check`、`triangle-alert` ×2 | **13 × 13** | `.../PluginHubPage.tsx:268,273,278` |
| 工具管理 | `search`（另有 `refresh-cw`/`folder-open`/`external-link` 同为 15） | **15 × 15** | `src/features/tools/ToolsPage.tsx:119,235,243,248` |

**规范出处**：`docs/design/02-icons-type-and-copy.md:35-48`——「完整规则是**五档**，
其余值（11、13、15、17、19、22…）**一律不收**」。

**影响**：同页不同按钮的图标大小不一致（同一行里 15 与 16 并排错位），是 09-21 已经收过的同一类漂移的复发。
观感与一致性层面，且违反明文规范。

**修法**：按语义归位——行内与文字同行的 `external-link` / `triangle-alert` → **14**；
`plus` / `search` / `refresh-cw` 按钮图标 → **16**；微型容器内的 11px → **12**。

---

### P1-3 【新发现】供应商页搜索图标被 flex 压扁（14.67 × 16）

**现象**：供应商页页头搜索框里的放大镜图标实测宽 14.67，而源码是 `size={16}`。

**实测数字**：`svg.lucide.lucide-search`，`attr = 16×16`，渲染 `14.67 × 16`（宽被压 1.33px）；
父容器 `.providerSearch` 为 `display: flex; align-items: center; gap: 8px`
（`src/app/App.module.css:201-202`），图标未设 `flex-shrink: 0`（`src/app/App.tsx:353`）。

**规范出处**：`docs/design/02-icons-type-and-copy.md:50`——「图标是弹性项时**必须**加 `flex-shrink: 0`：
兄弟节点是一段长文案时，默认的 `flex-shrink: 1` 会把图标压成一条（实测出现过 8.91 × 16 的警告图标）」。
本轮没有压到 8.91 那么极端，但**同一根因仍在**。

**影响**：图标变形（非等比缩小），在更窄窗口或更长占位文本下会进一步恶化。

**修法**：给 `.providerSearch` 内的 `svg`（或该处图标）加 `flex-shrink: 0`。

---

### P2-4 间距阶回归：阶外值在多页复现

**实测数字**（1280×720，浅色；排除已登记的 0 / 2px / 7px / 16px 控件内边距 / 24 卡片内边距）：

| 阶外值 | 出现位置（selector） | 计数 |
| --- | --- | --- |
| `gap: 6px` | 概览 `.step`、工具管理 `SUMMARY`、插件/内容中心标签项 `.active` | 概览 5、工具 1、内容 7、插件 2 |
| `gap: 20px` | 插件/内容中心 `.page` 容器 | 插件 2、内容 1 |
| `padding: 0 14px` | 分段控件选中项 `.active`（`SegmentedTabs`） | 插件 3、内容 3 |
| `padding: 0 6px` | 插件中心计数 `.count` | 2 |
| `padding-left/right: 38px / 34px` | 供应商页搜索输入 / 下拉 | 各 1 |
| `margin-top: 20 / 40 / 6 / 3px` | 概览 `.cardActions`、供应商 `.bar`、工具 `.hint`、内容 | 各 1 |

**规范出处**：`docs/design/01-foundations.md:166`（间距阶 4/8/12/16/24/32/48）、`:179`
（「全量审计量出 `gap:10px` 37 处、`padding` 里的 14px 9 处……**现已全部收回阶内**」）。
`SegmentedTabs` 容器的 `padding:3px`（3+32+2=40）是**已文档化**的高度推导（`2026-09-21-extension-pages-design-conformance.md` R4），属例外，不计。

**影响**：单看每处不刺眼，合起来是页面节奏不齐——正是 `:179` 记录过的同一类问题复发。

**修法**：`gap:6px` → 4 或 8；`gap:20px` → 16 或 24；`padding:0 14px` → 12 或 16；
搜索框 `38/34` → 统一走控件 `0 16px` + 图标内嵌（或明确记为光学需要并按 `:181` 写清用途）。

---

### P2-5 「API Key」标签与输入框之间 0 间距

**现象**：供应商弹窗（新建 / 编辑）里，「API Key」标签的下缘与密钥输入框容器的上缘**完全贴住**，两主题都报 `touching`。

**实测数字**：`.field-label` bottom = **518**，`._secret` top = **518**（差 0）；
标签 `margin-bottom = 0px`、输入容器 `margin-top = 0px`、包裹 `<label>` 为 `display: block` 且无 `gap`。
（`[role="dialog"]` 口径 `touching = 1`。）

**规范出处**：`docs/design/01-foundations.md:166`（8 用于标签与输入）。

**影响**：唯一一个标签与控件贴死的字段，看起来像「标签属于输入框的一部分」。

**修法**：该字段补 `margin-bottom: 8px`，或让它与其它字段共用同一套 label/控件间距规则。

---

### P2-6 PDF / 视频的禁用原因只存在于 Tooltip

**现象**：模型编辑器里「视频」「PDF」两格可见、灰态、点不动（合规），但**为什么不能选**只写在 `title` 属性里，页面上没有可见说明。

**实测数字**（`?view=model-editor`）：
`{ text:"视频", disabled:true, title:"PDF 与视频当前链路不能原生发送；即使声明也不会出现在原生能力里。", visibleReason:"" }`
（PDF 同）；该小节下方唯一的可见提示是通用文案「勾选＝声明这个模型支持这种输入……」。
`title` 由 `CheckCell.tsx` 的 `title={hint}` 承载，`hint` 来自 `ModelEditorPage.tsx:154`。

**规范出处**：`docs/design/02-icons-type-and-copy.md:52`（「Tooltip 不能成为唯一错误说明」）、
`:118`（已备好标准文案「当前 Codex 接入方式不能原生发送 PDF」）；
`docs/design/03-components.md:74` 要求这一项可见并说明原因。

**影响**：用户看到灰格点不动，只能靠悬停才能知道原因；触控板 / 键盘用户更容易错过。

**修法**：在「输入类型」小节下的 `p.field-hint` 里，当存在被禁用的能力项时追加一句可见说明，
复用 `02-icons:118` 的标准文案（而不是把它只挂在 `title` 上）。

---

### P2-7 字阶字重的小偏差

**实测数字**：`18px/26px` 的**字重为 w700**（区域标题规范 600，`02-icons-type-and-copy.md:91`，概览/网关各 1 处）；
`24px/32px` 的字重为 **w500**（数值强调规范 600，`:96`）；插件中心有 **2 处 `12/20`**（辅助说明规范 12/18）。
字号本身全部落在 12/13/14/16/18/24/28 阶内。

**规范出处**：`docs/design/02-icons-type-and-copy.md:87-97`。
**影响**：观感/一致性，P2。**修法**：把 18/26 的 w700 收到 600、24/32 收到 600、12/20 收到 12/18。

---

### P2-8 【模拟】200% 缩放下长 URL 不换行

**实测数字**（**视口模拟** 480×320，非 macOS 原生缩放）：设置页关于区 `a.text-mono` 越出 `dd` **60px**；
内容中心新闻行 `span._itemMeta` 越出 **50 / 26px**；插件中心详情列文字越出 255px（父为可滚动列，可达，不计缺陷）。

**规范出处**：`docs/design/05-patterns-and-accessibility.md:52`（「长 URL、模型 ID 可换行 / 复制」）。
**影响**：模拟条件下长文本横向溢出。**修法**：给这类文本加 `overflow-wrap: anywhere` / `word-break: break-word`。
**标注**：这是按视口模拟，**不等同** macOS 200% 原生缩放，只作线索。

---

## 3. 交互链路完整性

判定口径：每个入口是否闭合「入口 → 操作 → **反馈** → 出口」。

### 3.1 Toast 出口（复核历史结论）

**仍成立的部分**：
- **唯一出口**（`03-components.md:23`）：`showToast` 是全局唯一入口，`ToastHost` portal 到 `body`。
- **最多 3 条**：实测内容中心刷新时同时出现 **3 条** `role=status`（`Toast.tsx` `MAX_VISIBLE=3`）。
- **停留时长**：`success 5000 / info 7000 / danger null`（`src/components/Toast.tsx:31`）。实测一条 success
  toast 在 ~5s 后消失；同时页面的状态行（`role=status`、无关闭按钮、常驻）与 toast 区分清晰。
- **无历史 / 不可回看**：离开页面再返回，toast 不保留（实测导航到「概览」后 `role=status` 列表为空）。

**「应用结果只活 5 秒」——部分修复，重启结论仍不可回看**（09-20 卡点 D 的复核）：
- 实测点「应用到 Codex」→ 差异弹窗「应用并重新加载」后，出现 `role=status` toast：
  **「配置已提交，Codex 已重启；它回来后看看模型菜单。」**，约 5s 后消失。
- 页面同时保留一张**常驻的「事务状态」卡**（`CodexConfigPage.tsx:416-444`），实测内容为阶段时间线
  「已准备 / 提交中 / 等待 Codex 重新加载 / 已核验」+ `awaiting` 面板
  「配置已提交，尚未确认当前 Codex 窗口已加载……」，即**阶段可回看**（这一半是修复）。
- **重启的三结论（重启成功 / 旧进程仍在 / 退出了没起来）与 `quitForced` 警示（用了兜底信号、可能丢未保存对话）
  没有任何持久状态**——`CodexConfigPage.tsx:199-210` 只 `showToast(...)`，无 `useState` 保存 report。
  离开页面再回来只剩「等待 Codex 重新加载」，看不出上次重启到底成没成、是否强杀过。
- **「已加载」仍需用户自己宣布**：`CodexConfigPage.tsx:434` 的「Codex 已重新加载」由用户点击才闭环
  （09-20 卡点 E，仍在）。

**结论**：历史结论「应用结果只活 5 秒 Toast、不可回看」**部分修复**——阶段状态已常驻可回看，
但**关键的重启结论与强制重启警示仍只存在于 5 秒 Toast**，仍是 P1 的层级不足（09-20 §维度 5 / 卡点 D 未完全关闭）。

### 3.2 各入口的链路闭合

| 入口 | 入口 | 操作 | 反馈 | 出口 | 判定 |
| --- | --- | --- | --- | --- | --- |
| 概览 | ✓ | 添加供应商 / 管理全部 / 打开 Codex | 待应用徽章、供应商卡 | ✓ | 闭合 |
| 网关 | ✓ | 增删改供应商、多 Key、发现模型 | **保存成功即关弹窗** + toast（`03-components.md:28-36`） | ✓ | 闭合 |
| Codex 配置 | ✓ | 检测 → 差异预览 → 应用 → 重启 → 确认加载 | 阶段卡（常驻）+ 结论 toast（5s） | ✓ | **反馈层不完整**（见 3.1） |
| 工具管理 | ✓ | 重新检测 / 展开详情 / 复制路径 | toast（`ToolsPage.tsx:62,241`） | ✓ | 闭合 |
| 插件中心 | ✓ | 市场 → 详情 → 安装确认 → 安装 | toast + 卡片「已装」 | ✓ | 闭合（夹具桩，落盘未验证） |
| 内容中心 | ✓ | 立即刷新 / 增删源 | 页面状态行 + toast | ✓ | 闭合 |
| 连接诊断 | ✓ | 选择目标 → 开始检查 | 阶段时间线（页面内） | ✓ | 闭合 |
| 日志 | ✓ | 筛选 / 导出诊断包 | 预览清单 + 保存对话框 | ✓ | 闭合 |
| 设置 | ✓ | 主题 / 语言 / 还原 / 重开向导 | toast / 即时切换 | ✓ | 闭合 |
| 接入向导 | ✓ | 三步 / 稍后再说 | 步骤标记 | ✓ | 闭合 |

---

## 4. 「不做假开关」原则的落实（逐项核对）

结论：**未发现假开关。** 逐项实测/查证如下：

| 核对项 | 结果 | 证据 |
| --- | --- | --- |
| PDF / 视频等不可执行能力是否可见但禁用 | **是，合规** | 实测 `disabled=true`、灰态、点不动；`ModelEditorPage.tsx:156-157`。仅「原因只在 Tooltip」为 P2-6 |
| 文本能力是否锁定 | **是** | 实测 `文本 disabled=true, title:"文本是必需能力，不能取消。"`（`ModelEditorPage.tsx:156` locked） |
| Chat Completions 的「实验」标签：静态还是读核心结论 | **静态文案** | `ProviderForm.tsx:429` 在 `protocol === 'chat_completions'` 时渲染固定字符串 `providers.chatAdapterExperimental`（`src/locales/zh-CN.ts:732`）。`src/contracts/types.ts` 无「适配器工具调用门禁」字段（`CompatibilityStatus` 是 Codex 实例兼容性，见 `CodexConfigPage.tsx:364`）。它是诚实标注，不是开关，无「像能用」问题 |
| Anthropic Messages 是否直说未实现 | **是** | `ProviderForm.tsx:426` 常显 `providers.anthropicUnsupported`；下拉里无该选项 |
| 「已验证路由」是否被摆成可用能力 | **否（也无入口）** | 全仓 `src/` grep `已验证路由 / 路由验证 / verifiedRoute` **0 命中**；仅设计文档 `02-icons-type-and-copy.md:113` 有该文案。与 09-20「路由验证无 UI」一致 |
| 供应商预设是否留可点空壳 | **否** | 全仓无 `listPresets`、无「从预设」入口（grep 0 命中）；`ProviderPreset` 类型仍在 `contracts/types.ts:70` 但无 UI |
| 插件中心是否放了 MCP 灰按钮 / 假分页 | **否** | `src/features/plugins/` 与 locales grep `mcp/MCP` **0 命中**；符合 `06-...md` §10 决定 3「MCP 不做」 |
| Codex 实例兼容性徽章 | **读核心值** | `CodexConfigPage.tsx:364` `compat.${selected.compatibility}`，夹具实测显示「未验证」 |

---

## 5. 与历史审计的对照

**新增（历史未登记，本轮首次量出）**：
1. 侧栏在最小窗口 960×640 溢出、`sidebarBottom`/`version` 不可达，「设置」入口压到 22px（P1-1）。
2. 图标尺寸越出五档（11/13/15，P1-2）——`02-icons:35-48` 明文禁止。
3. 供应商搜索图标被 flex 压扁 14.67×16（P1-3）。
4. 间距阶回归（gap 6/20、padding 14/38/34 等，P2-4）。
5. PDF/视频禁用原因只在 Tooltip（P2-6）。

**复核仍存在**：
- 「应用结果只活 5 秒、重启结论不可回看」（`2026-09-20-product-review.md` 卡点 D / §维度 5）——
  **部分修复**：阶段卡已常驻，重启结论与 `quitForced` 警示仍只活 5s（§3.1）。
- 「已加载需用户自己宣布」（卡点 E）：`CodexConfigPage.tsx:434` 仍在。
- 「路由验证无 UI 入口」：仍无（§4）。

**已修复（本轮实测推翻 / 关闭历史条目）**：
- 09-19 §3.1 浅色 4 处对比度不合格 → **已修复**：两主题 × 9 表面 `contrast = 0`（强调色改青绿后关闭）。
- 09-19 D1–D6 控件尺寸（`button`/`input` 字号 13px、`.icon-button` 36×36、`checkbox` 16×16、
  `.badge` 11px、`.field-hint` 20px、`.text-button` 28 高）→ **已修复**：实测按钮字号恒 14px、
  图标按钮无 <32、勾选框点击区 ≥32、按钮高集合 {32,40,44}。
- 09-19 §3.2 点击目标 10 处 → **大幅收敛**：现仅剩侧栏「设置」入口 1 处，且成因不同（flex 压缩，非设计值）。
- 09-21 记录的 2 处 `_modelName`/`_line` `touching` → **未复现**：网关页两主题、两窗口 `touching = 0`
  （夹具默认状态下模型表已渲染，见 960×640 的 table 溢出）。**注**：这是夹具默认状态的结论，
  不等于所有供应商选中态都无此问题。
- 概览「接入进度」五格等高（历史 110 vs 92）→ **已修复**：实测 5 格均 92。
- 09-21 的 `2026-09-20-audit-synthesis.md` §10（待应用条跨页吸底 P2）→ **已修复**：待应用条只在网关页，
  本轮各页几何审计无重叠、`touching = 0`。

---

## 6. 未验证项（这台机器 / 本轮手段下验不了）

1. **原生 macOS 窗口真实外观**：交通灯避让、`titleBarStyle: Overlay` 实际观感、拖拽区与按钮的交互。
2. **读屏软件（VoiceOver）实际朗读**：只能按 DOM 语义（`aria-live`、`role`、accessible name）判断，未真机朗读。
3. **200% 缩放的 macOS 原生验证**：本轮只有**视口模拟**（480×320），不等同原生缩放；报告内已逐处标「模拟」。
4. **真实 Tab 顺序手工走查**：脚本只能查正 tabindex 与缺名，未逐页手动按 Tab 走一遍。
5. **Windows**：窗口吸附、原生按钮、字体渲染、探测路径（本机仅 macOS）。
6. **插件中心真实安装落盘**：夹具为合成数据；未对真实 `~/.codex/skills` 做过一次真实安装（沿用 09-21 结论）。
7. **内容中心真实抓取成功率 / 限流**：夹具为合成条目，「Hacker News 连续失败」是夹具数据，非真实失败。
8. **应用 / 重启的真实事务结果**：夹具的 apply / restart 为合成返回，只验证了「反馈形态」，未验证真实写入与重启。

---

## 7. 附：本轮使用的可复现命令

```bash
# 环境：dev server 已在 5173（strictPort）；headless Chrome 经 CDP 驱动
# 注意本机 http_proxy/https_proxy 指向 127.0.0.1:7897，curl 访问 localhost 需 --noproxy '*'
curl -s --noproxy '*' -o /dev/null -w '%{http_code}\n' http://localhost:5173/visual.html   # 200
# 审计函数：在页面内以 body / [role="dialog"] 为 root 调用
#   .zcode/skills/design-conformance/scripts/in-page-audit.js
```

---

**报告结束。** 本轮不改任何源码；P1 三条与 P2 五条供产品总监在汇总轮按 §四 标准裁决。
