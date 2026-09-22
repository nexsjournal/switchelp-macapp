# 设计规范符合性审计（Switchelp）

用实测数据（而不是凭印象）核对界面是否符合本仓库写好的设计规范：对比度、点击目标、溢出、重叠、
字号、焦点、键盘与响应式。需要「过一遍界面」「检查设计规范」「看看哪里不符合」「输出测试报告」
时，按本文的流程做。

## 为什么要有这套办法

这个仓库已经有写好的设计规范（`docs/design/01-foundations.md` 起）和一份 token 源
（`src/styles/tokens.css`）。问题从来不是「没有标准」，而是**没有人在标准与渲染结果之间做过
逐条核对**——于是「按钮 13px 而规范写 14px」这种偏差能活很久，因为光看代码看不出来。

所以这套办法的硬规则只有一条：

> **不许报一个你没量过的数字。**

「对比度看起来够」「间距大概是 24」这类结论一律不要写进报告。要么用
`scripts/in-page-audit.js` 量出来，要么写「未验证」。

## 环境准备

1. 起开发服务器（端口 5173 是 strictPort，已被占用时直接复用，不要再起一个）：
   ```
   npm run dev
   ```
2. 视觉夹具在 `http://localhost:5173/visual.html`，用合成数据渲染真实界面，不碰真配置。
   - `?view=onboarding` 走首次接入向导
   - `?view=codex` 直接进 Codex 配置页
   - `?view=settings` 直接进设置页（其余页面见 `src/dev/visual-fixture.tsx` 的 `initialPage`）
   - 主题用夹具的 `?theme=light|dark`
   - **注意**：只在 query 上变化的 `goto` 可能不触发重新加载（同文档导航），主题参数
     会看起来没生效。可靠做法是直接写 `document.documentElement.dataset.theme`——样式
     完全由这个属性驱动，两种主题都能量。
   - **设置页是例外**：它挂载时会 `applyTheme(readThemePreference())`，把 `?theme=` 的值
     覆盖回 localStorage 里存的偏好。要量浅色的设置页，先写
     `localStorage.setItem('gptswitch.theme','light')` 再加载（量完恢复原值）；
     否则会拿到「页面其实是深色」的假数据。
3. 浏览器操作用 `browser-use:control-browser` 技能（`mcp__node_repl__js`）。
4. **切主题之后必须等过渡走完再测量**。`--motion-hover` 是 100ms、抽屉 180ms，背景色是
   过渡属性：在过渡中间读 `getComputedStyle` 会拿到插值中的颜色，于是同一个页面的对比度
   结果会在「干净」和「报几处」之间来回跳（实测：设完 `data-theme` 立刻量得到 1~3 处，
   等 700ms 再量全部为 0）。同一次测量里还会出现「文字色是浅色主题、底色还是深色主题」
   这种自相矛盾的组合——看到这种组合就说明量早了，不要当成真问题去改代码。
   可靠做法：写 `data-theme` → 等 700ms → 量；并且顺手确认
   `getComputedStyle(document.body).backgroundColor` 已经是目标主题的底色。
   **但等 700ms 仍然不够**：2026-09-20 实测，在已渲染的页面上切 `data-theme`，等 1.5s 后
   容器与正文都换了主题，**按钮的背景色却还是旧主题的**——深色下读到「白底 + 浅灰字」、
   浅色下读到「深底 + 深字」，比值 1.09 / 2.14 这种一眼矛盾的数字。所以**要断言颜色就别在
   热切换后的页面上量**：用 `?theme=light|dark` 冷启动一次再量。同一批按钮在冷启动下测得
   6.4（浅色）与 9.2（深色），两套主题都合格。热切换只适合量几何（重叠、溢出、滚动）。

夹具的坑，别当成产品 bug：
- 夹具的 `saveProvider` / `selectCredential` 是空实现，新建后供应商列表不会真的变化。
- 夹具的 `saveModel` 不回写列表：供应商弹窗里「添加模型」保存后，模型表不会多出一行。
- `startProbe` 返回一份四个阶段全过的合成结果（用来走查「连接成功」结论条），`discoverModels` 返回 3 条写死的数据。
- 组件级直连视图（`?view=provider-form|model-form|model-editor`）**没有宿主可退回**，
  它们的关闭/取消/后退是跳到 `?view=providers`。所以「取消」会整页跳走而不是关掉浮层——
  那是夹具的接法，产品里这些出口由 App 壳处理（`App.test.tsx` 覆盖）。

## 审计流程

### 第 1 步：量几何（重叠、遮挡、溢出、紧贴）

把 `scripts/in-page-audit.js` 的**文件内容**作为字符串交给 `playwright.evaluate`，在下面这些
表面上各跑一次：

| 表面 | 怎么到达 |
| --- | --- |
| 概览 / 供应商与模型 / Codex 配置 / 连接诊断 / 日志 / 设置 | 点侧栏对应项 |
| 添加供应商弹窗 | 供应商页页头「添加供应商」 |
| 编辑供应商弹窗（含凭证池、获取模型） | 供应商页「编辑配置」 |
| 模型能力编辑器 | 供应商页「添加模型」 |

脚本会返回四类问题：`overlap`（相邻兄弟真重叠）、`overflow`（子元素横向溢出父容器）、
`touching`（相邻块紧贴 0 间距，通常是少了一层 grid gap）、`clipped`（被祖先的
overflow 裁掉）。

排除噪音：`thead`→`tbody`、`tr`→`tr` 的紧贴是表格的正确行为；`visually-hidden`、
纯装饰 SVG、绝对/固定定位的元素不参与判定。

### 第 2 步：量对比度与点击目标

把 `scripts/in-page-audit.js` 里的 `measureText` 也一起跑（同一份文件里）。它会按
**alpha 合成后的真实底色**算对比度——不是拿 token 名去猜底色，这点很关键，因为
`--text-muted` 放在 `--bg-surface` 上合格、放在 `--bg-selected` 上就不合格。

判定线：正文 4.5:1；≥24px 或 ≥18.66px 且粗体 3:1（WCAG AA）。

**一条已登记的例外**：表单控件的**静息态**描边（`--border-field`，深色 2.24 / 浅色 2.31）低于
「需要识别的控件边界 3:1」，这是 2026-09-21 的产品决定，理由与复查方式写在
[测试与发布](02-testing-and-release.md) 的
可访问性一节。它低于线，但**按例外处理，不要报成不合格项**；悬停（4.27 / 4.42）与焦点环照旧要在线上。

点击目标：规范要求图标点击区 ≥32×32、紧凑控件高 32、标准 40。

**必须两套主题各跑一遍。** 两套 token 是独立取色的，一套过不代表另一套过。

### 第 3 步：量最小窗口与缩放

`tab.setViewportSize({ width: 960, height: 640 })`——这是规范里的最小窗口。逐页检查
`document.documentElement.scrollWidth > clientWidth`（出现横向滚动即不合格），
以及侧栏是否按规范收窄到 184px。

### 第 4 步：键盘与语义

- `[tabindex]` 里不允许出现正数。
- 每个页面恰好一个 `<h1>`。
- 纯图标按钮必须有中文 accessible name。
- 表格 `columnheader` 全部带 `scope="col"`。
- 焦点环：`:focus-visible { outline: 2px solid; outline-offset: 2px }` 必须存在，
  且被 `overflow` 裁切的地方要有留白（`.scroll-area` 的 `padding:2px; margin:-2px`）。
- 对话框：Escape 关闭、关闭后焦点回到触发控件、有未保存内容时先问「继续编辑 / 放弃修改」。

### 第 5 步：写报告

持久化到 `docs/audits/<日期>-design-conformance.md`，格式照
`docs/audits/2026-09-19-design-conformance.md`（那就是这个格式的一个完整实例）。
每条发现必须带：**现象 → 实测数字 → 规范条目出处（file:line）→ 影响 → 修法**。
引用行号前先 `sed -n '<n>p' <file>` 核一遍——引错行号比不引更糟。

按严重度排序：P0（违反 P0 需求或挡住主流程）→ P1（违反明文规范或可访问性）→ P2（观感与一致性）。

最后必须有一节「未验证项」，写清楚哪些要求这台机器上验不了（真机 Codex 路由、
macOS 原生窗口外观、签名安装包、200% 缩放、Tab 顺序手动走查）。

## 已知基线（下次审计拿来对比漂移）

下面是 2026-09 一次全量审计的结论，之后每次审计都该复核这几条是否还成立：

- 深色主题对比度**全部合格**；浅色主题有 4 处不合格（主按钮白字、accent 链接、
  accent 小标签、选中行上的 `--text-muted`）。
- `--text-muted` 在 `--bg-selected` 上两种主题都不合格（深 4.11 / 浅 4.38），
  因为选中行底色把对比度压下去了。
- `global.css` 里若干控件尺寸与规范不符：按钮/输入字号 13（规范 14）、
  `.icon-button` 36（规范 32 或 40）、checkbox 16（规范 18 图形 + ≥32 点击区）、
  `.badge` 11px 且无 min-height（规范 12px / 最小高 22）、`.field-hint` 行高 20（规范 18）、
  `.text-button` 与表格行内按钮低于 32 高。
- `global.css` 第 35 行给 `a` 用的 `--status-info` 被第 39 行覆盖，属残留；规范禁止
  状态色承担非状态语义。
- 模型编辑器打开时点侧栏导航：面包屑与「当前页」高亮会变，但主内容仍是编辑器——
  导航在这一状态下是失效的。
