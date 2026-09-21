# 应用内更新：更新按钮、更新源与发布链路

状态：**已实现**（本机端到端验收通过，见 §10）。本文是这一功能的唯一说明处：
机制选型、信任链、界面入口、失败面、发布步骤都写在这里，改代码前先读这一篇。

相关文档：[安全与跨平台](05-security-and-platforms.md) §8 是策略层的要求（更新包必须签名、
版本与渠道匹配、不得在升级中降低安全设置）；[签名、公证与发布](../development/03-signing-and-release.md)
讲 Apple 签名与公证凭据。本文不重复它们的内容。

## 1. 目标与非目标

**目标**：用户在应用里看到「有新版本」按钮，点一下就能把包换成最新版，不用自己去 GitHub 找。
按钮位置在侧栏左上角、应用名下面（参考 ZCode 的做法）。

**非目标**：

- 不做后台静默自动更新。更新必须是用户点出来的——它会替换 `/Applications` 里的应用包并重启应用，
  这种动作不该在没有人的时候发生。
- 不做灰度、不做多渠道（stable/beta）。只有一个渠道：GitHub Release 的最新正式发布。
- Windows 暂不在范围内：Windows 上第三方模型还不能用（见 [证据索引](../appendix/01-source-index.md)），
  给它做更新链路没有意义。插件本身支持 Windows，配置好同一份 `latest.json` 的 `windows-x86_64`
  条目即可，但**没有验证过**。

## 2. 现状与差距

| | 之前 | 现在 |
| --- | --- | --- |
| 查询最新版本 | ✅ 已有：`switch-core/src/diagnostics/update.rs`，走 GitHub API `/releases/latest` | ✅ 改为由更新插件读 `latest.json`（见 §3 的选型理由），旧实现删除 |
| 下载与安装 | ❌ 没有。设置页原话是「本工具不自动下载安装」 | ✅ 应用内下载、验签、替换应用包、重启 |
| 界面入口 | ❌ 只有设置页一行文字 | ✅ 侧栏左上角按钮 + 更新弹窗（设置页那一行改为指向同一个入口） |

## 3. 机制：Tauri updater 插件 + `latest.json` + minisign

用 `tauri-plugin-updater`（v2.12）。它负责**下载、验签、落盘、重启**四件事，
我们只负责：什么时候查、界面怎么显示、失败怎么讲清楚。

为什么不用「点一下打开下载页」这种最省事的做法：那样每次更新都要用户自己找 dmg、解压、
拖进 `/Applications`。既然发布产物本来就有固定的附件名，更新可以就在应用里完成。

为什么更新检查不能在前端做：`tauri.conf.json` 的 CSP 是
`connect-src ipc: http://ipc.localhost`，webview 不允许直连外网。所有网络访问都走 Rust 侧，
前端只通过 IPC 命令与事件拿到结果。

**信任链**：

```
GitHub Release 附件 latest.json
  └─ platforms["darwin-aarch64"].url      → Switchelp.app.tar.gz（附件，直接下载）
  └─ platforms["darwin-aarch64"].signature → minisign 签名（对 tar.gz 的字节）
                    │
        下载后在本地用内置公钥验签 —— 签名不对就不安装，没有任何“跳过”开关
```

公钥（`pubkey`）写进 `tauri.conf.json`，随仓库公开；私钥只在本机与 CI secret 里（§4）。
校验失败的错误码是 `update.signatureMismatch`，界面上如实说「安装包签名校验没通过」。

**更新源**（`plugins.updater.endpoints`）：

```
https://github.com/nexsjournal/switchelp-macapp/releases/latest/download/latest.json
```

用 `releases/latest/download/<附件名>` 这条路径而不是 GitHub API：它不带速率限制，
也不需要 token，且**只指向正式发布**（草稿与预发布不会成为 `latest`）。
这对我们正好——未完成的发布是草稿（见 [release.yml](../../.github/workflows/release.yml) 里
「零产物一律先成草稿」那段）。

## 4. 两套签名，管的是两件不同的事

这是最容易混淆的地方：更新链路上有**两个完全独立**的签名，缺一个都不行。

| | minisign（更新包签名） | Apple Developer ID（应用签名） |
| --- | --- | --- |
| 谁校验 | 本应用自己（插件内置公钥） | macOS（Gatekeeper）、钥匙串 ACL |
| 防的是 | 包在传输/托管环节被替换 | 应用被篡改、身份不明 |
| 密钥在哪 | 私钥本机 `~/.config/switchelp/updater.key` + CI secret；公钥在 `tauri.conf.json` | 钥匙串里的 Developer ID 证书 |
| 不满足时 | 更新直接拒绝安装（签名不匹配） | 应用打不开，或**读不到已保存的 API Key** |

**密钥托管**：

- minisign 私钥**不进仓库**。本机默认位置 `~/.config/switchelp/updater.key`（权限 600），
  CI 用 `TAURI_SIGNING_PRIVATE_KEY`（内容或路径）+ `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
- 私钥丢了 = 已装用户再也无法自动更新，只能手动下载重装。所以**发布前先备份私钥**。
  现在换密钥的成本是零（还没有任何用户装过带更新的版本）；第一次正式发布之后再换就有代价了。
- 公钥换了同理：新公钥只认新私钥签的包，老版本应用内置的是老公钥，会全部拒绝。

**Apple 身份必须保持一致**（本项目特有的硬约束）：应用用 Developer ID 身份把供应商 Key
写进钥匙串，钥匙串 ACL 认的是这个身份。更新后的包如果换成别的身份（或干脆没签名），
应用读 Key 时会被系统弹窗拦下——用户看到的是「更新完就让我输钥匙串密码」。所以：

- 更新产物必须用**同一个** `APPLE_SIGNING_IDENTITY` 构建；
- 本机出包一律带 `APPLE_SIGNING_IDENTITY`，不要出无签名包（见
  [签名与发布](../development/03-signing-and-release.md) §5）。

## 5. 清单格式

`latest.json` 是本项目自己生成并上传的附件（脚本：`scripts/make-latest-json.mjs`）：

```json
{
  "version": "0.3.0",
  "notes": "## 这一版…",
  "pub_date": "2026-09-21T12:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<Switchelp.app.tar.gz.sig 的内容>",
      "url": "https://github.com/nexsjournal/switchelp-macapp/releases/download/v0.3.0/Switchelp.app.tar.gz"
    }
  }
}
```

- `platforms` 的键是 `{os}-{arch}`：本项目当前只有 `darwin-aarch64`（Apple Silicon）。
  Intel Mac 没有自动构建产物，因此**不会**收到更新提示（插件找不到平台对应的条目）。
- `version` 必须**严格高于**当前版本才会被认为有更新（插件用 semver 比较，不是字符串不等）。
- tar.gz 的内部结构必须是 `Switchelp.app/…`（顶层目录会被跳过，插件按此约定解包）。

## 6. 界面

### 6.1 入口：侧栏左上角

```
┌──────────────────┐
│ ▣  Switchelp     │   ← 品牌块：logo + 应用名
│   ( 更新 0.3.0 ) │   ← 有更新时：副标题让位给按钮
│  ⋯ 导航 ⋯        │
```

- **有更新时**：品牌块第二行由副标题（`app.subtitle`）换成更新按钮，按钮落在**副标题那一行**
  （实测：名字底 → 胶囊顶 2px，与副标题跟应用名的关系完全一致）。所以品牌块始终是两行，
  徽标不会因为出现按钮而错位。
- **没有更新时**：只有副标题，按钮不出现（不占位、不显示「已是最新」这类常驻文字）。
- **尺寸要小、位置要在那一行上**：它顶掉的是一行 12px 的副标题，视觉上就该是同一档——
  胶囊本身是内层 `<span>`（12px / 18px 字阶、`1px 8px` 内边距，实测 72×20）；
  按钮盒子 32 高（点击区），胶囊贴盒子**顶部**，于是胶囊顶在名字下方 **2px**、与副标题同一行
  （实测名字底 → 胶囊顶 = 2px）。把 `min-height: 32px` 加在按钮上让胶囊**居中**会把它压到
  名字下 19px（用户原话「位置有点太偏下」），所以必须是贴顶 + 抵掉多余高度。
- **徽标要和「名字 + 按钮」整列居中**：按钮必须放在文字列**里面**（当第二行），
  `.brand` 的 `align-items: center` 才会让徽标与整列居中；把按钮挂到列外，徽标就只跟名字那一行
  居中，看起来「徽标和按钮没对齐」（用户实测报过）。多出来的 12px 点击区用
  `margin-bottom: -12px` 抵掉、不参与排版（否则列高多 12px，徽标按 60 居中仍然偏下）。
  这是这套版面里唯一一笔换算，代码注释写了来由；实测两套主题下徽标中心与文字列中心偏差都是 **0**。
- **拖拽面不会吃掉按钮点击**：`data-tauri-drag-region="deep"` 可以放在整个品牌块上——
  Tauri 注入脚本里的 `CLICKABLE_TAGS` 含 `BUTTON`，鼠标落在按钮上时拖动判定返回 false
  （`tauri-2.11.5/src/window/scripts/drag.js`）。**不必**为了按钮把结构拆开：拆开正是上面
  「徽标没居中」那个 bug 的来源。
- 底 `--accent`、字 `--accent-fg`：tokens 里唯一验证过对比度的「accent 当底」组合。
  实测文字对比度：深色 9.09:1、浅色 5.47:1（均过 AA）。
- 文案：`更新 {version}`，宽度自适应，窄侧栏（含 200% 缩放）里省略号截断而不是换行。
- 点击后打开更新弹窗（§6.2），不在侧栏里直接开始下载——下载会替换应用包，先让用户看清版本。
- 设置页的「检查更新」保留，但不再谈安装：检查出有新版时提示「点侧栏左上角的更新按钮」，
  另外给一个「到发布页」按钮走系统浏览器兜底。手动检查的入口只有这一个，不另加按钮。

**踩过的坑（别重犯）**：副标题原来用结构选择器 `.brand span` 上色。更新胶囊的内层文字也是
`span`，于是被这条规则按「12px 灰字」接管，压在强调色底上对比度只剩 **1.05**——看起来是
一颗灰字的青胶囊。现在副标题有自己的类名（`.brandSubtitle`），结构选择器不许再用。

### 6.2 弹窗与状态机

弹窗走 [Dialog 组件](../../src/components/Dialog.tsx)（`width="normal"` 640，底栏在滚动区之外，
按钮放 `footer` 槽）。版面按规范拼装，不自己写间距：

- 正文用全局 `.form-fields`（24px 内边距 + 16px 网格间距），与标题、底栏的内边距对齐；
- 版本事实用 `dt/dd` 直接进网格的写法（110px 标签列，与设置页状态行同一读法）——
  包一层 `<div>` 会让每项单独成格、标签与值上下贴着（版面审计记成 `touching`）；
- 发布说明经 `renderReleaseNotes()` 渲染：认标题/列表/加粗/行内代码，
  **认不出的按普通文字显示**（宁可朴素也不吞掉作者写的内容）。清单里的说明请只用这个子集，
  表格与代码块会退化成一行行原文；
- 长说明的出口是「在 GitHub 上看完整说明」（走系统浏览器），不把整篇 Release 正文塞进弹窗；
- 错误用全局 `.error-message`，进度条是唯一的自定义控件（6px 轨道 + `--accent` 填充）。

| 状态 | 弹窗显示 | 底栏 |
| --- | --- | --- |
| `idle` | 版本事实（当前/可安装/发布时间）→ 说明 → 共存模式提醒 | 「稍后」/「下载并安装」 |
| `downloading` | 事实 + 进度条与 `已下载 3.2 / 5.4 MB`（上游没给总长时退化成不确定态）+ 说明 | 「关闭」 |
| `installing` | 同上，进度区改说「正在校验签名并替换应用包」 | 按钮禁用 |
| `failed` | 全局错误样式：一句人话 + 原始细节（其余内容照旧可见） | 「关闭」/「重试」/「到发布页手动下载」 |
| 安装完成 | 应用自动重启，弹窗随之消失 | — |

底栏那句提示**必须短**（实测一行 156×18）：第一版把「装完自动重启 + 共存模式要重启 Codex」
两句都塞进底栏，480 宽下换行 3 次、把底栏撑高。共存模式的提醒现在在正文里。

**下载没有「取消」**：插件的下载不支持中断，摆一个按下去什么都不停的按钮就是假开关。
所以底栏只给「关闭」，并明说「关闭这个窗口不会中断下载」——下载会在后台跑完，然后应用重启。

重启后**不再显示弹窗**：如果这次启动是「刚更新完」，用一次全局 Toast 说一句
「已更新到 x.y.z」——这是用户唯一能确认「刚才那下点成功了」的地方。
落地方式：安装完成前写一个标记文件（应用数据目录 `update-pending.json`），
启动时读到就弹 Toast 并删掉它（读一次就删，所以只会出现一次）。

**文案口径**：错误只说事实，不猜原因。网络不通就是「连不上更新源」，不说「请检查网络设置」；
签名不匹配就说「安装包签名校验没通过，已放弃安装」。

## 7. 失败面

| 情况 | 表现 | 处理 |
| --- | --- | --- |
| 无网络 / 需要代理 | 查询失败 | 侧栏不出现按钮；设置页如实显示「检查更新失败：…」+ 发布页链接。**不得**显示成「已是最新」 |
| 代理环境 | 插件默认走系统代理 | 系统代理未覆盖时失败，同上一行。这是已知限制，不做应用内代理配置 |
| GitHub 限流 / 5xx | 查询失败 | 同上 |
| 最新发布不是给这个平台的 | 插件报「没有匹配的产物」 | 按「没有更新」处理，不显示按钮 |
| 下载中断 | 下载失败 | 弹窗给「重试」；已下载的字节不保留（下次从头下） |
| 签名不匹配 | 拒绝安装 | 明确说校验没过。**绝不提供跳过选项** |
| `/Applications` 不可写 | 插件用 AppleScript 申请管理员权限 | 系统弹密码框；用户取消就是安装失败，如实报 |
| 待应用配置未提交 | — | 更新不影响配置：Codex 配置、应用数据、钥匙串条目都在应用包之外。**但**更新后 bridge 会换成新版本（启动时从包内重新安装），仍在运行旧 bridge 的 Codex 需要重启才能用上新 bridge。弹窗里不拦，安装完成后的 Toast 里附一句提示 |
| 正在跑推理请求 | — | 不拦截。更新不碰网关进程边界（更新即重启应用，网关随之重启），在途请求会断——所以更新前应当让用户知道会重启（弹窗里写了「应用会自动重启」） |

## 8. 发布步骤

### 8.1 版本号（三处必须同步）

`package.json` / `src-tauri/tauri.conf.json` / 工作区 `Cargo.toml`。CI 里已有一步强制校验，
并检查 tag 与版本号一致（`v{version}`）。更新功能对版本号**更敏感**：不升版本号，
已装用户永远收不到更新。

### 8.2 本机出包（带签名）

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: <你的名字> (TEAMID1234)"
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.config/switchelp/updater.key"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="<密钥口令>"
pnpm exec tauri build --bundles app,dmg
```

`bundle.createUpdaterArtifacts: true` 时，Tauri 在签名后会额外产出：

- `target/release/bundle/macos/Switchelp.app.tar.gz`（更新包）
- `target/release/bundle/macos/Switchelp.app.tar.gz.sig`（minisign 签名）

### 8.3 上传

Release 附件至少要有三样（缺 `latest.json` 或 `.sig`，更新链路就是死的）：

| 附件 | 用途 |
| --- | --- |
| `Switchelp_<版本>_aarch64.dmg` | 新用户手动安装 |
| `Switchelp.app.tar.gz` + `Switchelp.app.tar.gz.sig` | 应用内更新（插件按固定名找 `.sig`） |
| `latest.json` | 更新清单；用 `scripts/make-latest-json.mjs` 生成 |

```bash
node scripts/make-latest-json.mjs --version 0.3.0 --notes-file notes.md \
  --sig target/release/bundle/macos/Switchelp.app.tar.gz.sig > latest.json
gh release upload v0.3.0 latest.json Switchelp.app.tar.gz Switchelp.app.tar.gz.sig
```

### 8.4 CI

`.github/workflows/release.yml` 的 macOS job 已经需要 Apple 凭据；现在再多两个 secret：

| Secret | 用途 |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | minisign 私钥内容（base64 或原文） |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 私钥口令 |

未配置时 CI 不会产出 updater 附件——那样发出去的 Release 看起来正常，
但所有用户的更新按钮都不会亮（本轮不就绪时应当**先不发**或明确发成草稿）。

CI 里的三处改动（`.github/workflows/release.yml`）：

1. macOS job 的构建步骤带上上面两个 secret（`TAURI_SIGNING_PRIVATE_KEY` /
   `..._PASSWORD`），`createUpdaterArtifacts` 就会产出 `.app.tar.gz` 与 `.sig`；
2. 归集产物时把这两个附件一起收进 `out/`，并按本架构生成一份 `latest-<平台>.json`
   （`scripts/make-latest-json.mjs --target …`）；
3. publish job 在发布前把各架构的 `latest-*.json` 合成一份 `latest.json`
   （`--merge`；两个架构版本不一致时脚本直接报错退出，宁可不发）。

> 未经 CI 实跑：这套 workflow 是照着本机验证过的脚本与产物写的，但**没有在 GitHub Actions
> 上真跑过一次 tag 发布**。第一次正式发布时请对着 §10.3 的清单核一遍。

## 9. 首版迁移（重要）

**已经装了 0.2.0 及更早版本的用户，收不到这次更新。** 老版本里没有更新代码，
它的设置页只会说「本工具不自动下载安装」。他们需要手动下载一次 0.3.0（或更晚）的 dmg，
从此以后才有应用内更新。

这不是缺陷，是任何自动更新功能的第一次上线都要跨的一道坎。Release 说明里要写清楚。

## 10. 验收

### 10.1 真实链路（本机跑过，不是 mock）

做法：把 `endpoints` 临时指向本地 HTTP 源（debug 构建允许 http，release 构建会拒绝，
见插件 `validate_endpoints`），源上放一份广告 0.3.0 的清单与 0.3.0 的更新包；
用 0.2.0 的构建跑一遍完整更新。

| 步骤 | 观察到的证据 | 结果 |
| --- | --- | --- |
| 检查 | 应用启动后本地源收到 `GET /latest.json 200` | ✅ 侧栏左上角出现「更新 0.3.0」胶囊，副标题让位 |
| 下载 | 同一次更新里再接 `GET /Switchelp.app.tar.gz 200`（约 16 MB 的 debug 包） | ✅ 弹窗进入下载态，进度来自真实字节回调 |
| 验签 | 包由 `TAURI_SIGNING_PRIVATE_KEY` 签，公钥固化在 `tauri.conf.json` | ✅ 通过（签名不符时拒绝安装的分支由单测覆盖，见 10.2） |
| 安装 | 运行中的应用包从 `CFBundleShortVersionString` 0.2.0 变成 **0.3.0**；`lsappinfo` 报告运行中的进程 `Version="0.3.0"` | ✅ 包被真正替换 |
| 重启 | 进程 PID 变化（旧进程退出、新版本进程接手），重启后应用又查了一次更新源 | ✅ 自动重启，且新版本不再提示更新 |
| 更新后提示 | 标记文件 `update-pending.json` 被写入、随后被重启后的应用读走并删除 | ✅ 「已更新到 x.y.z」那条 Toast 的路径确实跑到了（写入→读取→删除闭环） |

补充说明：这次本机验收用的是**ad-hoc 签名**（`APPLE_SIGNING_IDENTITY="-"`），因为这台机器
上的 Developer ID 签名需要钥匙串口令，自动化环境里拿不到。签名身份只影响 Apple 的信任链与
钥匙串 ACL，不影响更新链路本身；正式发布必须用 Developer ID（§4）。

### 10.2 可自动化的部分

- 组件与交互：`src/features/update/Update.test.tsx`（8 项）覆盖「有更新才有按钮、副标题让位、
  检查失败静默、弹窗文案、真实进度、无总长退化成不确定态、失败可重试+手动下载、更新后的 Toast」。
- 文案键：`src/i18n.test.ts` 守住中英键一一对应，并检查代码里用到的 `update.*` / `error.update*` 都有文案。
- 界面几何与对比度：在视觉夹具（`visual.html`）里实测，结果见 §6.1。

### 10.3 版面审计

弹窗与胶囊的几何、对比度、点击目标、最小窗口、键盘与语义的实测数字见
[更新弹窗版面审计](../audits/2026-09-21-update-dialog-design-conformance.md)
（两套主题、最小窗口 960×640，四类问题均为 0）。第一版没做这一步被用户当场驳回，
这份报告是修复后的复测记录。

### 10.4 未验证（不得当成已通过）

- **CI 出包链路**：本机出了带 updater 附件的包并手工上传验证过流程，但 workflow 里加 secret
  之后的完整跑通没有验证（需要一次真实 tag 发布）。
- **公证后的更新**：本机包是「已签名未公证」。公证 + staple 之后更新是否仍无感没有验证——
  应用内下载的包不带 `com.apple.quarantine`，理论上更干净，但没有实测。
- **`/Applications` 不可写时的提权路径**：插件会走 AppleScript 申请管理员权限，这条分支没有实测。
- **Intel Mac / Windows**：没有对应产物，链路未验证。
- **真实点击**：本机自动化点按不可用，真实链路的验收由一段**临时的**构建期触发（点胶囊 → 点安装，
  走的是与用户相同的两条路径，验收后已删除）。界面本身的点击、键盘可达与视觉由夹具实测补上。
