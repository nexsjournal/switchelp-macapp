# 星算助手 1.6.6：工具管理 / 插件中心 / 内容中心只读拆解

日期：2026-09-21 · 范围：本机安装包的静态拆解与运行态数据目录只读检查。**未登录账号、未提交任何表单、未执行任何安装命令、未修改任何被检查的文件。**

关联：[参考项目调研](01-reference-projects.md) · [证据索引](../appendix/01-source-index.md) · 配套设计见 [工具管理、插件中心与内容中心](../design/06-tool-hub-plugin-hub-and-content-center.md)

> 本文件解决一个问题：**用户想要的三个板块，在参考产品里到底是怎么实现的。** 结论先行——三个板块的实现难度和可信度差异极大：插件是「把文件写进目标工具的 skills 目录」，工具是「声明式清单 + 本机执行安装命令」，内容中心是「云端聚合 + 客户端快照缓存」，**没有任何一项依赖内嵌浏览器或本地爬虫**。

## 0. 快照与证据边界

| 项目 | 记录 |
| --- | --- |
| 应用 | `/Applications/星算助手.app`，`CFBundleShortVersionString` = 1.6.6，Bundle ID `xyz.xsai5.desktop` |
| 外壳 | Electron（`Contents/Frameworks/Electron Framework.framework`），主进程 `Resources/app.asar` → `dist-electron/main.cjs`（289 KB） |
| 预加载 | `dist-electron/preload.cjs`，仅暴露 `window.__xingsuanShell`，通道 `xingsuan:invoke` / `xingsuan:shell` / `xingsuan:event` |
| 核心 | `Contents/Resources/core-host/xingsuan-core-host`，Mach-O arm64，Rust（字符串含 `crates/xingsuan-core/src/...`），与前端 IPC 对接 |
| 前端 | `dist/assets/*.js`，100 个 chunk；`index-C5nDNxaM.js`（3.0 MB）是主包 |
| 工具清单 | `Contents/Resources/_up_/tools/<id>/{config.json,paths.json}`，共 52 个条目 |
| 运行态目录 | `~/.xingsuan/`（未改动，仅 `ls` / `cat` / `sqlite3 .schema`） |
| 云端 | `https://api.xsai5.xyz`（`la.` / `api-hk.` 为区域入口） |
| **未验证** | 未登录 → 插件市场列表、安装、更新检查均**未亲测**，只有代码路径与本地已落盘产物可证；未执行任何工具安装；未验证插件装到非 Claude/Codex 工具后的真实行为 |

## 1. 三个板块在导航里的位置

来自 `dist/assets/zh-Hans-BEuJEMAX.js` 的运行时文案（非猜测）：

| i18n key | 文案 |
| --- | --- |
| `nav.appManager` / `page.appManager` | 工具管理 / 应用管理 |
| `nav.pluginHub` / `page.pluginHub` | 插件中心 |
| `nav.content` / `content.title` | 内容中心 |

内容中心四个标签：`content.tab.tutorial` 教程文档、`content.tab.case` 实战文章、`content.tab.github` GitHub 热门，加上首页/页内的 **AI 资讯**（`AiPulseProvider`）。

## 2. 插件中心：把 SKILL.md 写进目标工具

### 2.1 调用链

```text
React 页面
  └─ window.__xingsuanShell.invoke(cmd, payload)     preload.cjs
      └─ ipcMain.handle("xingsuan:invoke")           main.cjs（校验 sender / 主 frame / origin 后转发）
          └─ core-host（Rust）                       真正落盘
```

渲染层封装成 `se(cmd, payload)`（`index-C5nDNxaM.js` @~518000）。与本板块相关的命令：

| 命令 | 作用 |
| --- | --- |
| `list_installed_plugins` | 列出已装插件，返回项含 `plugin_id` / `version` / `target_tools` |
| `install_plugin_for_tools` | `{ plugin: { plugin_id, kind, install_payload }, targetTools: string[] }` |
| `uninstall_plugin` | `{ pluginId, targetTools }` |
| `scan_local_skills` / `read_skill_document` | 扫描本机已有技能、读单个技能文档 |
| `set_skill_enabled` | `{ tool, skillId, enabled }` |
| `check_mcp_prerequisites` | MCP 前置条件检查 |
| `plugins_pending_for_tool` | 某工具还有哪些插件待装 |

`kind` 取值受主进程白名单约束（`main.cjs`）：`/^(skill|mcp|github)$/`，`plugin_id` 形如 `/^[A-Za-z0-9._-]{1,64}$/`。

### 2.2 "装到当前在用的智能体工作台"是怎么实现的

`install_payload` 就是一个**文件清单**：core-host 侧的 `GeneratedSkillInstallPayload` 字段可见 `files` / `path` / `content` / `target_tools` / `directive` / `notes_md`。安装动作 = 把 `files[]` 写进每个 `targetTool` 对应技能目录，并留下归属标记。

本地能直接看到的成品（本次未安装任何东西，这些是应用此前自己落的盘）：

```text
~/.xingsuan/skills/xs-connectors/xs-github/{SKILL.md, .xs-managed}
~/.xingsuan/skills/xs-connectors/xs-ffmpeg/{SKILL.md, .xs-managed}
```

- `.xs-managed` 内容为一行 `managed by xingsuan` —— **这是"卸载时只删自己写的文件"的依据**。
- `SKILL.md` 是标准 YAML front-matter + 正文（`name` / `description` / `metadata.author`），正文里直接给 agent 可执行的命令示例，并写明「读取类可直接执行，写入类必须先确认」「不得读取本机凭据文件」。

core-host 字符串里出现的技能归属来源标记：`builtin / cursor / claude / agents / codex / hermes / openclaw / skillhub / registry / bundle`，位置分 `global`（如 `~/.codex/skills`）与 `project`（项目内 `.codex/skills` 等）。工具侧对应字段是 `PathsConfig.skillsPath` / `defaultSkillsPath`。

### 2.3 目录（市场）从哪来

前端两块，各挂各的域名：

| 来源 | 端点 |
| --- | --- |
| 自有插件目录 | `GET /api/desktop/v1/plugins`、`/{id}`、`/{id}/doc`、`POST /plugins/check-updates` |
| SkillHub 检索 | `GET /api/registry/skillhub/search?q=` |
| ClawHub（registry）检索 | `GET /api/registry/search?q=`、`POST /api/registry/install`、`/install-preview` |
| 技能包（整仓多技能） | `GET /api/bundles`、`POST /api/bundles/install`、`/install-preview` |
| 通用目录检索 | `GET /api/catalog/v1/search` |
| MCP 市场 | `/api/mcp/marketplace/...` |

**全部来自云端。**客户端不抓 GitHub、不解析仓库，只拉 JSON。`install_preview` 与 `install-preview` 的存在说明"安装前先出差异"是它的既定设计。

### 2.4 交互（应用内手册原文，非推测）

`index-C5nDNxaM.js` 内置了一份手册（`id: plugins-bundles` 等），逐字描述流程：

1. 左侧点「插件中心」，确认顶部在 **插件市场**；
2. 上面点 **技能** 分区；
3. 用右上角搜索或左边分类找到卡片；
4. **点一张卡片，右边出详情**；展开「安装时全部添加」能看到整仓包含哪几个技能；
5. 在 **放进工具** 里勾上想装到哪几个工具，点 **添加技能**；
6. 装完卡片上出现 **已装**；
7. 想确认装在哪，去 **管理台** 的 **技能** 一栏看。

已装列表的空态文案写明它会显示「装到了哪些平台、有没有新版本、还缺哪个平台没装」。整仓安装的部分失败会写成「整仓 3/5 个技能已装」。

### 2.5 顺带发现：它撬开了 Codex 的插件入口（我们不抄）

`_up_/tools/codex/codex-plugin-unlock.js`（37 KB）是运行期加载的 CDP 注入脚本，注释写明两件事：

- 老版 Codex 桌面端在 apikey 模式下禁用左侧「插件」入口 → 用 DOM 操作"撬锁"（新版已原生放开，脚本改成自适应、默认惰性）；
- 经 `~/.codex/config.toml` 的 `model_catalog_json` + 自建 `xingsuan-model-catalog.json` 注入第三方模型，再用 CDP 包住 Statsig 的 `getDynamicConfig`，把模型 slug 补进动态配置 `107580212` 的 `available_models` 白名单，否则模型在 picker 里被渲染成通用的「自定义」。

**这两条都不进本项目**：注入宿主前端属于改别人的界面，与 Switchelp「不改签名包、可回滚、凭据归用户」的立场冲突；模型进菜单我们走 `crates/bridge` 的协议合并路线（已在库内）。此处记录只为说明参考产品的"真实可用"有相当一部分建立在注入上。

## 3. 工具管理：声明式清单 + 本机执行安装

### 3.1 清单本体就在安装包里

`Contents/Resources/_up_/tools/<id>/`，52 个条目（`codex` / `claudecode` / `claudedesktop` / `gemini` / `qwen` / `aider` / `crush` / `goose` / `opencode` / `hermes` / `openclaw` / `zeroclaw` / `droidclaw` / `nanobot` / `ffmpeg` / `ytdlp` / `exiftool` / `whisper` / `gh` / `notion` / `obsidian` / `office` / `wps` / `dingtalk` / `feishu` / `wecom` / `wx` / `tmeet` / `aliyunpan` / `baidupcs` / `biliup` / `modelscope` / `huggingface` / `vibe-trading` / `tradingagents` / `autohedge` / `pi` / `opendesign` / `openfang` / `cloudflared` / `wrangler` / `coffeecli` / `agentmail` / `outlookcli` / `grokbuild` / `tccli` / `deepseek*` …）。每个条目两个 JSON。

`paths.json`（以 `githubcli` 为例）字段语义：

| 字段 | 含义 |
| --- | --- |
| `name` / `names` | 展示名与 i18n |
| `category` | `CLI Code` / `Agents` / `AutoTrading` / `Desktop` / `Utility` / `Custom` |
| `website` / `docs` / `installUrl` | 外链 |
| `command` / `startCommand` / `envVar` | 可执行名、启动方式、注入用的环境变量（Codex 是 `CODEX_PATH`） |
| `configDir` / `configFile` / `requireConfigFile` / `detectByConfigDir` | 配置位置与"是否必须存在配置文件才算装上" |
| `paths.{win32,darwin,linux}` | 各平台候选绝对路径，用来定位已安装实例 |
| `apiProtocol` / `wireApi.{inbound,evidence}` | 支持的接口协议，且 `evidence` 直接写结论依据（例：Codex 恒写 `wire_api = "responses"`） |
| `noModelConfig` | 无模型概念的工具不显示模型选择器 |
| `readiness.{static.any_file, probe.{cmd,ok_exit,cache_s}}` | 就绪判定：静态文件 + 命令探针（带退出码与缓存秒数） |
| `connector.{id,label,auto_install,auth.{mode,login,status,logout,url_hosts,connected.{any_of,none_of}},skill.{name,description,usage}}` | 连接器：交互式登录/状态/登出的**具体 argv**，以及**自动生成的技能** |
| `agent_usage.{non_interactive,tags,delegatable}` | 给 agent 的非交互用法清单 |
| `auth.mode` | `oauth` / `interactive` 等 |
| `description` | 界面说明 |

`config.json` 只管读写格式：`{ docs, configFile, format: "toml"|"json"|"yaml", custom: true }`。

**关键观察**：`connector.skill` 就是 §2.2 里落盘的 `xs-github/SKILL.md` 的来源——`skill.name` = `xs-github`、`description` 与正文的「常用：…」与 `skill.usage` 逐字对应。也就是说，**"给工具装上自己的一键登录连接器" 在这套设计里就是"往技能目录写一个 SKILL.md"**，不需要可执行插件宿主。

### 3.2 状态、策略与安装

| 端点 | 作用 |
| --- | --- |
| `GET /api/tools/registry` | 已注册工具清单（本机引擎侧） |
| `GET /api/tools/status` | 各工具状态 |
| `GET/PUT /api/tools/policy` | 每个内置工具的 `tools_enabled` 开关（按 avatar 存） |
| `POST /api/tools/install` | **SSE 流式**安装在**本机引擎**上跑 |
| `POST /api/desktop/v1/tool-install/report-failure` | 安装失败上报 |
| `POST /api/desktop/v1/tool-diagnostics/report` | 诊断上报 |

`POST /api/tools/install` 的请求体是 `{ tool_id }`，逗号风格应答分离出的事件：`{ phase, percent, message, installed, version, install_command, log_tail }`，`phase` 终止态集合为 `done` / `error` / **`manual_required`**。前端有 `xingsuan-detected-tools-v1`（localStorage）缓存"检测到的工具"，并有专门的失败文案：「本地服务没有响应，检测不到已安装的工具」。

安装命令长什么样，从 core-host 字符串可读到成对的多平台配方（节选，未执行）：

```text
"macos (official)":      npm install -g @wecom/cli && npx skills add WeComTeam/wecom-cli -y -g
"windows_powershell":    npm install -g @tencentcloud/tmeet; npx skills add TencentCloud/tencentmeeting-cli -y -g
"linux (official)":      curl -fsSL .../dingtalk-workspace-cli/.../install.sh | sh
"from source (official repo fallback)": git clone https://github.com/larksuite/cli.git && cd cli && make install && npx skills add larksuite/cli -y -g
```

包管理器白名单也写死在二进制里：`npm / pnpm / yarn / brew / pip / cargo / apt / dnf / yum / pacman / zypper / apk / docker / git / dkpg` 等，另有权限拒绝模式的识别（`operation not permitted` / `errno 1` / `-1743` / `-1744`，即 macOS 自动化权限）。`manual_required` 相位对应的是它没法替你做的那些步（需要 TTY、需要 sudo、需要扫码登录）。

`README` 意义上的产品描述与之一致：官网把桌面端写作「Claude · Codex · Cursor · Cline · Aider 一键安装纠错，账号余额一处管理」。

## 4. 内容中心与 AI 资讯：云端聚合 + 客户端快照

### 4.1 数据来源

```text
GET /api/desktop/v1/news-pulse?lang=&limit=      AI 资讯（无需登录）
GET /api/desktop/v1/news-daily?date=            往期归档
GET /api/desktop/v1/news-daily/dates            可翻的日期
GET /api/desktop/v1/courses                     公开课
GET /api/desktop/v1/home                        首页聚合
GET /api/content/articles                       实战文章
GET /api/content/article?id=|slug=&item_type=    文章正文
POST /api/content/article-reactions             点赞 / 有用
GET /api/content/github-trending                 GitHub 热门
```

全部落在 `api.xsai5.xyz`。**客户端不做全网抓取**，只有云端聚合好的 JSON。

### 4.2 客户端只做「快照 + 30 分钟 TTL」

`index-C5nDNxaM.js` 的 `AiPulseProvider` 可逐字读出：

| 项 | 值 |
| --- | --- |
| 缓存键 | `pulse:items:${zh|en}`、`pulse:meta:${zh|en}`，localStorage |
| 条数上限 | 3000 |
| 新鲜度 | `Date.now() - meta.lastFetched < 30 * 60 * 1000` 且本地非空 → 直接返回缓存，不发请求 |
| 合并 | 新旧按 `url` 去重，再按 `published_at`（缺失时退 `first_seen_at`/`last_seen_at`，且未来时间超过 5 分钟视为不可信）降序 |
| 手动刷新 | 传 `force=true` 绕过 TTL |
| 失败处理 | 保留旧快照，把错误显示出来（`retry` 可重试） |
| 应答字段 | `fetched_at`、`items[]`（含 `title` / `url` / `source` / `published_at` / `first_seen_at`） |

首页那条滚动资讯条与内容中心是同一个 provider。

### 4.3 另有本地 RSS 与本地定时器（但不用在资讯上）

- **本地 RSS**：core-host 有 `crates/xingsuan-core/src/cli/chat_rss.rs`，命令 `cli_chat_fetch_rss`，属于「码上空间」（CLI 工作台）的阅读功能。渲染层可见一批写死的订阅源：`sspai.com/feed`、`ruanyifeng.com/blog/atom.xml`、`baoyu.io/feed.xml`、`linux.do/latest.rss`，以及一批课程/教程入口。**AI 资讯不走这条路。**
- **本地定时任务**：core-host 有完整的 `AutomationTask`（17 字段：`prompt` / `workspace` / `permissionSnapshot` / `frequency` / `effectiveDateRange` / `lastRunStatus` / `lastRunError` / `fromTemplate`）与 `AutomationRun`（`triggerscheduledFor` / `claimedAt` / `missedCount` / `executorPid` / `finalText`），频率枚举 `Once | Daily | Interval{hours}`，命令是 `automation_save_task` / `automation_load_tasks` / `automation_run_now` / `automation_sync_system_schedule` / `automation_read_log`。这是**跑 agent 任务**的调度器（可同步到系统计划），不是资讯抓取器。

### 4.4 落盘形态

`~/.xingsuan/` 下与内容相关的只有：`cli-workbench.db`（SQLite，表 `projects` / `conversations` / `messages` / `native_history_cache`）、`studio/sse-events.sqlite3`、`storage/kb`。**没有任何资讯历史表**——资讯只有 localStorage 里那一份最新快照。

## 5. 对 Switchelp 的直接含义

| 参考产品做法 | 可移植性 | 原因 |
| --- | --- | --- |
| 插件 = 往目标技能目录写文件 + 归属标记 | **可直接移植** | 纯文件系统操作，无需宿主插件运行时；Codex/Claude 都读 `SKILL.md` |
| 多平台工具清单 + 就绪探针 + 连接器 | **可直接移植** | 声明式 JSON + 固定 argv 探针；`_up_/tools` 的结构可以照着重画一份自己的 |
| 安装 = 本机执行安装命令 + SSE 进度 | 可移植但要降级 | 我们能跑固定 argv，但"一键装 Cline/Cursor/Aider"意味着替用户装第三方软件，风险与产品定位都要单独拍板 |
| 插件目录来自自家云端 | **不可移植** | 我们没有后端，也没有运营团队；只能改用公开源（GitHub 仓库）作目录 |
| 资讯来自云端聚合 + 客户端 30 分钟快照 | **可等价替换** | 改为核心直接抓 RSS / GitHub 搜索 API，本地表里存最新快照，语义一致 |
| 「每日精选」「点赞/有用/反馈」 | **不做** | 那是人工运营内容与工单系统，本地工具造不出来，也不该造假 |
| CDP 注入宿主前端解锁插件/模型 | **不做** | 改的是别人的签名界面，与本项目「不改签名包、一切可回滚」的硬约束冲突 |

**未知项（保持未知，不要写成结论）**：① 未登录状态下插件市场列表内容、安装/更新/卸载的真实行为全部未亲测；② 插件装到 `cursor` / `hermes` / `openclaw` 等非 Claude/Codex 目标后的落盘位置与生效方式未验证；③ 整仓（bundle）安装的原子性只在文案层面见过「3/5 个技能已装」，无实现级证据。
