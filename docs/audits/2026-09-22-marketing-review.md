# 营销方案审核：市场面叙事与转化路径

日期：2026-09-22 · 审核员：营销方案审核员（向产品总监汇报）
仓库：`/Users/example/Code/ProjDev/gptswitch-macapp` · 工作区版本 0.3.0（`git describe` = `v0.2.0-17-ge345bbd`，**未打标签、未发布**）
依据：`docs/audits/2026-09-22-product-director-charter.md` §四（裁决标准）与 §5.6（本岗任务书）
本文件含两部分：**A 审核结论**、**B 可落地营销方案**。本轮只读、只写本文件，未改任何源码或文档。

---

## 0. 方法与证据等级

| 级 | 含义 | 本报告中的例子 |
| --- | --- | --- |
| **M** | 本仓实测/可复核的公开数字（给命令、URL 与抓取日期） | GitHub Releases 附件下载数、traffic API、`gh api repos/*` |
| **F** | 仓库内事实（`文件:行`，行号当场核过） | `README.md:120`、`docs/README.md:3` |
| **R** | **推理**（不是结论，标明假设与方向，不给数字） | 未公证导致的流失方向与量级推理 |
| **U** | 未验证（仓库内无数据源） | 真实转化率、用户测试、GUI 菜单渲染 |

公开数据抓取日期统一为 **2026-09-22**，命令为 `gh api ...`（GitHub REST）。凡本报告出现的计数均来自该抓取，不是估计。

---

## A 审核结论

### A1 命题一｜定位与说服力：首句说对了什么、漏了什么

**首句原文**（`README.md:5-6`）：*"Make third-party providers show up in **Codex's own model picker**, and manage providers, API keys, and each model's context limit, output limit, input capabilities and reasoning levels."*

**说对了**（F）：
- 主语是"让第三方供应商的模型出现在 Codex 自己的选择器里"——这正是 PRD §1 的核心价值（`docs/01-product-requirements.md:10-12`），也是与全部参考项目的分界线（`docs/research/01-reference-projects.md:16-21`）。
- 第二句点明"本地 macOS 配置应用 + Tauri 2 + Rust 核心"（`README.md:8-9`），对技术读者的第一层信任（不是又一个套壳）到位。

**漏了什么**（F，逐条）：

1. **首句是"结果声明"，但正文随后承认这个结果未验证，首屏没有提示语**。`README.md:131-132`：*"Not verified: … the **visual** appearance of the Desktop GUI picker — the evidence above comes from the app-server and log layers, not from looking at the menu."* 首句与 131-132 严格说不算自相矛盾（一个是承诺、一个是状态），但在"不许声称未验证能力"的文案纪律下，**首屏必须给出这条边界**，否则冷启动用户的第一次信任建立在未验证的能力上。→ 修法见 B3（首屏成稿已把"已证实/未证实"分成两块）。
2. **没有说清"我不做什么"**：不导入官方订阅登录（`docs/audits/2026-09-20-product-review.md:158` 把这条列为竞品差距）、Windows 不产出且会拒绝应用配置（`README.md:75-79`）、Intel Mac 要自己编（`README.md:81-83`）。目标用户里"订阅 + 中转双持"的比例不低（同上 :158），不提前划界会让这批人读完才发现"第二工具"定位。→ 修法：首屏加一行 negative scoping（成稿已含）。
3. **信任要素被埋在文档中段**：MIT 在文末（`README.md:141`）、Key 只进钥匙串在 `README.md:113`、"不记请求正文"整句**根本不在 README**——它只存在于 PRD 的验收目标（`docs/01-product-requirements.md:117`：*"默认不上传遥测、不记录请求正文、不导出密钥"*，且同行上文 `:105` 明确写"下列均为未来验收目标，尚无实测数据"）。→ 修法：把可核验的那几条提到首屏一行（成稿已含），把"无实测数据"的目标级表述与"设计保证"分层（见 B4）。

**自述冲突全清单**（逐处找出，含修法）：

| # | 位置 | 冲突内容 | 状态 | 修法 |
| --- | --- | --- | --- | --- |
| C1 | `docs/README.md:3` vs `README.md:120`（中文对应 `README.zh-CN.md:91`） | `docs/README.md:3` 仍写"**尚未用真实第三方供应商验证过**"，而 `README.md:120` 写"has been **verified against a real third-party provider on macOS**"，`README.zh-CN.md:91` 写"**已在 macOS 上用真实第三方供应商验证过**" | **未修复**（章程 §5.6 点名） | 改 `docs/README.md:3` 的状态串为："机制已端到端跑通，并已在 macOS 上用真实第三方供应商验证过一次完整推理；**Desktop GUI 选择器的视觉渲染未验证**"。保留 `docs/README.md:7` 的"设计决策/验收目标不等于现状"总声明不变——它是这类冲突的长期防线 |
| C2 | `docs/audits/2026-09-20-out-of-box-onboarding.md:122` vs 现 `README.md:120` | 该审计断言"中英 **都写**『尚未用真实第三方供应商验证过』"——**这句本身已过期**（README 于 09-21 已改，`README.md:120` 现为已验证） | **历史记录，已不成立** | 审计是历史快照，**不要回改正文**；由产品总监在本轮汇总里加一条"该结论所指的事实已于 09-21 修复"的对照即可（本报告 §A6 已给） |
| C3 | `README.md:43-44` vs Releases 页面实际列出的 Windows 安装包 | `README.md:43-44` 说 0.2.0"publishes macOS Apple Silicon only"，读者据此认为"没有 Windows 产物"；但 Releases 页面 v0.1.1 / v0.1.2 / v0.1.3 **仍挂着可下载的 `_x64-setup.exe` 与 `_x64_en-US.msi`**（M，`gh api repos/nexsjournal/switchelp-macapp/releases`，2026-09-22） | **未修复，且有害** | 这三个安装包带的正是 `README.md:76-79` 自述"**比没用更糟**"的行为（配置照写、界面报成功、弄坏可用的 Codex）。修法：①在这三个 Release 的正文顶部加"**已撤回，请勿下载：该版本在 Windows 上会写坏配置**"；②或直接删除这三个附件；③在 `README.md:71-80` 的 Windows 段补一句"Releases 页上 ≤0.1.3 的 Windows 安装包已作废" |
| C4 | `README.md:5-6` vs `README.md:131-132` | 首句的结果声明 vs 正文的"未验证" | 未修复（文案纪律项） | 首屏加"已证实/未证实"分块（B3 已落实） |
| C5 | 设置页许可证文案 vs MIT | 曾写"未附带开源许可证 / 保留所有权利"（历史 `docs/audits/2026-09-20-out-of-box-onboarding.md:124`） | **已修复** | 现 `src/locales/en.ts:835` = `"MIT"`、`src/locales/zh-CN.ts:837` = `"MIT"`，说明文案也已改（`en.ts:834`、`zh-CN.ts:836`）。**不得当新发现报** |
| C6 | private-patterns 路径不一致（历史 :138） | 曾 README 与脚本各写一个路径 | **已修复** | 现 `README.md:105` 与 `README.zh-CN.md:76` 都写 `~/.switchelp-private-patterns`，脚本两个历史名都认（`scripts/check-publish-safety.sh:126`）。**不得当新发现报** |

**命题一小结**：首句方向正确，故障点不在"说什么"而在"**说了结果却把边界放到了第 131 行**"，以及"**信任要素不在首屏**"。六条冲突里 2 条已修复（C5/C6）、1 条是过期审计记录（C2）、**3 条真的还在**（C1/C3/C4），其中 **C3 是唯一会实际伤害用户的**。

---

### A2 命题二｜命名一致性：五个面 vs 一个名字

**现状盘点**（F，全部核过）：

| 面 | 取值 | 位置 |
| --- | --- | --- |
| 展示名 | `Switchelp` | `README.md:1`、`src-tauri/tauri.conf.json:3`（`productName`）、`package.json:2`（`name`） |
| 仓库名 | `switchelp-macapp` | `README.md:41` |
| 安装包名 | `Switchelp_0.2.0_aarch64.dmg` / `Switchelp-0.2.0-arm64.zip` | `README.md:48-49`（M：Releases 附件一致） |
| **bundle id** | `app.gptswitch.desktop` | `src-tauri/tauri.conf.json:5` |
| **内部标识符** | `model_providers.gptswitch`（配置）、`gptswitch-bridge`（二进制） | `README.md:15`、`README.md:24`、`tauri.conf.json:34` |
| **历史产物名** | `GPTSwitch-0.1.0-*`（4 个附件） | M：`gh api .../releases` v0.1.0 |
| 文档自述 | "产品名为 Switchelp（原名 GPTSwitch）……`gptswitch` 标识符一律保留，只改展示名" | `docs/README.md:7` |

**对新用户首次接触的困惑程度：中等偏低，但有三处会被真实踩到**（F + R）：

1. **Releases 页面**是下载者先落地的地方（`release.yml:336` 自己承认"下载的人先落到这里——README 里的同名说明他可能根本不会翻"）。这一页上同时存在 `GPTSwitch-0.1.0-*` 与 `Switchelp-0.2.0-*`，间隔 8 个版本、命名不同。**读者会问"GPTSwitch 是同一家的旧版，还是另一个产品？"** ——而 Release 正文（`release.yml:338-354`）**完全没有解释命名变迁**。
2. **"How it works" 图**在命名说明**之前**出现（`README.md:14-17` 的 `model_providers.gptswitch`，说明在 `:67-69`），读者先看到 `gptswitch` 再看到解释，中间隔了 50 行。
3. **安全敏感用户**会去搜 `app.gptswitch.desktop`——搜到的不是"Switchelp"，而是无关联信息，反而降低"这是个正经项目"的判断。

**建议：保留 bundle id，收口可见面**（明确推荐，不是折中）：
- **保留** `app.gptswitch.desktop` 与全部内部 `gptswitch` 标识符。`README.md:68` 的理由（改名会让旧版本的应用数据与钥匙串凭据失联）是正确的工程判断，且当前已有 0.1.x 的真实用户数据；改名方案的收益（命名统一）远小于代价（用户凭据失联）。**不要改**。
- **收口可见面①**：把命名说明从 `README.md:67-69` 上提到 "How it works" 之前，或在图里给 `gptswitch` 加一个脚注标记"（遗留标识符，见下）"。
- **收口可见面②**：在 Release 正文（`release.yml:338-354`）加一行：*"Formerly **GPTSwitch**. The bundle id is still `app.gptswitch.desktop` on purpose, so existing app data and Keychain entries keep working. The 0.1.0 artifacts below still carry the old name — do not download them."*
- **收口可见面③**：处理 `GPTSwitch-0.1.0-*` 附件（与 C3 合并处理；0.1.0 是 GPTSwitch 名下的 Windows 包，同样属于"会写坏配置"的那一批）。
- **不要做**：把安装包名改成 `gptswitch-*` 之类——那会让**唯一一致的三个面**（展示名/仓库名/包名）也乱掉。

---

### A3 命题三｜下载安装摩擦的量化（只给可解释推理，不给编造数字）

**先给实测基线**（M，`gh api repos/nexsjournal/switchelp-macapp/...`，抓取 2026-09-22）：

| 指标 | 实测值 | 说明 |
| --- | --- | --- |
| 全部 Release 附件下载总数 | **7** | v0.1.0: 5（zip 2 / dmg 1 / exe 1 / msi 1）、v0.1.3: 1（dmg）、v0.1.6: 1（dmg）；**其余全部为 0** |
| **v0.2.0（当前版本）附件下载数** | **0 / 0** | `Switchelp_0.2.0_aarch64.dmg` = 0、`Switchelp-0.2.0-arm64.zip` = 0 |
| Star / Fork | **1 / 0** | 仓库创建于 `2026-09-18T04:43:39Z` |
| 仓库浏览（traffic API，可得的 14 天窗口） | **20 次 / 1 独立访客** | `traffic/views` |
| 克隆 | 1009 次 / 225 独立 | `traffic/clones`；**但 09-20 当天浏览为 0 而克隆为 228** → 克隆不可归因于人类流量，属自动化/镜像 |
| 外部引荐来源 | **无**（`traffic/popular/referrers` 只有 `github.com`，14 次 / 1 独立） | 即：**至今没有任何外部渠道把人送进来** |

**由此得到的第一个结论（M，不是推理）**：产品**还没有冷启动流量**。v0.2.0 零下载、零外部引荐、唯一访客就是维护者自己。因此**任何"流失率 / 转化率"数字此刻都是无根据的**——本报告不给，也不允许被引用为结论。

**未公证摩擦的方向性推理（R，明确标注）**：

- 摩擦不是"装不上"，而是**一次带恶意软件观感的拦截 + 一个需要离开下载流程并输入密码的动作**。已做的两条指引（`README.md:51-65` 的系统设置路径 + `xattr` 命令；`release.yml:338-349` 把同一段放在 Release 正文——这一手做对了，理由见 `release.yml:336` 的注释）都是**正确的**，且 `README.md:55-56` 主动纠正了"右键 → 打开"已失效这一点，是加分项。
- **受影响的边际人群**：目标是"习惯 Homebrew / 终端的个人开发者"（PRD `docs/01-product-requirements.md:7`）。对这个人群，`xattr` 一条命令是**已知技能**；所以流失不发生在"做不到"，而发生在"**看起来像病毒，于是不试了**"。这是**信任断点**，不是能力断点。
- **量级方向（不给数字）**：损失上界 = 在 Gatekeeper 弹窗处放弃的人数比例；下界 = 0（熟悉的人在 10 秒内过）。**公证把这个区间整体压成"双击即开"**，即把一次需要判断 + 输密码的多步流程变成零决策动作。由于公证是**一次性凭据配置 + CI 自动化**（`release.yml:238-245` 已有 Gatekeeper 通过性断言，配好凭据即自动产出公证包），它是整条漏斗上**性价比最高的一处修复**。→ 这是推理，不是实测；要变成结论必须做一次带 UTM 的渠道投放后看数据。
- **另一个被忽略的摩擦：Intel Mac**（`README.md:81-83`）与**端口固定 18765 占用即网关不启动**（历史 `docs/audits/2026-09-20-out-of-box-onboarding.md` 表格里的"死路"项）。后者对开发者是典型场景（本地已有服务占端口），且设置页无改端口入口。

**降低摩擦的具体动作清单**（"已做"不再重复）：

| # | 动作 | 成本 | 为什么排这个位置 |
| --- | --- | --- | --- |
| 1 | **办下公证凭据**，接进 `release.yml` 的 macOS job（`APPLE_ID` / `APPLE_TEAM_ID` / `APPLE_APP_SPECIFIC_PASSWORD`）。`release.yml:241` 已有 "Gatekeeper 通过" 断言，凭据一配即生效 | 一次性，需账号方提供 | 唯一能把"多步 + 密码 + 恐吓弹窗"变成"双击"的动作 |
| 2 | **给 README 加一个可复制的安装命令块**，与 dmg 并列（`curl -L <zip> -o /tmp/s.app.zip && unzip … && xattr -dr com.apple.quarantine /Applications/Switchelp.app`）。让熟悉终端的人**一行装完**，不必在浏览器与系统设置间切换 | ~30 分钟 | 直接把受众人群的优势（终端）变成摩擦的解法 |
| 3 | **发一个 Homebrew Cask**（`brew install --cask switchelp`）。Cask 对未公证包可用（用户拉取时加 `--no-quarantine` 或 Cask 内声明） | 半天 | Cask 是这个人群的默认安装入口，且是**可被搜索到的分发渠道**（渠道价值见 B2） |
| 4 | **补一份用户向的"卸载 / 还原原生"说明**（现缺；历史 `docs/audits/2026-09-20-out-of-box-onboarding.md` §2.3 已登记"没有安装排错 / 常见问题 / 卸载说明"） | 1 小时 | 安装摩擦不止"装上"，还有"敢不敢装"——能一键回滚才是信任前提 |
| 5 | **首启按钮**：应用内首次运行时，如果检测到 quarantine 残留或用户是从 dmg 直接运行，主动弹一条"macOS 可能拦过你，原因与两条解法"，并给 `xattr` 一键执行入口 | 半天 | 把摩擦的解法**放进产品里**，而不是要求用户回 README 找 |
| 6 | **撤回作废的 Windows 0.1.1–0.1.3 安装包**（见 C3） | ~15 分钟 | 这是唯一一条**会主动伤害用户**的现存问题 |
| 7 | **端口占用的出路**：至少把 `gateway_error` 的原始中文串走翻译键并补一句 `lsof -i :18765` 指引（历史审计"折中版"） | 1 小时 | 开发者常见场景，且英文用户会读到中文错误串 |
| 8 | **确认 `latest.json` 更新源指向 0.3.0**（`tauri.conf.json:40` 指向 `releases/latest/download/latest.json`）：0.3.0 必须被标为 latest，否则老用户的应用内更新报"更新源里没有可读的更新清单"（`release.yml:306` 自己警告过这一条） | ~10 分钟 | 更新链路是留存的一部分，且它的失败文案很难懂 |

---

### A4 命题四｜差异化是否被人看得到

**差异化本体**（F，`docs/research/01-reference-projects.md:16-21` 的对比表 + `docs/audits/2026-09-20-product-review.md:162-171`）：

> 唯一一个把第三方模型写进 Codex **原生选择器**、同时把 Key **留在系统凭据库**、把写入做成**可回滚事务**的跨平台 **MIT** 工具。

逐条查它**在哪个可见面上出现**：

| 差异点 | README | Release 正文 | 首屏（应用内） |
| --- | --- | --- | --- |
| 写进原生选择器（官方配置 + 本地目录） | ✅ `README.md:5-6`、`:19-21` | ❌ | ❌（历史审计已判"不清晰"，`docs/audits/2026-09-20-product-review.md:105`） |
| Key 只进系统凭据库 | ✅ `README.md:113`（在"Security boundaries"表里，属文档中段） | ❌ | ❌ |
| 计划 → CAS → 原子替换、可还原 | ✅ `README.md:31-32` | ❌ | ❌ |
| MIT | ✅ 但**只在文末一行** `README.md:141` | ❌ | ❌（且应用内许可证文案刚被修回 MIT，见 C5） |
| 跨平台（对比竞品） | ⚠️ 只讲"Windows 不产出"，没讲"竞品 macOS 壳无法覆盖 Windows" | ❌ | ❌ |

**结论（F）**：差异化在 README **"有"，但散落在中后段且不成块**；在 **Release 正文（`release.yml:338-354`）里彻底没有**——而 Release 页恰恰是下载者先落地的地方（`release.yml:336`）。**"竞品对比"这个最容易被转发、最容易被社区接受的形态，全仓库一处都没有。**

**可立即用上的公开事实（M，抓取 2026-09-22，`gh api`）**：

| 项目 | Star | License | 备注 |
| --- | --- | --- | --- |
| `openai/codex`（官方，社区所在地） | **125,725** | Apache-2.0 | `has_discussions: true`；这就是社区渠道的体量 |
| `jlcodes99/cockpit-tools` | 18,089 | — | 多 IDE 账号管理（含 Codex） |
| `Loongphy/codex-auth` | 2,730 | — | Codex 账号切换 |
| `Lampese/codex-switcher`（参考项目之一） | **828** | **null（无 LICENSE 文件）** | `docs/research/01-reference-projects.md:115` 已记"未发现项目级 LICENSE" |
| `AITabby/codexsplit`（参考项目之一） | **723** | **null（无 LICENSE 文件）** | 同上 `:83`；且其 macOS DMG **同样未公证**（公开 README 自述，抓取 2026-09-22） |
| `HeiGeAi/heige-codex-skin-studio` | 472 | — | Codex Desktop 主题工具——说明"动 Codex Desktop 本身"的需求真实存在 |
| **`nexsjournal/switchelp-macapp`** | **1** | MIT | 本产品 |

→ **两个可用的、可核验的差异点**：①两个最接近的参考项目**都没有 LICENSE**（M），而 Switchelp 是 MIT；②CodexSplit 的 macOS 包**也未公证**（公开自述），所以"未公证"在同类工具里**不是异常**，可以坦白说、不必遮掩。这两条都能直接写进文案，且都是可复核的公开事实。

**"接进原生选择器未验证"对文案的约束**（硬规则，F）：
- `README.md:131-132` 与 `README.zh-CN.md:95`：证据来自 app-server 与日志层，**不是看着那个菜单**。
- 因此一切文案**不得**出现"模型已出现在 Codex 菜单里"这类完成时陈述。允许的措辞上限是：**"它写的是让模型进入原生选择器的那份官方配置；我们已在 app-server 与日志层验证过受管模型会被 `model/list` 列出、Desktop 会用受管模型开会话；菜单里的视觉渲染待你在真机上实测。"**
- 差异化表述因此要落在**机制**（写官方配置、Key 只进钥匙串、事务可回滚）而不是**结果**（模型看得见）。B3 成稿已按此写。

---

### A5 命题五｜可落地营销方案

见 **Part B**（B1 人群画像 / B2 渠道 / B3 中英首屏成稿 / B4 信任要素 / B5 首发节奏）。

---

### A6 未验证项（不得编造，逐条说明数据源状况）

| 项 | 状态 | 数据源状况 |
| --- | --- | --- |
| 真实下载量 | **已实测**：全部 Release 附件下载合计 7；v0.2.0 = 0 | `gh api repos/nexsjournal/switchelp-macapp/releases`，2026-09-22 |
| 真实访问量 | **已实测（14 天窗口）**：20 次浏览 / 1 独立；外部引荐 0 | `gh api .../traffic/views`、`.../traffic/popular/referrers`，2026-09-22。**traffic API 只回 14 天，且不含转化口径** |
| **转化率（浏览→下载→首发成功）** | **未验证，不可计算** | 无任何漏斗埋点；traffic 与 downloads 不同源、不同窗口，不能相除得出率 |
| Gatekeeper 处的放弃比例 | **未验证** | 无埋点。本报告只给方向性推理（R），不给数字 |
| 用户测试 / 可用性测试 | **未验证** | 仓库内无任何用户研究产物（无 `docs/research/` 用户访谈、无 issue） |
| GUI 菜单的视觉渲染 | **未验证**（`README.md:131-132`） | 需真机 Codex + 人工观察 |
| 0.3.0 的公证状态 | **未验证** | `dist-release/` 有 0.3.0 产物，但公证状态无法从仓库判定；0.3.0 未打标签（`git tag` 止于 `v0.2.0`） |
| "无遥测 / 不记正文"是否为**架构保证** | **部分可核**：依赖表无任何分析 SDK（`package.json:18-24` 仅 Radix/Tauri/lucide/react），CI 有隐私扫描 job（`.github/workflows/ci.yml:141-149` 跑 `scripts/check-publish-safety.sh`） | 但 PRD 把该条列为"未来验收目标，尚无实测数据"（`docs/01-product-requirements.md:105,117`），**且无第三方审计**。文案按"设计保证"表述，不按"已审计"表述 |
| 各竞品真实下载量 | **未验证** | GitHub 不公开他人 release 下载数之外的漏斗；本报告只用了可复核的 Star 与 License |

---

### A7 与历史审计的对照

**本轮复核仍存在的（历史已登记，未修复）**：
- `docs/README.md:3` 的"尚未用真实第三方供应商验证过"与主 README 冲突——章程 §5.6 已点名。**本轮新增的信息**：冲突方向与历史记录相反——`docs/audits/2026-09-20-out-of-box-onboarding.md:122` 记的是"README 写未验证"，而 README 已在 09-21 修复（`README.md:120`），**留在后面的是 `docs/README.md:3`**。
- 无卸载/还原用户文档、`docs/` 全中文无英文入口——`docs/audits/2026-09-20-out-of-box-onboarding.md` §2.3 已登记，**仍存在**。

**本轮复核为"已修复"的（推翻历史条目，不得再报为问题）**：
- 设置页许可证文案（历史 `:124`、S 级 item 2）：已修为 MIT（`src/locales/en.ts:835`、`src/locales/zh-CN.ts:837`）。
- private-patterns 路径不一致（历史 `:138`、item 4）：已对齐（`README.md:105`、`scripts/check-publish-safety.sh:126`）。
- README 下载表的版本号/SHA 与"右键打开"表述（历史 S 级 item 1、3）：现表为 0.2.0 且 SHA 与附件一致，Gatekeeper 段落已按 Apple 现行路径重写（`README.md:51-61`）。

**本轮新增的结论（历史审计没有的）**：
1. **冷启动基线实测**：v0.2.0 零下载、零外部引荐、1 star（M）。这直接把章程 §六 第 5 条的"会静默吃掉大部分冷启动流量"从担忧变成**当前的第一障碍不是流失而是没有流量**——顺序变了，方案的重心也随之变（先分发，再谈摩擦）。
2. **C3：Releases 页仍挂着 0.1.1–0.1.3 的 Windows 安装包**，而 README 自述那批行为"比没用更糟"（`README.md:76-79`）。历史审计谈了"零产物 Release 对贡献者是信息缺口"，**没有谈"历史产物仍在可下载"**。
3. **Release 正文（`release.yml:338-354`）不含任何差异化与信任要素**——下载者先落地的页面只讲了怎么过 Gatekeeper。历史审计聚焦 README，未评估 Release 作为营销面。
4. **命名困惑的第三个落点**：不只是 bundle id，而是**Releases 页上 `GPTSwitch-0.1.0-*` 与 `Switchelp-*` 并存且无解释**。
5. **两个可用的公开事实**：两个最接近的参考项目均无 LICENSE（`gh api` 2026-09-22），CodexSplit 的 macOS 包同样未公证（公开自述）。这是"MIT + 坦白未公证"这一叙事的事实底座。

**不属于本岗、不重复计入的**：首屏信息架构、术语收口、几何/对比度、四态模型——属设计与需求审核员（`docs/audits/2026-09-20-product-review.md:105,:121,:128` 已登记）。

---

## B 可落地营销方案

### B1 目标人群画像

**主画像：中转 API 的重度 Codex 用户（"手里有 Key 的独行开发者"）**
- 已装 Codex Desktop，天天用；同时握着 1–3 家中转/聚合服务商（或自建网关），因为官方配额不够或想按量付费。
- 技术栈：macOS Apple Silicon、`brew` 与终端是日常工具、读过 Codex 的 `config.toml` 文档或至少知道有这个文件（PRD `docs/01-product-requirements.md:7`）。
- **他们要的不是"配置管理"，是"我的 Key 能用上，且别弄坏我现在的 Codex"**（章程 §六 第 2 条）。
- 触发场景（第二、三次使用）：换 Key、加一家供应商、某次请求失败来看原因（`docs/audits/2026-09-20-product-review.md:171`）。
- 决策路径：GitHub Release → README 首屏 8 秒 → 下载 → 首次打开 → 一次 apply。**任何一步的"不确定"都会让他回到"我自己手改 config.toml"**，因为手改是他已有的、可控的替代方案。

**次画像：在多工具间切换的"第二工具使用者"**
- 已经在用 cockpit-tools / codex-switcher / CodexSplit 之一（M：分别 18,089 / 828 / 723 star）。
- 这是最大的可争取池，但**当前产品对他们缺一条硬理由**（`docs/audits/2026-09-20-product-review.md:158`：无官方订阅登录导入、无 Key 故障切换）。→ 方案不把他们当首发目标，只在渠道内容里留钩子（"MIT + 不碰你的历史"）。

**明确不作为首发目标的**：团队/企业（PRD §1 排除）、Windows 用户（会拒绝应用配置）、要综合工作台的人、以及"只想要个聊天客户端"的人。

---

### B2 渠道（具体到渠道名与投放形态）

按"**先有流量、再谈转化**"排序（依据 A3：目前外部引荐为 0）。

| 序 | 渠道 | 具体形态 | 为什么是它 |
| --- | --- | --- | --- |
| 1 | **GitHub Releases + 仓库本身** | ①`v0.3.0` 正式 Release（正文含首开指引 + 差异化块 + 命名说明）；②给仓库加 topics：`codex`、`codex-desktop`、`model-provider`、`openai-codex`、`tauri`、`macos`、`mit-license`；③发一个 Homebrew Cask（`brew install --cask switchelp`） | 所有其他渠道的落点；topics 是 GitHub 站内**唯一**的自然发现入口；Cask 是这个人群的默认安装路径 |
| 2 | **`openai/codex` GitHub Discussions**（M：`has_discussions: true`，仓库 125,725 star） | 在 **Show and tell / Integrations** 类目发一篇：标题 *"Switchelp — a local macOS app that writes Codex's own provider config for third-party models (MIT)"*；正文以**技术说明为主**（`model_catalog_json` 的替换语义、`wire_api` 官方只支持 `responses` 所以 Chat 走网关翻译、Key 只进 Keychain），把"未验证 GUI 渲染"写在正文里 | 这是**唯一**官方托管、且已在讨论 Codex 扩展的开发者聚集地。发技术说明而不是广告，是这类社区能否接受的开关 |
| 3 | **Hacker News — Show HN** | *"Show HN: Switchelp – put third-party models into Codex's own picker (MIT, local-only)"*。发帖时点：**周二至周四，美东上午**。第一条自评**主动交代**未公证 + GUI 渲染未验证 + `model_catalog_json` 会替换列表 | Show HN 是工具类冷启动的核心放大点；主动交代缺点是 HN 的通货，隐瞒会被反噬 |
| 4 | **Reddit**：`r/ChatGPTCoding`、`r/LocalLLaMA`、`r/macapps` | 各自原生发帖（不要同一段复制）：`r/LocalLLaMA` 走"把本地/自建端点的模型接进 Codex"角度；`r/macapps` 走"未公证但 MIT、可一键还原"角度 | `r/LocalLLaMA` 正好是"自己有端点"的人群，与主画像重合 |
| 5 | **`community.openai.com` → Codex 类目**（已核实存在该 category，抓取 2026-09-22） | 以**回答问题**为主：找"如何让 Codex 用自定义 provider / 模型"的既有帖子，给出准确答案并顺带提一句工具。**不发纯广告帖** | 论坛对推广容忍度低，但对"能解决问题的人"容忍度高 |
| 6 | **中文开发者社区** | ①**V2EX** `分享创造` 节点（标题走"我给 Codex 写了个本地工具，让中转 Key 的模型进原生选择器"）；②**LinuxDo**（Codex 讨论活跃）；③**少数派 sspai** 投稿（正式文章，不是帖子）；④**掘金**技术文（讲 `model_catalog_json` 替换语义与事务写入的实现）；⑤**即刻 / 微信公众号**（短文 + 一张首屏截图） | `docs/` 全中文（`docs/README.md` 自述）反而是中文渠道的优势：能给出英文渠道给不出的实现细节 |
| 7 | **垂直收录** | `awesome-codex` 类清单 PR、`awesome-macos-apps` 类清单 | 被动长尾，成本极低 |
| 8 | **不做** | Product Hunt（人群偏泛，与"个人开发者 + 自备 Key"错位）、Twitter/X 无受众基础时不适合作为首发主渠道 | 渠道要少而准 |

**投放纪律（对每条渠道都适用）**：
- 每条帖子的能力声明**必须与 `README.md:120-132` 逐字对齐**——"已实测"的三条可以讲，"模型出现在菜单里"不能讲。
- 未公证这件事**每条都前置写明**（见 B5），不要等评论问。
- 中文帖不是英文帖的翻译，而是**换角度**：英文讲"官方配置 + 事务"，中文讲"Key 不落 config.toml + 一键还原"。

---

### B3 中英双语首屏文案（成稿，可直接用）

> 说明：下列为 **README / 落地页首屏**成稿，非要点。能力声明严格按 `README.md:120-132` 的已验/未验分界；`model_catalog_json` 的替换语义（只提一次，但必须提）与"不做订阅登录/无 Windows 产物"的划界都已含入。

**English**

```markdown
# Switchelp

Put your own API models into Codex's own model picker — without hand-editing `config.toml`.

A local macOS app for developers who already run Codex Desktop and already hold a custom or relay
API key. Point Switchelp at your provider and it writes Codex's **official** config for you:
providers, keys, and each model's context window, output limit, input capabilities and reasoning
level. Upstream keys go to the macOS Keychain and **never into `config.toml`**; the host only ever
sees a local gateway token. Every write is **plan → digest check (CAS) → atomic replace**, and one
click restores native Codex. MIT licensed. Not a subscription importer, not a chat client, not a
workbench: it edits the fields it manages and leaves the rest of your config alone.

**What we have proven, and what we haven't:**

- **Proven on macOS against a real third-party provider.** A real upstream answered a completion
  routed through the local gateway; a real Codex `model/list` returns the managed models; the
  Desktop app-server opened a session on a managed model (`thread/start` with
  `client_name="Codex Desktop"`); output-limit enforcement, reasoning-level mapping and modality
  rejection are backed by the request parameters a real upstream received.
- **Not proven: the picker's on-screen rendering.** That evidence comes from the app-server and log
  layers, not from looking at the menu on your machine. Read
  [Current status](README.md#current-status) before you apply.
- **Read this before you apply:** `model_catalog_json` **replaces** the host's model list. While the
  tool is applied, Codex's built-in models leave the picker until you restore native mode. Coexist
  mode (Bridge) keeps both.
- **Platforms:** macOS Apple Silicon. Intel Mac builds from source. Windows builds are not published,
  and the app refuses to apply configuration there instead of writing a broken config.

MIT · no telemetry · no request-body logging · keys stay in the Keychain · managed fields only, the
rest of your `config.toml` is preserved.
```

**简体中文**

```markdown
# Switchelp

让第三方 API 的模型出现在 Codex 自己的模型选择器里——不用手改 `config.toml`。

给已经装了 Codex Desktop、手里已经有自定义或中转 API Key 的个人开发者用的本机 macOS 应用。
把供应商填进来，Switchelp 替你写 Codex 的**官方配置**：供应商、Key，以及每个模型的上下文、
输出上限、输入能力与推理档位。上游 Key 只进 macOS 钥匙串，**永远不写进 `config.toml`**，
宿主拿到的只是本机网关令牌。每次写入都走**计划 → 摘要校验（CAS）→ 原子替换**，一键可还原
原生 Codex。MIT 许可。它不是订阅导入器、不是聊天客户端、不是综合工作台：只改它受管的字段，
其余配置原样保留。

**已经证实的，和还没证实的：**

- **已在 macOS 上用真实第三方供应商验证过。** 真实上游返回过一次经本机网关路由的完整推理；
  真实 Codex 的 `model/list` 能列出受管模型；Desktop app-server 用受管模型真实开过会话
  （`thread/start`，`client_name="Codex Desktop"`）；输出上限执行、推理档位映射、模态拒绝
  都以真实上游收到的请求参数为证。
- **尚未证实：选择器在你屏幕上渲染出来的样子。** 以上证据来自 app-server 与日志层，不是
  在你机器上看着那个菜单得出的。应用之前请先读[「当前状态」](README.zh-CN.md#当前状态)。
- **应用前请务必知道：** `model_catalog_json` 是**整体替换**宿主的模型列表。工具处于已应用
  状态时，Codex 的原生模型会从选择器里消失，直到你还原原生模式；共存模式（Bridge）可以让
  两者同时保留。
- **平台：** macOS Apple Silicon；Intel Mac 从源码构建；不发布 Windows 产物，且应用在
  Windows 上会拒绝应用配置，而不是写坏你的配置。

MIT · 无遥测 · 不记录请求正文 · Key 只进钥匙串 · 只改受管字段，其余 `config.toml` 原样保留。
```

**首发帖用的一句话版（渠道正文首段，中英各一条）**：
- EN: *"Switchelp is a local macOS app that writes Codex's own provider config so your third-party models are usable from Codex itself — keys stay in the Keychain, every write is atomic and reversible, MIT. It is signed but not notarized yet, and the picker's on-screen rendering is the one thing we haven't verified."*
- ZH: *"Switchelp 是个本机 macOS 应用，它替你写 Codex 自己的供应商配置，让第三方模型能在 Codex 里直接用；Key 只进钥匙串，每次写入原子且可回滚，MIT。目前已签名未公证，唯一没验证的是选择器在屏幕上的渲染。"*

---

### B4 信任要素（每条标注可核验级别）

| 信任要素 | 可核验级别 | 依据 | 文案口径 |
| --- | --- | --- | --- |
| MIT 许可 | **可核验** | `LICENSE:1`、`package.json:4`、`Cargo.toml`、M：`gh api .../repos/nexsjournal/switchelp-macapp` → `license: "MIT"` | 可直接写"MIT" |
| 无遥测 | **设计保证，可部分核验** | 依赖表无任何分析 SDK（`package.json:18-24`） | 写"no telemetry"；**不写"已审计"** |
| 不记录请求正文 / 不导出密钥 | **PRD 目标，尚无实测数据** | `docs/01-product-requirements.md:117`；同页 `:105` 自述"下列均为未来验收目标，尚无实测数据" | 写"设计上不记录请求正文"，并说明机制（allowlist 结构化提取 + 二次脱敏，`README.md:116`）。**不写"已证明"** |
| Key 只进钥匙串 | **可核验（代码层）** | `README.md:113`：只有系统凭据库，SQLite 只存引用与掩码；`config.toml` 只见本地 token（`README.md:29-30`） | 可直接写；这是最强的单条差异 |
| 配置可回滚 | **可核验（代码 + CI）** | `README.md:31-32`；应用管线有 CI job（`.github/workflows/ci.yml:88` "应用管线（真实产物）"） | 可直接写"plan → CAS → atomic replace，一键还原" |
| 零上传 | **设计保证，可部分核验** | 上游 Key 不出本机；网关只绑 `127.0.0.1`、每次启动换新令牌、拒绝带 `Origin` 的请求（`README.md:114`） | 写"your keys never leave your machine"；注意内容中心会抓 RSS/GitHub 公开源（`docs/README.md:55`），属外发请求，**文案里不要写成"应用完全无网络请求"** |
| 写入的诚实（不假装成功） | **可核验** | `README.md:31-32`（只到"等待宿主重载"）、`README.md:75-79`（Windows 拒绝写而不是假装成功）、`README.md:33-34`（协议损失记账） | **这是最该被讲但完全没被讲的信任要素**：一个"宁可报未完成也不报成功"的工具 |
| 隐私扫描门禁 | **可核验** | `.github/workflows/ci.yml:141-149` | 可在贡献者向内容里提一句 |
| 未公证的坦白 | **可核验** | `README.md:48`、`release.yml:338-349`；且 CodexSplit 的 macOS 包同样未公证（公开自述，2026-09-22） | 主动讲，并说明是"凭据未配，不是不想配"，给出补齐路径 |

---

### B5 首发节奏（0.3.0 发布按什么顺序放出什么信息）

**前提事实**：0.3.0 **未打标签、未发布**（`git tag` 止于 `v0.2.0`；`git describe` = `v0.2.0-17-ge345bbd`）；`dist-release/` 已有 0.3.0 产物；v0.2.0 零下载、零外部引荐（M）。

**顺序与内容（每一格是"放出什么信息"，不是"发一条帖"）**：

| 阶段 | 动作 | 放出的信息 |
| --- | --- | --- |
| **T-7 ~ T-1：修地基（不发任何推广）** | ①改 `docs/README.md:3` 的状态串（修 C1）；②撤回/标注 v0.1.1–0.1.3 的 Windows 安装包（修 C3，**发布前必做**）；③首屏按 B3 成稿重写（`README.md` + `README.zh-CN.md`）；④补"卸载/还原"说明；⑤办下公证凭据并让 CI 通过（`release.yml:241` 的断言必须真过）；⑥确认 0.3.0 的 `latest.json` 会让老用户更新到 0.3.0 | **不对外**。理由：Release 页是落点，带着 C3 与新户会踩的坑上场，等于把流量倒进漏桶 |
| **T-0：打标签 + Release**（顺序固定） | ①`git tag v0.3.0` 并推送 → ②Release **先成草稿**、附上**公证过的** dmg/zip → ③手写 Release 正文（模板见下）→ ④发布 | 落地页正文，顺序即优先级：**首开指引 → 一句话价值 + 差异化块 → 已证实/未证实 → `model_catalog_json` 替换警告 → 命名说明（GPTSwitch/bundle id）→ 平台划界 → 校验和** |
| **T+0 小时级：仓库自身可被发现** | 加 GitHub topics；确认 Release 已标 latest | "这是一个 macOS 上给 Codex 用第三方模型的 MIT 工具"（topics 是站内发现的唯一入口） |
| **T+1：最大的一块社区** | `openai/codex` Discussions 发技术说明（B2 第 2 行） | **技术深度**：`wire_api` 官方只支持 `responses`（公开文档可核，2026-09-22），所以 Chat 走网关双向翻译且损失记账；`model_catalog_json` 是启动时加载且整体替换；Key 只进 Keychain |
| **T+2：英文大流量** | Show HN（美东上午）+ `r/ChatGPTCoding` | **诚实与克制**：第一条评论交代"未公证 / GUI 渲染未验证 / 会替换模型列表"，并给出"为什么仍然值得试"（MIT、可一键还原、只改受管字段） |
| **T+3：中文** | V2EX 分享创造 + LinuxDo + 即刻/公众号短文 | **换角度**：Key 不落 `config.toml`、一键还原、中文文档齐全（`docs/` 全中文是优势） |
| **T+4：长尾** | 掘金/少数派正式文章 + awesome 清单 PR + `r/LocalLLaMA`、`r/macapps` | **纵深内容**：事务写入（plan→CAS→atomic replace）的实现、隐私 allowlist、以及"不假装成功"的工程取舍 |
| **T+7：复盘** | 看 `traffic/views`、`traffic/popular/referrers`、各附件 `download_count` 的**增量**，与 A3 基线对照 | **第一次真正有了可算的漏斗**：此时才允许讨论转化率；此前的任何数字都是无根据的 |
| **T+14：第二个版本窗口** | 若公证已生效：把"double-click to open"做成一条可转发的事实讲；若还没生效：把首启按钮（动作 5）做出来补位 | "摩擦已经没有了"或"我们把摩擦的解法放进了产品里" |

**"未公证"这件事怎么前置说明（三条纪律）**：
1. **落点先说**：Release 正文第一段（已在 `release.yml:338-349` 做对，保留并把它扩成"首开指引 + 差异化"两段）；README 下载表 `README.md:48` 保留"not notarized"标记。
2. **每条社交帖都说**：以"作者主动交代"的姿态，而不是等评论问。措辞用 B3 的一句话版末句。
3. **说清原因与时间表**，并给同类参照：*"Signing and notarization are two gates and we only have the first; the credentials are on the account owner. One other Codex-side tool ships notarized-less too — but ours will be notarized by 0.3.x, and until then the two-step fix is in the release notes."*

**Release 正文模板（建议替换/扩充 `release.yml:338-354`，保持中英双语随标签走）**：

```markdown
## Switchelp 0.3.0

把第三方 API 的模型接进 **Codex 自己的模型选择器**——只写 Codex 的官方配置，Key 只进钥匙串，
每次写入原子且可还原。MIT。

**与同类工具不同的三件事**：① 写进 Codex **原生**选择器（官方 provider + `model_catalog_json`，
不是自绘列表）；② 上游 Key 只进系统凭据库，`config.toml` 里只有本机网关令牌；③ 写入走
计划 → 摘要校验 → 原子替换，一键还原原生 Codex，未受管字段原样保留。

**已证实**：真实上游经本机网关返回过完整推理；真实 Codex `model/list` 列出受管模型；Desktop
app-server 用受管模型开过会话。
**未证实**：选择器在你屏幕上的渲染；Windows 真机；读屏表现。

**应用前必读**：`model_catalog_json` 会**整体替换**宿主的模型列表，原生模型会从选择器消失，
直到你还原原生模式。共存模式（Bridge）可两者都在。

## 首次打开（macOS）

包已用 Developer ID 签名，但**尚未公证**，所以 macOS 会拦一次。两条路，先试第一条：
…（保留 release.yml:341-349 原文）

## 命名

原名 **GPTSwitch**。bundle id 仍是 `app.gptswitch.desktop`（有意保留，让旧版本的应用数据与
钥匙串凭据继续可用）。**请勿下载 0.1.0 的旧产物**，它们仍是旧名，且 Windows 版会写坏配置。

**Windows 与 Intel Mac**：不产出自动构建。原因与替代做法见 [README 的下载与安装一节]。
```

---

## 一页纸结论

1. **首句方向对，问题在"结果说在前面、边界藏在 131 行"，且信任要素不在首屏**（`README.md:5-6` vs `:131-132`；MIT 在 `:141`；"不记请求正文"只在 PRD `:117`）。
2. **六条自述冲突里 3 条真还在**：`docs/README.md:3` 与 `README.md:120` 直接矛盾；Releases 页仍挂着会被 README 自己判为"比没用更糟"的 Windows 0.1.1–0.1.3 安装包（`README.md:76-79` vs `gh api .../releases`）；首屏未标注能力边界。另 2 条已修复（设置页 MIT、private-patterns 路径），1 条是过期审计记录。
3. **命名不必改名**（`app.gptswitch.desktop` 保留是对的），要收口的是三个可见面：Releases 页的旧名产物、`How it works` 图里的 `gptswitch`、Release 正文缺命名说明。
4. **当前第一障碍不是转化率而是零流量**（M：v0.2.0 下载 0、外部引荐 0、1 star）。所以方案重心是"先分发、后优化"；未公证的摩擦是真实存在的信任断点，但顺序上排在拿到流量之后，而公证凭据应当现在就去办（一次性成本）。
5. **差异化在 README 有、在 Release 正文没有**——而 Release 是落点。补一个差异化块 + 两个可核验的公开事实（两个最接近的参考项目都无 LICENSE；CodexSplit 的 macOS 包同样未公证）。
6. **所有渠道文案的能力上限**：可以讲"写的是让模型进原生选择器的官方配置、已在 app-server 与日志层验证";不可以讲"模型出现在菜单里"。B3 的中英首屏成稿已按此写，可直接用。

---

**报告结束。** 本文件为营销专项唯一产出，未改动任何源码或既有文档。
