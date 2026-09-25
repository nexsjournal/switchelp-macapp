# Switchelp 调研与设计文档

版本：0.3 · 初稿：2026-09-17 · 更新：2026-09-22 · 状态：机制已端到端跑通（前端 + Rust 核心 + 本机网关 + 事务写入），并**已在 macOS 上用真实第三方供应商验证过一次完整推理**（真实上游经本机网关返回、`model/list` 列出受管模型）；**尚未验证的是 Desktop 模型选择器在屏幕上的渲染**——那一层证据来自 app-server 与日志层，不是看着菜单。详见[全产品审核裁决](audits/2026-09-22-audit-verdict.md)与[人工层探针记录](appendix/2026-09-22-e2e-probe-run.md)。

> 本文档集是设计与调研记录，其中一部分能力仍是**目标**而非现状。实现与文档不一致的地方集中列在文末「与文档的已知偏差」，改代码前先看那一节。

产品名为 **Switchelp**（原名 GPTSwitch，仓库同步更名为 `switchelp-macapp`）。应用标识与签名主体沿用 `app.gptswitch.desktop`：标识决定应用数据目录与密钥库条目位置，改名会让已有用户的数据与凭据失联，因此代码与配置里的 `gptswitch` 标识符一律保留，只改展示名。本文档中的技术选型、性能数字和交互尺寸是本项目的设计决策或验收目标，不是现有产品已经具备的能力。

## 核心结论

做一个专注于 Codex 的本地供应商与模型配置工具：管理多个供应商和 API Key，让导入的模型出现在 **Codex 自己的模型选择器** 中，并能配置上下文、输出限制、输入能力与推理选项。主界面延续参考截图的深色、灰阶卡片、白色主按钮和固定侧栏。

推荐 **Tauri 2 + React + TypeScript + Rust**。第三方模型统一经过本机网关，凭据保存在系统凭据库；网关将稳定的模型身份映射到具体供应商和 Key。优先使用 Codex 的自定义 provider 与 `model_catalog_json`，先做隔离验证；只有受支持的 Desktop 版本确实需要时才引入进程级 Bridge。不能把 CLI 配置成功当作 Desktop 菜单已经生效。

2026-09-18 根据星算助手开发者的迁移反馈复审：当前配置工具范围保留 Tauri，Rust 核心与桌面壳解耦；若确定需要高频内嵌控制台、多标签或独立网页会话，在创建工程前优先改 **Electron + Rust core-host**。详见 [桌面壳选型复审](research/03-desktop-shell-decision.md)，不能将“内嵌浏览器成本”直接推广成所有桌面工具的结论。

“流畅切换”分成两种：相同模型和能力下更换 Key、调整请求参数，目标是后续请求生效且不用重启；增删菜单模型或修改菜单能力，当前证据指向启动时加载，集中到一次明确的应用操作。不得承诺所有修改都能热更新。官方订阅原生模式与第三方模式保留清楚的边界；二者同菜单无缝共存属于独立兼容目标。

## 阅读顺序

| 文档 | 内容 |
| --- | --- |
| [01 产品需求](01-product-requirements.md) | 用户目标、范围、优先级、成功标准 |
| [参考项目调研](research/01-reference-projects.md) | 三个仓库、星算助手、截图、代码链路与取舍 |
| [Codex 可行性](research/02-codex-feasibility.md) | 菜单接入、字段边界、证据等级与验证门槛 |
| [桌面壳选型复审](research/03-desktop-shell-decision.md) | 星算助手迁移反馈、Tauri/Electron 对比、内嵌浏览器边界与切换条件 |
| [DSH Desktop 补充调研](research/04-dsh-reference.md) | Electron 实际用法、本地界面与网站浏览器的区别、窗口/平台/恢复设计 |
| [星算助手三板块拆解](research/05-xingsuan-tools-plugins-content.md) | 1.6.6 的工具管理 / 插件中心 / 内容中心怎么实现：清单 schema、云端端点、本地落盘与边界 |
| [AITracker 调研](research/06-aitracker-research.md) | 追踪什么（36 个 AI 工具的本地日志路径与 reader 清单）、**能否全网找免费 token/额度的逐项否证 + 合规替代方案（第 9 节）**、离线价格包、出网全清单、许可证限制与可借鉴点 |
| [总体架构](architecture/01-system-architecture.md) | 技术选型、模块、进程和架构决策 |
| [配置与应用事务](architecture/02-configuration-lifecycle.md) | 配置优先级、差异预览、原子写入、回滚、冲突 |
| [网关与协议](architecture/03-gateway-and-protocols.md) | 路由、流式响应、工具调用、输出限制、重试 |
| [数据与接口](architecture/04-data-and-contracts.md) | 实体、关系、IPC、事件和错误契约 |
| [安全与跨平台](architecture/05-security-and-platforms.md) | Keychain、Windows 凭据、进程、安装与更新 |
| [应用内更新](architecture/06-updates.md) | 侧栏更新按钮、更新源与清单、两套签名、失败面、发布步骤与验收；**已实现** |
| [更新弹窗版面审计](audits/2026-09-21-update-dialog-design-conformance.md) | 两套主题 + 最小窗口实测：第一版五处版面问题与修法、复测数字、未验证项 |
| [视觉系统](design/01-foundations.md) | 色彩、布局、间距、尺寸、动效与主题 |
| [图标、字体与文案](design/02-icons-type-and-copy.md) | Lucide 映射、字阶、品牌图标、状态文案 |
| [组件规范](design/03-components.md) | 基础组件与业务组件的状态和交互 |
| [页面与流程](design/04-pages-and-flows.md) | 页面结构、线框、操作、校验、空态与异常 |
| [模板与交互规则](design/05-patterns-and-accessibility.md) | 页面模板、系统交互、键盘与无障碍 |
| [工具管理、插件中心与内容中心](design/06-tool-hub-plugin-hub-and-content-center.md) | 三个新板块的范围、页面、数据模型、抓取策略、安全约束与分批验收；**批次 A + C 已实现** |
| [用量页](design/07-usage-page.md) | 扩展组新板块：只读 Codex 本地会话记录统计 Token、本机计划额度窗口；含解析算法（累计值差分）、契约、版面规范与验收；**已实现** |
| [扩展板块设计规范审计](audits/2026-09-21-extension-pages-design-conformance.md) | 三轮实测：几何/对比度/点击目标/键盘、配色「更清爽」的取值依据、三处版面重做 |
| [用量页审计与真实数据核对](audits/2026-09-25-usage-page-design-conformance.md) | 两套主题 × 两种窗口实测（几何/对比度/点击目标/语义全绿）、交互走查、解析算法与独立基准在 337 个真实文件上逐项一致 |
| [开发计划](development/01-implementation-plan.md) | 分期、任务、依赖、产物与工期估算 |
| [测试与发布](development/02-testing-and-release.md) | 兼容实验、协议测试、双平台验收与发布门禁 |
| [设计规范符合性审计方法](development/04-design-conformance.md) | 用实测数字核对界面：几何四项、对比度、点击目标、键盘与语义、最小窗口与缩放；含已登记的例外与「不许报一个没量过的数字」这条硬规则 |
| [证据索引](appendix/01-source-index.md) | 仓库 SHA、源码定位、本地证据与调研边界 |
| [需求追踪与待验证项](appendix/02-traceability-and-risks.md) | 每条需求的实现、页面、测试和未决问题 |

## 当前已经完成什么

- 克隆并固定三个参考仓库的调研快照；阅读核心文档、相关实现和代表性测试，未运行第三方安装脚本。
- 追加固定 DSH Desktop 快照，阅读桌面壳、视图、导航、preload、平台与恢复代码；修订桌面壳边界，未运行该项目。
- 查看 `referimg/` 中全部 9 张截图；只提取界面结构，不将截图中的会话内容和供应商凭据写入文档。
- 只读查看已安装星算助手的模型中心、添加模型表单和 Codex 工具配置；检查安装包中的程序结构与相关前端调用。
- 2026-09-21 追加只读拆解本机星算助手 **1.6.6**：工具清单（52 份 `paths.json`/`config.json`）、插件安装的 IPC 与落盘形态（`SKILL.md` + 归属标记）、内容中心的云端端点与 localStorage 快照策略。未登录账号、未执行任何安装命令、未改动被检查的文件。见 [三板块拆解](research/05-xingsuan-tools-plugins-content.md)。
- 2026-09-21 实现扩展板块的**批次 A（工具管理只读 + 插件中心）与批次 C（内容中心）**：随包工具清单与探测、技能的公开源安装与按清单卸载、RSS 与 GitHub 搜索的本地快照与退避抓取。批次 B（替用户安装第三方工具）按设计文档决定 1 未做。
- 2026-09-21 追加界面两轮：侧栏分组与页签统一（分段控件）、待应用入口从「跨页吸底」改为「网关页页头下方一条」、工具管理页重做、配色一轮「更清爽」（浅色蓝/绿与深色灰阶，全部按 AA 实测算出）。
- 核对官方配置文档、上游 schema，以及本机 Codex 二进制生成的 app-server schema。
- 输出需求、技术、设计、实施、验收和证据文档。未创建应用工程，未切换模型、调用供应商 API、读取完整密钥或修改现有 Codex 配置。


## 与文档的已知偏差

以下能力在本套文档里被写成设计或需求，但**当前实现里还没有**（或与描述不一致）。放在这里是为了避免照文档写代码时踩空；修掉一项就删一行。

| 文档位置 | 文档所述 | 现状 |
| --- | --- | --- |
| `design/04-pages-and-flows.md` P03 | 供应商预设、状态筛选、详情页签（连接 / Key / 模型）、自定义 headers / 代理与超时 | 未实现：**预设已连同那个恒返回空数组的桩一起删除**（预设是产品内容决策，不做假实现，见 [批次 E 偏差](audits/2026-09-20-batch-E-deviations.md)）；详情是单卡片堆叠，`Provider` 没有 header / 代理 / 超时字段。**多 Key、模型级协议、供应商搜索已实现**（同批次 E） |
| `design/04-pages-and-flows.md` P04 | 目录排序含「最近测试」；筛选按「可用 / 未测试」 | 实现是「按上下文限制排序」与「是否纳入目录」 |
| `design/04-pages-and-flows.md` P05 | 输入能力的三条路径（native / converted / tool_read）与「查看原因」 | 只有 supported / unsupported / unknown 三档，`effectivePath` 恒为 blocked |
| `design/04-pages-and-flows.md` P05 | 错误就地显示并汇总顶部 N 项 | 只有一个通用错误串 |
| `design/04-pages-and-flows.md` P06 | 「Codex 已观测版本」、主区显示默认模型与最近备份 | 观测版本仍恒显示「尚未观测」（本工具不读 Codex 的运行时状态），主区不显示默认模型与最近备份（备份在设置页）。**「重启 Codex」已实现**：应用后自动重启，结论来自进程观察（未退出 / 被强制结束 / 退出未起来分三种说法） |
| `design/04-pages-and-flows.md` P07 | 探测阶段含 SSE、工具调用、工具续接、图片 / 推理 / 长度检查；可选协议与修订 | 只有 connect / credential / model / generate 四阶段，协议是只读展示 |
| `design/04-pages-and-flows.md` P08 | 导出打开本地保存对话框；日志按供应商筛选 | 直接写入应用数据目录并返回路径；筛选维度只有级别 / 类别 / 时间 / 全文 |
| `design/04-pages-and-flows.md` P09 | 「后台运行」分组与登录启动 | 未实现 |
| `design/04-pages-and-flows.md` P09 | 本机网关分组里列监听地址与令牌指纹 | 有意不展示：这两个是内部实现细节（同类工具也不展示），设置页只保留状态、已发布目录、请求数与暂停开关 |
| `design/03-components.md` | Tabs / Tooltip / Drawer / DiffViewer / ProgressSteps | 未实现：现有 AppLogo / Dialog / EmptyState / RowMenu / Switch / CheckCell / FieldHelp / **Toast（右下角宿主，弹窗与页面共用）**，Badge 是全局类 |
| `architecture/01-system-architecture.md` | Axum + Reqwest + rustls；Radix 覆盖 Dialog/Popover/Select；由 Rust 类型生成 TS 类型 | 实际是手写 TCP/HTTP1.1 + ureq；只有 Dialog 用 Radix，Select 是原生；契约类型手工同步 |
| `architecture/01-system-architecture.md` | 端口被占用时给出改端口计划 | 固定 18765，占用即网关不启动 |
| `architecture/02-configuration-lifecycle.md` | 外部配置改动监听 + debounce | 未实现（只能手动重新生成差异） |
| `architecture/03-gateway-and-protocols.md` | 换 Key 后续接仍绑定原凭据（`CONTINUATION_BOUND`）；探测结果 10 分钟过期 | 未实现 |
| `architecture/03-gateway-and-protocols.md` | `POST /v1/responses/compact` 返回结构化 unsupported | 返回 404 |
| `architecture/05-security-and-platforms.md` | Windows 与 macOS 相同核心功能 | Windows 凭据 helper 是显式未完成的桩：界面可用，但第三方模型路由不通 |
| `development/02-testing-and-release.md` | 故障注入（journal 各阶段杀进程、磁盘满、只读目录、凭据库失败）；发布门禁含 checksums / SBOM / 许可证 | 均未实现；CI 现在会跑类型 / 测试 / 构建 / 隐私扫描，但没有故障注入与 SBOM |

另外两处**实现比文档更保守**，是刻意的：网关只绑 `127.0.0.1`（不提供对外监听入口，因为没有 UI 能安全地改端口），提交成功最多到「等待宿主重载」，不自行宣称已加载。

## 进入开发前最先做的事

先执行 [G0 兼容性验证](development/01-implementation-plan.md)，在隔离目录、测试账号和 macOS / Windows 真机中证明：模型菜单出现、选中后确实路由到对应模型、能力设置真实生效、失败可以恢复。G0 没通过，不能用一个漂亮配置界面代替核心能力。
