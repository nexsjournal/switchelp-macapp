# 陌生人从下载到跑通：开源工具配置审计（2026-09-20）

本次审计的任务是回答用户提出的一个问题：**「别人下载这个开源 APP 之后，能不能直接使用，要做到一次顺滑的配置流程」**。

做法：站到一个「从 GitHub Releases 下载、从没接触过这个项目」的陌生人的位置，把每一步走一遍。核心产物是第 1 节的**逐步清单**，然后是分主题的证据（第 2 节）、按「改动成本 vs 收益」排序的改进清单（第 3 节）、未验证项（第 4 节）。

---

## 0. 审计范围、方法与本地状态声明

**证据分三档，报告里每条结论都落在其中一档上**：

| 档 | 来源 | 覆盖范围 |
| --- | --- | --- |
| ① 发布事实 | `gh release list/view`、`gh api`、workflow run 日志、`dist-release/` 产物与 SHA256、`codesign`/`spctl`/`hdiutil` | 下载到打开这一段 |
| ② 代码事实 | 具体 `文件:行` | 错误分支、空态、状态机 |
| ③ 实测 | 本地 headless Chromium 走 `pnpm dev` 的 `/visual.html` 夹具；真机启动一次 `/Applications/Switchelp.app` 观察进程与端口 | 界面流程 |

**实测工具的边界（必须先说清）**：本 agent 不能用 ZCode 的浏览器自动化——`agent.browsers.getForUrl(...)` 直接报 `Browser is not available in subagent`。因此界面走查用的是**本地 headless Chromium**（`playwright-core` 1.60，手动指定缓存里的 `chrome-headless-shell`）+ `pnpm dev` 夹具，脚本在 `/tmp/walk/`（含各步截图）。**真机 GUI 的视觉核验（Gatekeeper 弹窗、Codex 模型菜单）本次没有做**，见第 4 节。

**夹具的已知空实现（不能当成产品问题）**：`src/dev/visual-fixture.tsx` 里 `saveProvider`/`saveModel` 不回写列表、`listProviders` 恒返回固定数据、`discoverModels` 返回写死 3 条、`startProbe` 合成四阶段全成功、`planApply` 返回固定计划。凡结论依赖这几处的，行内标【夹具】。

**夹具运行环境的一处失真**：5173 上的 `pnpm dev` 是 9 月 18 日 14:58 启动的常驻进程，`vite.config.ts` 的 `define: { __APP_VERSION__ }` 在**启动时**求值，所以夹具界面上显示的版本号是 `0.1.0`（`package.json` 已是 0.2.0）。这是 dev server 陈旧，不是产品缺陷；被服务的源码是当前的（同一份 `App.tsx` 里 `showOnboarding`/`PendingApplyBar` 命中 7 处）。**报告里凡是引用界面文案的地方，说的都是文案键的真实值（来自 `src/locales/*.ts`），不是夹具的合成数据。**

**对真实机器做的事与还原**：

| 动作 | 影响 | 还原 |
| --- | --- | --- |
| 启动 `/Applications/Switchelp.app`（0.2.0，进程 68327）观察启动是否成功 | 网关绑定 `127.0.0.1:18765` | 已退出，端口已释放（`lsof -nP -i :18765` 空） |
| 在 `/tmp/qtest.app`（`/Applications/Switchelp.app` 的副本）上写 `com.apple.quarantine` 再删除 | 只动 /tmp 副本 | 已 `rm -rf` |
| 备份 `~/.codex/config.toml` | 只读复制 | 全程**未改写**该文件；审计前后 sha256 均为 `0dc8fb41682a8cc0f54a8d37298c4c9e6450200d8bd6aa5ea8bd044a78236f46`，备份在 `/tmp/config.toml.audit-backup` |

---

## 1. 陌生人从下载到跑通的逐步清单

判定口径：**顺利**＝按 README/界面走即可；**有摩擦**＝能过去，但要说清摩擦是什么；**卡死**＝这里过不去或会被劝退。

| # | 步骤 | 判定 | 摩擦 / 卡点（含证据） |
| --- | --- | --- | --- |
| 1 | 打开 README，找下载入口 | **有摩擦** | 下载表格写的是 `Switchelp_0.1.3_*`（`README.md:36-38`、`README.zh-CN.md:29-31`），而当前最新发布是 **v0.2.0**，中间还有 0.1.6/0.1.7/0.1.8/0.1.9 都没进表。陌生人照表找 → 下载到 5 个版本前的包。下载数据佐证：v0.1.0 dmg 2 次、v0.1.3 dmg 1 次、**v0.2.0 dmg 0 次**（`gh release view --json assets`）。更糟的是 v0.1.3 的 release notes 明确写「本工具目前不会替你重启 Codex」——这句在 0.2.0 已经不成立（现在会自动重启：`src/features/codex/CodexConfigPage.tsx:186`、`src/app/PendingApplyBar.tsx:82`），陌生人会按过期说明多走一步 |
| 2 | 下载 macOS 包 | **有摩擦** | 9 个 release 的产物**全部只有 aarch64**，没有任何 x86_64 / universal。CI 的 macOS matrix 里虽然定义了 `x86_64-apple-darwin`，但没配签名 secret 时整个 job 跳过（下面 2.1）。**Intel Mac 用户没有任何可下载的包**，README 也没写「Intel 请自行构建」 |
| 3 | 首次打开（Gatekeeper） | **卡死（对不懂 macOS 的人是劝退点）** | `.app` 只有签名、没有公证：`codesign -dv` = `Developer ID Application: <签名姓名> (<TEAMID>)`，`spctl --assess --type execute -vv` = **rejected / `source=Unnotarized Developer ID`**。README 给的第一顺位解法是「**右键 → 打开**」，但 Apple 现行的官方支持页（`support.apple.com/en-us/102445`，本次实际抓取）只描述 **系统设置 → 隐私与安全性 → 仍要打开** 这一条路径，已不再提及 Control-click。`xattr -dr com.apple.quarantine` 那条是有效的（我在 /tmp 副本上验证了标记可被清除），但要用户会开终端、会粘贴路径；而且 **.dmg 挂载后只有 `Switchelp.app` 与 `Applications` 符号链接，没有任何首次打开说明文件**——提示只存在于 README 和 release notes |
| 4 | 第一次启动 → 接入向导自动展开 | **顺利** | `src/app/App.tsx:126`：`showOnboarding = (onboardingForced \|\| (providers.length === 0 && !onboardingDismissed)) && page === 'overview' && ...`。夹具 `?view=onboarding` 实测自动进入「检测 Codex」步骤，零点击。真机启动也成功（进程活着、网关绑上 18765） |
| 5 | 向导第 1 步：检测 Codex | **顺利**（未装 Codex 的分支只有代码证据） | 检测结果把「配置存在 / 当前未运行 / 兼容性 / 其他工具」逐项列出；检测到别的配置管理工具时说明残留标记长什么样、**写在哪个文件**（`OnboardingPage.tsx:156-168`）。未装 Codex → 空态 + 手动路径输入 + 明确「本工具不下载不安装 Codex」（`:127-134`）。**该分支夹具渲染不出来**（夹具恒定返回一个实例） |
| 6 | 向导第 2 步 → 点「添加供应商」 | **卡死（中道消失，且不可逆）** | 见 1.1，这是本次最重要的产品问题 |
| 7 | 填供应商表单 | **有摩擦** | 必填 3 项：名称 / Base URL / API Key（`ProviderForm.tsx` 的 `persist()` 校验）。**协议默认 `chat_completions`**（`:59`），而该适配器自带「尚未通过工具调用门禁、实验状态」提示（`:337`，文案 `providers.chatAdapterExperimental`）。陌生人的第一步就落在实验路径上，README 通篇没提 |
| 8 | 获取模型 / 手动添加 | **顺利** | 「获取可用模型」会先落库再读上游；上游不返回长度时给出「按 128,000 / 8,192 的默认值添加，之后可以逐个编辑核对」这类可核对说明（夹具实测 P1.6 文案即真实 locale 值）。上游没有 `/models`、无法解析、限流、超时都有 `advice.*` 系列的具体建议（`src/locales/en.ts:31-70`） |
| 9 | 选中模型 / 纳入 Codex 目录 | **顺利** | 模型行上有目录开关，添加时默认开 |
| 10 | 应用并重启 Codex | **顺利**（真机未验） | 待应用条常驻页面底部（`PendingApplyBar.tsx`）。夹具实测：从「已配好未应用」到提交成功**恰好 2 次点击**（条上「应用并重启 Codex」→ 差异弹窗里「应用并重启 Codex」）。差异弹窗列出「字段 / 当前值 / 应用后 / 原因」四列，并写明「应用后会退出并重新打开 Codex（未保存的对话可能丢失）」，提交后自动重启，无需第二次点 |
| 11 | 「为什么需要重启」在哪讲 | **有摩擦** | **向导里一个字都没有**：两套文案里所有 `onboarding.*` 键中，含 `restart`/`launch`/`start` 的为 **0 条**（`grep` 实测）；夹具第 3 步文案也只有「在 Codex 模型选择器中选择」。重启的必要性只在待应用条（`codex.pendingScope`：「Codex 只在启动时读配置，应用后需要重启」）、差异弹窗、Codex 配置页的重启确认里讲。而向导自称「完成第一条可用链路」，最后一步的按钮叫「查看差异并应用」（`overview.viewDiffAndApply`），**实际只做导航**（`OnboardingPage.tsx:238` → `onViewDiff` → `navigate('codexConfig')`），真正的「应用」在下一页——按钮名与行为不一致，属于「点了好像没生效」那一类问题 |
| 12 | 恢复原生 Codex | **顺利** | Codex 配置页按钮行里有「还原为原生 Codex」（`CodexConfigPage.tsx:284`，夹具实测 count=1 且可见）；设置页「危险操作」里还有「还原 Codex 配置」入口（`SettingsPage.tsx:205`，只做跳转，文案自己声明「这里只做入口」）。memory 里那条「藏在页面下方导致用户以为还原失败」的问题已修（`docs/audits/2026-09-20-global-ui-and-dialogs.md` §1.2：差异/确认卡片改成了模态） |

### 1.1 卡点 1：向导在第 2 步把自己干掉，而且「稍后再说」不可逆

**机制**：向导存在的唯一条件是**没有任何供应商**（`App.tsx:126` 的 `providers.length === 0`）。而向导第 2 步的 1 号动作就是「添加供应商」；保存成功后 `providers.length > 0` → `showOnboarding` 变 false → **整个向导消失，第 3 步（测试并应用）永远走不到**。

夹具里能看到同一机制的弱化版（因为夹具的 `saveProvider` 不回写列表）：打开供应商弹窗时 `showOnboarding` 为 false，`OnboardingPage` 被卸载；关掉弹窗后它**重新挂载、`step` 回到第 1 步**（实测 P1.8：关掉弹窗后又回到了「检测 Codex」）。真机上因为供应商已经存在，它不会再回来。

**`onboardingForced` 是死状态**：全仓库只有 `useState(false)`（`App.tsx:102`）和 `setOnboardingForced(false)`（`:222`），**没有任何 `setOnboardingForced(true)`**。所以：

- 点一次「稍后再说」→ 写 `localStorage['gptswitch.onboarding.dismissed']='true'`（`App.tsx:100,224`）→ **向导再也打不开**；
- 我把 6 个页面全点了一遍（实测 P1.11 列出每页所有按钮），**没有任何「接入向导 / 重新打开向导」入口**，设置页也没有；
- 实测：点「稍后再说」→ 刷新后向导不再出现（P1.9 / P1.10）。

**对陌生人的后果**：主线「配置供应商 → 获取模型 → 选中模型 → 应用并重启」在第 3 步到第 4 步之间断掉，之后要靠概览页底部那条待应用条自己接上（那条待应用条文案写得清楚，已经是当前最好的线索）；同时「稍后再说」这个看起来最无害的按钮，是一次不可逆的永久关闭。

### 1.2 卡点 2：Gatekeeper 这条路的顺滑度

事实层：签名有效、公证缺失（`spctl` = rejected / Unnotarized Developer ID）。这是**已知且有意的现状**，README 和 release notes 都承认，`docs/development/03-signing-and-release.md` 里有完整的补齐步骤。

问题不在「没公证」，而在**替代路径的写法**：

1. README 把「右键 → 打开」放在第一顺位，而 Apple 现行文档只保留「系统设置 → 隐私与安全性 → 仍要打开」；
2. `xattr` 那条被埋在表格单元格里，不是一段可直接复制的命令块（0.2.0 的 release notes 里反而是标准命令块，做得比 README 好）；
3. `.dmg` 里没有任何首次打开提示文件——陌生人双击被拦时，手上只有 README。

参考能落地的改进：把 `xattr` 一行做成独立代码块并提到表格正上方；把「系统设置 → 隐私与安全性 → 仍要打开」写成第一顺位；README 与 release notes 统一；再往上就是配公证（第 3 节第 11 条）。

---

## 2. 主题详述

### 2.1 下载到打开的链路：README 事实核对

| 声明 | README（中/英） | 事实 | 判定 |
| --- | --- | --- | --- |
| 版本号 | `0.1.3`（文件名里） | `package.json` / `src-tauri/tauri.conf.json` / `Cargo.toml`(workspace) 全是 `0.2.0`；最新 release `v0.2.0`（`gh release list` 共 9 个 release） | **不一致** |
| macOS 文件名 | `Switchelp_0.1.3_aarch64.dmg`、`Switchelp-0.1.3-arm64.zip` | v0.2.0 实际产物 `Switchelp_0.2.0_aarch64.dmg`、`Switchelp-0.2.0-arm64.zip`（release 里存在，`dist-release/` 里也有同名的本地产物） | **不一致（差 5 个版本）** |
| Windows 文件名 | `Switchelp_0.1.3_x64-setup.exe` / `_x64_en-US.msi` | v0.1.0~v0.1.3 有这些文件；**v0.1.6 之后所有 release 都没有 Windows 产物**（改名后 Windows 只在手动触发 `with_windows` 时才构建） | **不一致且更具误导性**：README 让陌生人以为有 Windows 包，实际最新几个版本没有 |
| 产物可信度 | release notes 公布两个 SHA256 | 实测 `dist-release/` 两个文件的 sha256 与 v0.2.0 notes 完全一致（`5b36e362…dmg`、`0fbc0337…zip`） | **一致** ✅ |
| 0.1.0 仍是旧名 `GPTSwitch` | 有说明 | v0.1.0 的 asset 确实是 `GPTSwitch-*` | **一致** ✅ |
| 未公证 | 有说明 + 原因 + 补齐入口 | 与 `spctl` 实测一致 | **一致** ✅ |
| Windows 第三方模型不可用 | 有说明 | 代码侧未找到 `.cmd` helper 的可用实现（只有 README/文档的说法） | **一致（未实测）** |

**发布流程对「只有源码的贡献者」意味着什么**：v0.2.0 的 release run `35505727245` 实测输出——

- `gate` job：`signed=false`，注释原文「未配置 APPLE_SIGNING_IDENTITY，跳过 macOS 构建：未签名的 macOS 包下载后会被 Gatekeeper 拦下，发布它比不发布更糟」；
- `Windows (x64)` 与 `macOS` 两个 job **均被跳过**，但整个 run 是**绿色**（`conclusion=success`）；
- `publish` job 打印「这次没有 CI 产物（macOS 未配签名时跳过、Windows 默认不出）。Release 照常创建，macOS 包由本机签名后手动附上」，然后照常建 Release；
- `gh api releases/tags/v0.2.0` 显示两个产物的 **uploader 都是 `nexsjournal`**（人的账号），创建时间 `10:42:28`，晚于 CI 的 `10:41:53` → **产物是人工上传的，不是 CI 产物**。

所以：贡献者打一个 `v*` 标签，会得到一个**没有任何二进制、但 CI 全绿**的 Release，README 完全没有提到这件事。历史上 v0.1.6 的 release run 因此红过一次（`35500544871`，那时零产物算失败），现在改成 notice 是刻意的设计——对维护者合理，对贡献者是信息缺口。

### 2.2 首次启动与错误分支逐条检查

| 前置条件 | 代码路径 | 给的是出路还是死路 |
| --- | --- | --- |
| 没装 Codex | 向导空态 + 手动路径 + 「本工具不下载不安装 Codex」（`OnboardingPage.tsx:127-134`）；Codex 配置页同样的空态 + 路径输入（`CodexConfigPage.tsx:215-232`）；设置页 `settings.noInstanceBody` | **可操作** ✅（夹具无法渲染 ⚠） |
| Codex 装了但没登录 | **全流程不检查登录态**；本工具不读 `auth.json`（README/0.2.0 notes 声明，代码侧 `codex/` 下也没有对 auth.json 的读写） | **能过去，但少一句解释** ⚠：向导不会告诉陌生人「Codex 仍是原生登录那一套，你的模型会出现在同一个菜单里」；这句话只在恢复原生的文案里出现 |
| 端口被占用 | 端口固定 `gateway::DEFAULT_PORT = 18765`（`crates/switch-core/src/gateway/mod.rs:24`）；绑不上返回 `PortInUse`（`gateway/server.rs:107-112`）；`DesktopState::new` 把 safe_detail 直接存成 `gateway_error`（`src-tauri/src/state.rs:58-68`）；前端只在底栏显示「网关未启动：{reason}」（`en.ts:548`） | **死路** ❌：reason 是**中文原文**（形如「无法绑定 127.0.0.1:18765：Address already in use」），设置页**没有任何改端口入口**。`docs/README.md` 自己把它列为已知偏差：「端口被占用时给出改端口计划 → 固定 18765，占用即网关不启动」。英文用户还会读到一条中文错误 |
| 没网络 | `advice.timedOut/unreachable/unresolvable` 各给 2-3 条具体动作（`en.ts:52-63`，含「用了代理就检查代理是否在跑」——正好是本机情形） | **可操作** ✅ |
| 多个 Codex 实例 | 不替用户选，列全部让人自己选（`OnboardingPage.tsx:60,136`） | **可操作** ✅ |
| 存在别的配置管理工具 | 警告 + 残留标记长什么样 + 涉及文件路径（`:156-168`） | **可操作** ✅ |
| 宿主没退出 / 退出没起来 / 被强制结束 | 三种结果三种说法（`CodexConfigPage.tsx:112-121`） | **可操作** ✅ |
| 重启失败（配置已提交） | 降级成 Toast 提示，不影响已提交的配置（`CodexConfigPage.tsx:184-187`） | **可操作** ✅ |

### 2.3 文档对陌生人的完整性

- **`docs/` 全部中文**（`docs/README.md` 自述），README 也承认「currently written in Chinese only」。对英文用户的后果：应用内 UI 有完整英文（`src/locales/en.ts`，且两套字典由 `src/i18n.test.ts` 守住键集合，实测 112 个用例通过），但**任何「为什么这个字段不能改 / 这个数字从哪来」的追查都没有英文入口**，`docs/README.md` 的「与文档的已知偏差」表也没有英文版。
- **没有安装排错 / 常见问题 / 卸载说明**：仓库根只有两份 README + LICENSE。`docs/development/03-signing-and-release.md` 讲清了 Gatekeeper 原理与公证步骤，但那是**给发布者**的中文文档；卸载/还原只在 `docs/architecture/02-configuration-lifecycle.md` §8 有设计叙述（这对应用内「还原为原生 Codex」的实际行为，是可用的内部依据，不是用户文档）。
- **README 的「Current status」已过时**：中英都写「尚未用真实第三方供应商验证过 / has **not been verified against a real third-party provider yet**」，而同一批审计的 `docs/audits/2026-09-20-implementation-verification.md` 已在本机真机用真实上游 `https://api.<上游域名>/v1`（chat_completions）跑通一轮完整推理（3.13s 返回「可用」），并明确写出「证伪 README 的一条『未验证』声明」；「Not verified: the Desktop GUI model picker」也被推进到「`client_name="Codex Desktop"` 的 GUI 连接已用受管模型成功创建/恢复会话并调用 `model/list`，只剩列表渲染的视觉核验」。
  - 对陌生人是**劝退而非诚实**：读到「还没对真实供应商验证过」很可能直接不装；同时它让「诚实的偏差清单」这套自我披露机制失去可信度（真正未验证的只剩 Windows 真机、GUI 渲染、读屏）。
- **设置页「关于」的许可证一行是错的**：`settings.licenseValue` = 「未附带开源许可证 / No open-source license」，`settings.licenseNote` = 「默认保留所有权利 / All rights reserved by default」（`en.ts:723-725`、`zh-CN.ts:725-727`），而仓库是 MIT（`LICENSE`、`Cargo.toml` `license = "MIT"`、`package.json` `"license": "MIT"`）。v0.1.3 的 release notes 还专门写「**MIT 许可证**（新增 LICENSE……About 栏会出现 MIT 徽章）」——说明这是后来改坏的，不是从未有过。

### 2.4 配置流程顺滑度实测（点击次数、必须离开当前页的步骤）

- **从「已配好供应商与模型、未应用」到「已应用并重启」：2 次点击**，且不用离开当前页（待应用条 → 差异弹窗确认）。这条主线在 0.2.0 已经顺了。
- **从「零供应商」到「已应用」的实际路径**（真机推演 + 夹具验证）：向导只有 **1.5 步可用**（检测 → 第 2 步的清单）→ 点「添加供应商」后向导消失 → 「测试并应用」要自己在概览页/供应商页找 → 应用发生在**另一个页面**。必须离开当前页的步骤只有一处（第 2 步 → 供应商弹窗，这是正常的），但**第 3 步到第 4 步之间向导断掉**（1.1）。
- 让人猜的地方：① 向导消失后没有「下一步做什么」的引导（待应用条是唯一线索，它的文案确实说清了原因与后果，这点做得好）；② 向导第 3 步按钮名与行为不一致（1.1）；③ 供应商表单第一次进来有三处需要判断：选 Chat Completions 还是 Responses（默认是实验路径）、Key 必填、Base URL 不要带 `/chat/completions`（有 hint 提示）。
- 【夹具】结论边界：`saveProvider`/`saveModel` 不回写，所以「保存后列表里出现」这一步在夹具里看不到；`discoverModels` 的 3 条与四阶段探测成功是写死的，因此第 8 步的**成功路径不算产品证据**，只有错误分支的文案来自真实 locale 文件。第 10 步的 2 次点击包含夹具的合成 plan，但 `PendingApplyBar`/`ApplyConfirmDialog` 的交互结构与文案是真实代码。

### 2.5 开源协作面

- **「Build and verify」不够用**：README 给了 4 条命令，但**缺前置依赖与版本**。Node 20 / pnpm 9 / Rust 1.88（`Cargo.toml` `rust-version = "1.88"`）只出现在 CI workflow 里；仓库**没有** `rust-toolchain.toml`、`.nvmrc`、`.node-version`，`package.json` 也没有 `packageManager` 字段。而 `ci.yml` 与 `release.yml` 各有两处注释写着「与 rust-toolchain.toml 一致」——**这个文件不存在**。
- **实测能否在新机器跑通**：`pnpm test` → 15 个测试文件、**112 个用例全过**（3.45s）；`cargo test -p switch-core` → **EXIT=0**（7 个测试文件的完整套件通过）。`pnpm install --frozen-lockfile` 由 CI 每次绿灯间接验证（含 v0.2.0 那次）。`pnpm exec tauri build --debug --bundles app` **本次未跑**（耗时；`target/debug/bundle/macos/Switchelp.app` 已由既有流程产出，存在）。
- **缺协作面文件**：**没有** CONTRIBUTING、CODE_OF_CONDUCT、SECURITY.md、issue 模板、PR 模板（`.github/` 下只有 `ci.yml` 和 `release.yml`）。
- **隐私扫描脚本与 README 路径不一致**：脚本读的是 `$HOME/.switchelp-private-patterns`（`scripts/check-publish-safety.sh:121`，可用环境变量覆盖），而 README 中英两版**和脚本自己的头部注释**都写 `~/.gptswitch-private-patterns`（`README.md:73`、`README.zh-CN.md:63`、脚本第 6 行）。照 README 抄一条命令，扫描器**不会读它**（实测：未建该文件时脚本输出「未配置 …/.switchelp-private-patterns（可选）」）。脚本本身能跑通；本次运行报的一处 ✗ 来自 `docs/audits/` 里的审计产物（真实 keychain 转储片段），不是产品问题。
- **许可证兼容**：前端依赖全为 MIT/ISC/Apache-2.0/BSD/0BSD/MIT-0/CC-BY-4.0（`pnpm licenses list` 汇总：MIT 188、ISC 8、Apache-2.0 5、Apache-2.0 OR MIT 3、BSD-3 2、BSD-2 2、MIT-0 1、CC-BY-4.0 1、0BSD 1），**无 GPL/AGPL**。Rust 侧 `cargo metadata` 的 license 字段汇总里，弱 copyleft 只有 **MPL-2.0 共 5 个**（`cssparser`、`cssparser-macros`、`selectors`、`dtoa-short`、`option-ext`，随 Tauri/wry 的样式引擎进来，文件级 copyleft，与 MIT 分发兼容）和 `r-efi` 的 `MIT OR Apache-2.0 OR LGPL-2.1-or-later`（三选一，选 MIT 即可）；其余是 MIT / Apache-2.0 / Zlib / BSD / Unicode-3.0 等。**以 MIT 分发不冲突**。但应用内没有第三方许可证清单页（`settings.license*` 还写错了），发布门禁也没有 SBOM（`docs/README.md` 已列为已知偏差）。

---

## 3. 改进清单（按「改动成本 vs 收益」排序）

### S 级：1 小时内，收益立刻可见（建议先做这 5 件）

1. **README 两版下载表版本号改成 0.2.0**（文件名 + 两个 SHA256 直接用 v0.2.0 notes 里已公布的），并写明「最新版永远在 Releases 页顶部」。成本 ~10 分钟。收益：直接消除「下到 5 个版本前的包 + 读到过期说明」这个最大摩擦，也顺带修掉「README 说有 Windows 包、最新几个版本其实没有」的误导。
2. **修设置页许可证文案**：`settings.licenseValue` → MIT、`settings.licenseNote` 去掉「保留所有权利」（en/zh 各 2 条键）。成本 ~10 分钟。收益：陌生人一眼看到 MIT，而不是与 LICENSE 矛盾的声明。
3. **README 首次打开段落改成 Apple 现行路径**：「系统设置 → 隐私与安全性 → 仍要打开」放第一顺位，后面跟一段独立可复制的 `xattr` 命令块，并注明「右键 → 打开 在较新 macOS 上可能已不可用」。成本 ~20 分钟。
4. **对齐 private-patterns 路径**：把 README（中英）与脚本第 6 行注释统一成脚本实际读的 `~/.switchelp-private-patterns`（或改脚本读旧的——二选一，别两头不一致）。成本 ~5 分钟。
5. **更新 README「Current status」**：把「尚未对真实第三方供应商验证」改成「已用真实上游跑通一次完整推理（附审计链接）」，把「GUI 选择器未验证」改成「GUI 已用受管模型建会话，仅列表渲染未做视觉核验」。成本 ~20 分钟。收益：不再自我劝退，并让偏差清单重新可信。

### A 级：半天内，解决真正的产品问题

6. **让向导不再中道消失（本次最重要的产品问题）**：把 `showOnboarding` 与「供应商数量」解耦——首次运行后保持向导可见，直到用户显式完成或关闭；或在概览页/设置页提供「重新打开接入向导」入口（`onboardingForced` 已存在，只差一个按钮把它置 true）。成本 1-2 小时（含测试）。收益：主线不再断在第 3 步之前，「稍后再说」不再是不可逆的永久关闭。
7. **向导第 3 步讲清重启，并让按钮名与行为一致**：在第 3 步加入「Codex 只在启动时读配置，应用后会替你重启」的一句（复用 `codex.pendingScope` 的措辞），按钮改成「去应用」；或直接在向导内完成「生成计划 → 差异确认 → 应用 → 重启」（复用 `PendingApplyBar` 的流程）。成本 1-2 小时。
8. **供应商表单默认协议改成 `responses`**（或至少在选择器的 label 上直接标「实验」）。成本 ~30 分钟。收益：陌生人的第一步不会落在实验路径。
9. **端口被占用的出路**：折中版——错误文案本地化，并补一句可操作指引（「关掉占用 18765 的进程，`lsof -i :18765`」），同时把 `gateway_error` 的原始 `safe_detail` 换成 `messageKey` 走翻译。完整版——设置页加端口字段、核心装配支持改端口。成本 1 小时（折中）/ 半天（完整）。
10. **补协作面文件**：CONTRIBUTING.md、`.github/ISSUE_TEMPLATE/`（至少 bug_report 一份，让报障者知道要带诊断包）、`rust-toolchain.toml`（把 CI 注释里承诺的那个文件补上）、README 里加一行前置依赖版本。成本 1-2 小时。收益：贡献者一次跑通、报障有规范。

### B 级：天级，收益大但成本或前置条件高

11. **配公证 + 让 CI 出包**：`docs/development/03-signing-and-release.md` 里步骤已完整，缺的是账号所有者的凭据。顺带配 `APPLE_SIGNING_IDENTITY`/`APPLE_API_KEY`，让 Release 里的包由 CI 产出（现状是人工上传，任何贡献者打标签都拿不到包）。收益最高的一条（陌生人双击即开），但依赖外部凭据。
12. **Homebrew cask**（`brew install --cask switchelp`）：macOS 用户最常见的安装方式之一，但要先有稳定发布 + 公证，否则 cask 下载的包同样被 quarantine 拦。
13. **英文排错/FAQ**：安装、首次打开、还原原生、卸载、Windows 现状——至少在 README 加一节 Troubleshooting，或单独 `docs/troubleshooting.md` 并给英文版。
14. **应用内首次使用引导卡片**：比只读 README 的用户体验好，但它只能在应用已经能启动之后出现，**不能替代**第 3 条（README 的 Gatekeeper 说明）。
15. **Windows 凭据 helper 落地 + Windows 真机验证；Intel Mac 产物**（或至少在 README 明确写「仅 Apple Silicon，Intel 请自行构建」）。

---

## 4. 未验证项

1. **真机 GUI 全程**：本 agent 无法使用浏览器/电脑自动化（`agent.browsers` 在 subagent 里直接报 `Browser is not available in subagent`）。因此：
   - **Gatekeeper 弹窗的实际文案与「右键 → 打开」在当前 macOS 上是否仍然有效，没有做视觉/交互复现**。现有证据只有两条：`spctl --assess` = rejected / `source=Unnotarized Developer ID`；Apple 现行支持页只描述「系统设置 → 隐私与安全性 → 仍要打开」这一条路径（本次实际抓取，页面未提及 Control-click）。
   - Codex 模型选择器里是否真的出现受管模型，**未做视觉核验**（`docs/audits/2026-09-20-implementation-verification.md` 已提供「GUI 连接用受管模型建会话并调用 `model/list`」的非视觉证据）。
2. **真机上的完整「应用 → 重启 Codex → 菜单出现模型」链路未执行**：本次只启动了 `/Applications/Switchelp.app`（确认进程存活 + 网关绑定 127.0.0.1:18765 后退出），**没有对真实 Codex 执行应用/重启**，以免动 `~/.codex/config.toml` 与用户正在运行的 Codex。第 1 节第 10 步的「2 次点击」来自夹具合成计划 + 真实交互代码。
3. **Windows**：无真机。「第三方模型在 Windows 上不可用（凭据 helper 的 `.cmd` 是显式未完成的桩）」只有 README 与文档的说法，代码侧本次未逐行确认，也未实测；release 里的 Windows 包只到 v0.1.3。
4. **Intel Mac / 其他 macOS 版本**：无实机。「无 x86_64 产物」是 release 资产列表的事实，「Intel 用户拿不到包」是按此推演。
5. **上游为 chat_completions 时的响应质量、读屏实际表现、浅色主题下的首启体验**：本次未验。
6. **`pnpm exec tauri build --debug --bundles app` 与 `node scripts/g0/probe-full-loop.mjs`**：本次未执行（耗时 / 需要真实宿主）。
7. **夹具失真项**（见第 0 节与 2.4 末段）：`saveProvider`/`saveModel` 不回写、`discoverModels` 写死、`startProbe` 合成成功、`planApply` 固定计划；5173 上的 dev server 陈旧导致夹具界面显示版本号 0.1.0。
