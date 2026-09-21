# 工具管理、插件中心与内容中心

状态：**已确认，批次 A + C 已实现（2026-09-21）** · 上游证据：[星算助手 1.6.6 只读拆解](../research/05-xingsuan-tools-plugins-content.md) · 实测记录：[扩展板块设计规范审计](../audits/2026-09-21-extension-pages-design-conformance.md)

> 这份文档把参考产品的三个板块拆成三件**可以分别决定做不做**的事。三件事的风险差两个数量级，所以本文不把它们打成一个包。
>
> **实现进度（2026-09-21）**：第 10 节的五项决定已确认——工具安装（批次 B）本版不做；技能首批只开 Codex 与 Claude Code；
> MCP 不做；内容源用第 7.1 节那批；侧栏接受新增三个条目。**批次 A（工具检测 + 插件中心）与批次 C（内容中心）已落地**，
> 逐条验收见 §9 与审计报告。第 3–8 节保留为设计依据，与实现的差异见 §12。

## 1. 先给结论

| 板块 | 参考产品怎么做的 | 我们能不能做 | 靠什么做 |
| --- | --- | --- | --- |
| **工具管理** | 52 份声明式 JSON 清单 + 本机执行安装命令（SSE 进度） | **能做**，但要拆成两半：检测（零风险）与安装（替用户装第三方软件，需单独拍板） | 自建清单 + 复用既有 `diagnostics/discovery.rs` 的探测能力；安装用固定 argv + 确认弹窗 |
| **插件中心** | 云端目录 → 点卡片 → 勾"放进工具" → 把 `SKILL.md` 写进该工具的 skills 目录，用 `.xs-managed` 标记归属 | **能做**，且这是三件里性价比最高的 | 目录改从公开 GitHub 仓库取；安装 = 事务化写文件 + 归属清单，卸载按清单精确回滚 |
| **内容中心 / AI 资讯** | 云端聚合好的 JSON + 客户端 localStorage 快照，TTL 30 分钟 | **能做，但形态必须改** | 没有后端，所以改为核心直接抓 RSS 与 GitHub 搜索 API，本地 SQLite 存最新快照 |

**关于"要不要数据库"的直接回答（你在需求里问的）**：要，但只需要一张很小的表，而且**不是因为要存"每天的全部资讯"，是因为要做三件别的事**：

1. **去重**——同一个链接在多个源出现、或同一条被源二次推送，靠 URL 主键天然解决；
2. **离线与降级**——抓取失败时界面显示上次成功的快照并标明时间，而不是空白或假数据（这是本项目"状态必须来自观察"的一贯口径）；
3. **礼貌抓取**——存 `ETag` / `Last-Modified` 做条件请求，源没更新时服务器直接返回 304，不重复下载正文。

只保留最近 N 天、按写入时裁剪即可，**不需要**按天归档、不需要全文检索、不需要给资讯建"历史库"。你的判断是对的：只展示最新就能把问题压到很小——参考产品正是这么做的，它的资讯在本地连一张表都没有，只有一份 localStorage 快照。我们比它多加一张表，是为了离线可用和条件请求，不是为了存历史。

## 2. 范围

### 2.1 做

- **P-T1 工具检测与状态**：识别本机已装的 agent 工作台与常用 CLI，显示路径、版本、配置位置、登录/授权状态、已装技能数。只读。
- **P-T2 插件中心**：技能（`SKILL.md`）的浏览、安装、更新检查、启停、卸载，安装时选择"放进哪些工具"。
- **P-C1 内容中心：AI 资讯 + GitHub 热门**：RSS 订阅源与 GitHub 搜索 API，定时刷新，本地快照。
- **P-T3 工具安装（第二批，独立拍板）**：显示确切命令、逐条确认、流式输出、失败时给手动步骤。

### 2.2 不做，以及为什么

| 不做 | 原因 |
| --- | --- |
| CDP / DOM 注入宿主前端（参考产品用它解锁 Codex 插件入口与模型名白名单） | 改的是别人的签名界面；与本项目「不改签名包、一切可回滚」的硬约束冲突。模型进菜单我们走 `crates/bridge` 的协议合并路线 |
| 「每日精选」「点赞 / 有用 / 反馈」「公开课」 | 那是人工运营内容与工单系统。本地工具造不出来，造个空的更糟 |
| MCP 服务器安装（写工具的 `mcp_servers` 配置） | 属另一类风险（把外部进程接进 agent 的工具链），且目标工具的配置 schema 各不相同。第二批单独评估 |
| 服务端目录 / 账号体系 | 本项目 MIT、无后端、无遥测。目录只能来自公开源 |
| 后台常驻抓取（LaunchAgent / 计划任务） | 那是系统级状态变更。v1 只在应用运行时刷新，界面直说 |
| 给非 Codex 工具应用模型配置 | 参考产品做得到，但那要吃透每个工具的配置格式。我们先把 Codex 做对 |

## 3. 信息架构

侧栏在「Codex 配置」之后插入一组「扩展」，内容中心独立成组：

```text
Switchelp
├── 概览
├── 供应商
├── 模型
├── Codex 配置
├── ── 扩展 ──
│   ├── 工具管理        ← 新增
│   └── 插件中心        ← 新增
├── ── 内容 ──
│   └── 内容中心        ← 新增
├── 连接诊断
├── 日志
└── 设置
```

分组标题只做视觉分隔，不可点击、不折叠（避免为一组分隔引入新状态）。若你希望更保守，退路是把「工具管理」并进「连接诊断」做成页签——**不推荐**：连接诊断回答"我的网关通不通"，工具管理回答"我机器上有什么、装在哪"，两者的失败原因和排查动作完全不同。

## 4. 页面设计

### 4.1 P-T1 工具管理

一页一表，主列是工具，行内展开详情。表头两行以内说清状态口径。

**列表列**：名称（含分类标签）｜状态徽章｜版本｜可执行路径｜配置位置｜已装技能（数字，可点进插件中心）｜操作

**状态徽章**（沿用既有语义色映射，全部集中在 `src/features/.../policy.ts` 一处，不散写）：

| 徽章 | 语义色 | 判定依据 |
| --- | --- | --- |
| 已就绪 | 成功（绿） | 静态文件存在**且**探针命令退出码等于 `ok_exit` |
| 已安装·未授权 | 琥珀 | 二进制在、探针失败且输出命中"未登录"这类特征 |
| 已安装 | 中性 | 二进制在，该工具无授权概念（如 `ffmpeg`） |
| 未安装 | 中性 | 各平台候选路径都不存在 |
| 不支持本平台 | 中性 | 清单里没有当前平台的路径条目 |

**行内详情**（展开）：一句话用途｜官网/文档链接（走系统浏览器）｜探针原文的最后几行（可复制）｜"给 agent 的用法"（来自清单的 `agent_usage.non_interactive`）｜技能连接器的安装状态。

展开后的版面（2026-09-21 重做）：事实表装在一个带描边的浅底面板里，后面每一节
（给 agent 的用法 / 版本探针原文 / 登录探针原文）都自带小标题与**上方一条分割线**，
动作行也由一条分割线与正文分开。整块详情有一条 2 px 的左侧竖线把它挂到上面那一行。
做这轮改动的原因是用户的实际体验：「展开后我都看不到它下面是怎么区分分隔的，有的间距都快挨住了」
——一整段长内容只靠留白分组，1 px 的行分隔线在浅色主题下又几乎看不见。
窄窗（< 1080 px）里详情不再左侧缩进，避免路径被挤成两行。

行的 hover 底色由 `::before` 画：一块比内容左右各宽 12 px、上下各内收 2 px 的圆角底。
用户原话：「hover 感觉背景直接挨住内容了」——底色必须包住内容，不能从内容边上开始。
**不用「负外边距 + 内边距」**去凑同样的效果：那会让每一行都比父容器宽 24 px，
页面几何审计把这类外扩一律记成 `overflow`（实测一次 12 处），噪声会盖住真问题；
绝对定位的伪元素不参与该判定。内层那个整行按钮要显式关掉全局 `button:hover` 的底色，否则叠两层。

**空态**：`没检测到任何工具。点"重新检测"扫描本机常见安装位置。` **失败态**：说明是扫描失败还是真的没有；扫描异常必须与"确实没装"分开显示。

**铁律**：这一页的每个徽章都必须来自一次真实探测。探针结果带缓存时间（清单里给 `cache_s`），界面显示"检测于 3 分钟前"，不写"已就绪"这种无时间戳的断言。探针失败绝不显示为绿色。

### 4.2 P-T2 插件中心

顶部两个页签：**插件市场** / **已安装**。

左侧技能列表的每一项都是**有底、有描边的卡片**：未选中 `bg.surface` + `border.subtle`，
悬停 `bg.hover`，选中 `bg.selected` + `accent.border`——与网关页左侧供应商列表同一套
（`App.module.css` 的 `.providerItem`）。从前未选中是「透明底 + 透明描边」，
一列里只有被选中的那一项看着像元素、其余几项像纯文字（用户点名的就是这个）。
选中底色会把弱文字压到 4.5 以下（实测 4.14），选中行里的说明因此提级为次级色——
与供应商列表同一条处理。

**插件市场**（左列表 + 右详情，沿用现有 master-detail 交互）：

- 分区：`技能`（先做）／`MCP`（暂不开放，显示"暂不支持"并说明原因，不放灰按钮）
- 搜索框过滤 名称 / 描述 / 来源仓库
- 卡片：名称｜来源仓库（`owner/repo`）｜一句话描述｜版本或 commit 短 SHA｜已装标记
- 右侧详情：完整描述｜`SKILL.md` 原文预览（只读、可折叠）｜**声明的权限与风险提示**｜`放进工具` 多选（本页从工具管理页读到"已就绪"的工具）｜`安装` 按钮｜若有历史版本，显示"更新"与"重装"

**已安装**（沿用工具管理同款表）：

- 列：名称｜来源｜版本 / commit｜装到了哪些工具｜可更新｜开关（启停）｜卸载
- 每个工具的单元格独立可操作（参考产品的空态文案说得好：显示"装到了哪些平台、有没有新版本、还缺哪个平台没装"，我们照这个口径）
- **卸载确认必须列出将被删除的确切文件**，并保证只删带我们归属标记的文件

**关键交互规则**：

1. **安装前必看**：`SKILL.md` 是写给 agent 的执行指令，不是装饰文本。详情页默认展开"这个技能会让 AI 做什么"（首屏摘要 + 命令片段），安装按钮下方常驻一行：`技能是给 AI 的执行说明，不是你点的按钮。装之前请看清它会要求 AI 执行哪些命令。`
2. **部分完成也如实报**：整仓安装失败时写"3/5 个技能已装"，并把失败的那几个列出来，绝不能只弹一个"安装失败"。
3. **冲突处理沿用既有事务口径**：目标目录已存在同名技能且不是我们装的 → 不覆盖，进入冲突状态，让用户选"保留两者（改目录名）／跳过／查看差异"。
4. 安装完成后不弹二次成功弹窗，走既有 Toast 出口，卡片上的"已装"就是结果。

### 4.3 P-C1 内容中心

顶部页签：**AI 资讯** / **GitHub 热门** / **订阅源**。

**AI 资讯**：

- 时间倒序列表：标题（点击走系统浏览器）｜来源名｜相对时间
- 顶部一条细状态行：`上次更新 12 分钟前 · 下次自动更新 48 分钟后`，旁边一个"立即刷新"
- 抓取失败时：**保留旧内容**，状态行转琥珀色写清原因（网络不可达 / 源返回 404 / 全部源失败），并给出"重试"
- 不做无限滚动：默认展示最新 100 条，底部"加载更早"（从本地表翻，不重新联网）

**GitHub 热门**：时间范围切换 今日 / 本周 / 本月｜关键词搜索（本地过滤已抓到的结果）｜卡片：`owner/repo`｜★ 数｜语言｜一句话描述｜"打开仓库"。数据来自 GitHub 搜索 API，卡片上如实标注来源与抓取时间。

**订阅源**：源清单管理（增删改、启停、单源手动刷新、显示每个源的上次成功时间与连续失败次数）。预置一批经过挑选的源，用户可以全部删掉。

## 5. 数据模型

### 5.1 领域对象（`crates/switch-core/src/domain/`）

```rust
// domain/tool.rs
pub struct ToolDescriptor {          // 来自随包清单，只读
    id: String,
    names: BTreeMap<String, String>, // i18n: zh-Hans / en
    category: ToolCategory,          // CliCode | Agents | Desktop | Utility
    website, docs: Option<String>,
    command: Option<String>,
    config_dir, config_file: Option<PathBuf>,
    require_config_file: bool,
    paths: PlatformPaths,            // win32 | darwin | linux
    model_config: bool,              // false = noModelConfig
    readiness: Readiness,            // static_files + probe{cmd, ok_exit, cache_s}
    connector: Option<ConnectorSpec>,
    agent_usage: AgentUsage,
    install: PlatformRecipes,        // 每平台一组带说明的命令；空 = 只给手动指引
}

pub struct ToolState {               // 运行时探测结果，不落库之外的真相
    id: String,
    installed: Option<InstalledTool>, // path / version / config_path / skills_path / skills_count
    auth: AuthState,                  // Ready | NeedsLogin | Unknown | NotApplicable
    probed_at: DateTime,
    probe_tail: String,               // 探针输出尾部，界面可复制
}
```

```rust
// domain/plugin.rs
pub struct SkillSource {             // 目录项：来自公开仓库
    repo: String,                     // owner/repo
    commit: String,                   // 解析出的提交 SHA
    skills: Vec<SkillManifest>,       // 一个仓库可能含多个技能
}

pub struct SkillManifest {
    id: String,                       // front-matter 的 name
    title, description: String,
    body: String,                     // SKILL.md 原文（详情页预览用）
    dir_name: String,                 // 目标目录名
    declared_bins: Vec<String>,       // front-matter metadata.gate.requires_bins
}

pub struct SkillRecord {              // 本地归属清单，落库
    id: String,
    source_repo: String,
    source_commit: String,
    dir_name: String,
    target_tool: String,              // codex | claude-code | agents
    installed_path: PathBuf,
    files: Vec<FileFingerprint>,      // path + sha256，卸载与冲突检测的依据
    installed_at: DateTime,
    enabled: bool,
}
```

### 5.2 SQLite 表（迁移版本 +1）

```sql
CREATE TABLE tool_probe_cache (
  tool_id TEXT PRIMARY KEY,
  probed_at INTEGER NOT NULL,
  payload_json TEXT NOT NULL          -- ToolState 的序列化；有 cache_s 才复用
);

CREATE TABLE installed_skills (
  id TEXT NOT NULL,
  target_tool TEXT NOT NULL,
  source_repo TEXT NOT NULL,
  source_commit TEXT NOT NULL,
  dir_name TEXT NOT NULL,
  installed_path TEXT NOT NULL,
  files_json TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  installed_at INTEGER NOT NULL,
  PRIMARY KEY (id, target_tool)
);

CREATE TABLE feed_sources (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,                 -- rss | github_search
  url TEXT NOT NULL,
  label TEXT NOT NULL,
  lang TEXT NOT NULL DEFAULT '',
  enabled INTEGER NOT NULL DEFAULT 1,
  etag TEXT, last_modified TEXT,
  last_ok_at INTEGER, last_error TEXT, fail_streak INTEGER NOT NULL DEFAULT 0,
  next_fetch_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE feed_items (
  url TEXT PRIMARY KEY,
  source_id TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT NOT NULL DEFAULT '',
  published_at INTEGER NOT NULL,      -- 缺失时退 first_seen_at
  first_seen_at INTEGER NOT NULL,
  lang TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_feed_items_time ON feed_items(published_at DESC);
```

**裁剪策略**：每次写入后删掉 `published_at` 早于 30 天且不在最新 1000 条内的行；单源保留上限 200 条。表大小封顶在个位数 MB。

**技能文件不落库**（正文本来就在磁盘上），库里只存归属指针与指纹。

### 5.3 落盘位置

```text
<app_data_dir>/tools/catalog.json         随包清单（构建期从仓库 catalog/ 拷入，运行时只读）
<app_data_dir>/plugins/skills/<id>/       我们自己的技能缓存（先下载校验，再写入目标）
<target_skills_root>/<dir_name>/          目标工具真正读取的位置
<target_skills_root>/<dir_name>/.switchelp-managed   JSON：repo / commit / 文件指纹 / 安装时间
```

`.switchelp-managed` 是卸载与冲突判定的唯一依据，与参考产品的 `.xs-managed` 同思路但携带更多信息（它是标记，我们是清单）。

## 6. 实现落点

### 6.1 Rust 核心（`crates/switch-core/src/`）

| 新模块 | 职责 | 复用既有 |
| --- | --- | --- |
| `toolhub/catalog.rs` | 载入并校验随包清单（schema 版本、路径展开、缺字段报错而不是静默） | `domain/error.rs` |
| `toolhub/detect.rs` | 路径候选匹配 + 探针执行 + 版本解析 + 缓存 | **`diagnostics/discovery.rs`**（已有的实例发现逻辑先看能不能直接扩，别重写） |
| `toolhub/probe.rs` | 固定 argv 子进程执行、超时、输出截断、退出码语义 | 参照既有 `diagnostics/probe.rs` |
| `plugins/source.rs` | 从 GitHub 取仓库（`ureq`，带 UA 与超时）、解析 `SKILL.md` front-matter、目录缓存 | 新 |
| `plugins/install.rs` | 事务化写文件、指纹、冲突检测、卸载回滚 | **`storage/journal.rs` + `storage/operation.rs` + `storage/snapshot.rs`** |
| `plugins/registry.rs` | `installed_skills` 表的读写与启停（启停 = 目录重命名 `<dir>` ↔ `<dir>.disabled`，因为各工具的"禁用"约定不一致，改名是唯一通用做法，界面上说明这一点） | `storage/repository.rs` |
| `content/fetch.rs` | RSS / Atom 解析、条件请求、超时、UA、每主机最小间隔 | `ureq` |
| `content/schedule.rs` | 下次抓取时间计算、启动时补偿、并发上限 2、抖动 | `tokio` |
| `content/store.rs` | `feed_items` / `feed_sources` 读写与裁剪 | `storage/sqlite.rs` |

**注意**：`ureq` 是阻塞客户端，所有网络调用必须包在 `tokio::task::spawn_blocking` 里，别在 async 上下文里直接跑（会卡住整个运行时）。这一点在既有的 `models_discover` 里应已有先例，实现前先对齐。

### 6.2 Tauri 命令（`src-tauri/src/commands.rs`，沿用既有下划线命名）

```text
tools_catalog()                         -> ToolDescriptor[]
tools_state(refresh: bool)              -> ToolState[]        // 尊重 cache_s，refresh=true 强刷
tools_probe(tool_id)                    -> ToolState
plugins_catalog(query)                  -> SkillSource[]
plugins_preview(source, skills)         -> InstallPreview     // 将要写入的文件清单
plugins_install(source, skills, targets, idempotencyKey) -> ExecuteResult
plugins_installed()                     -> SkillRecord[]
plugins_check_updates()                 -> UpdateReport
plugins_set_enabled(id, target, enabled) -> SkillRecord
plugins_uninstall(id, targets, idempotencyKey) -> ExecuteResult
content_feeds()                         -> FeedSource[]
content_feed_save(draft, expectedVersion) -> FeedSource
content_feed_delete(id)                 -> void
content_items(filter, cursor)           -> { items, nextCursor }
content_refresh(sourceId: Option<String>) -> RefreshReport
content_status()                        -> { lastOkAt, nextFetchAt, failures }
```

写类命令一律返回 `ExecuteResult { operationId }`，把结果写进既有 operation 日志——这样"为什么没生效"仍然可以用既有的方式（查 operations 的 stage）诊断，不引入第二套记账。

### 6.3 前端

- `src/features/tools/`：`ToolsPage.tsx` + `ToolDetailPanel.tsx` + `toolsPolicy.ts`（徽章映射，单一出处）
- `src/features/plugins/`：`PluginHubPage.tsx`（两页签）+ `MarketList.tsx` / `MarketDetail.tsx` / `InstalledTable.tsx` + `InstallConfirmDialog.tsx`
- `src/features/content/`：`ContentPage.tsx` + `NewsList.tsx` + `GithubTrending.tsx` + `FeedSourcesPage.tsx`
- `src/desktop/client.ts` 扩接口、`transport.ts` 加转发、`src/contracts/types.ts` 加 DTO
- 侧栏与路由：`src/app/`（既有导航定义处）
- 文案：`src/locales/zh-CN.ts` 与 `en.ts` 同步

## 7. 抓取策略（内容中心）

### 7.1 源清单（预置，可删可改）

| 类 | 源 | 方式 |
| --- | --- | --- |
| 中文社区 | 少数派 `sspai.com/feed`、阮一峰 `ruanyifeng.com/blog/atom.xml`、linux.do `linux.do/latest.rss` | RSS/Atom |
| AI 方向 | 宝玉 `baoyu.io/feed.xml`、Hacker News `hnrss.org/newest?points=100` | RSS |
| 开源热门 | GitHub 搜索 API `/search/repositories?q=created:>DATE&sort=stars` | JSON API，**不抓网页** |

参考产品在渲染层写死了同类的几个源（`sspai` / `ruanyifeng` / `baoyu` / `linux.do`），可见这个选择是有共识的；我们做成可编辑列表，比写死更好，也不比它贵。

### 7.2 调度与礼貌

**2026-09-21 改：固定时刻，不再可配。**

| 项 | 现在的规则 |
| --- | --- |
| 抓取时刻 | **每天本地 06:00 与 18:00**（`content::DAILY_FETCH_HOURS`），错峰 0–5 分钟（按源 id 哈希，可复现） |
| 谁决定 | 核心持有时刻表，`ContentStatus.scheduleHours` 报给界面；界面只显示「每天 06:00、18:00 自动更新」，不自己写一份小时数 |
| 触发 | 应用内后台任务每 5 分钟醒一次，抓哪些源由每个源的到期时间决定；「立即刷新」忽略到期时间 |
| 每源保留 | **20 条**（`PER_SOURCE_KEEP_ITEMS`）：资讯页一屏至少放 10 条，留一倍余量 |
| 覆盖方式 | **整源覆盖**：抓取成功时，这个源里「这次没抓到的」条目会被删掉（`HubStore::sync_source_items`），本地只留最近一次的结果 |
| 兜底 | 保留期 3 天、全局 400 条。源长期失败时旧条目不会永久留在本地 |
| 空正文 | 订阅源返回 200 但没有条目时**什么都不做**，保留上一份快照——与「失败时保留旧内容」同一条原则 |

为什么从「可配间隔」改成「固定时刻」：用户要的是「早上 6 点、晚上 6 点各一次」，而且应用一天里会被
开开关关，固定间隔会把节奏漂到任意时刻去（用户原话：「尽量不存数据，更新的时候就把新的覆盖掉旧的」）。
`content_set_interval` 命令、`contentInterval` 设置与界面上的间隔下拉都已删除。

列表样式与卡片内边距跟着一起对齐了：卡片四边 24；新闻行四边 12（hover 底色由 `::before` 画，左右各外扩 12）；
仓库卡片四边 16；插件列表项四边 16。判据是「第一行文字距卡片顶 = 距卡片左」，实测都相等。

| 项 | 取值 | 理由 |
| --- | --- | --- |
| 默认间隔 | 60 分钟 | 与参考产品的 30 分钟 TTL 同量级；资讯不需要更密 |
| 可选范围 | 15 分钟 – 24 小时，或关闭 | 让用户自己控制流量 |
| 触发时机 | 应用启动时（若已过期）+ 定时器 | **仅在应用运行时**。界面在订阅源页写明这一点 |
| 并发 | 最多 2 个源同时抓 | 对源站友好，也避免拖慢界面 |
| 抖动 | ±10% 随机 | 避免所有用户整点同时请求 |
| 条件请求 | 带 `If-None-Match` / `If-Modified-Since`，304 则只更新 `last_ok_at` | 省流量，也让"没更新"是可观察的事实 |
| UA | `Switchelp/<version> (+<repo-url>)` | 源站能识别我们是谁 |
| 超时 | 连接 5s / 整体 15s | 失败要快，不要占着并发位 |
| 单源失败 | 计入 `fail_streak`，退避 5min → 15min → 1h（上限） | 不因一个坏源把整页打成错误 |
| 全部失败 | 界面显示上次成功快照 + 琥珀状态行 | 绝不显示空白或示例数据 |

### 7.3 GitHub 搜索 API 的现实约束

未认证时是 **10 次/分钟**（按 IP）。所以：热门榜缓存 60 分钟、单次只取 25 条、不做"点一次搜一次"的实时搜索（搜索在本地已抓到的结果上做）。设置里提供一个可选的 GitHub Token 输入（存系统凭据库，复用既有 `credentials/` 通路），填了可以提高频率限制——这一条要写清"不填也能用"。

## 8. 安全与隐私

新增网络抓取会**改变本项目的隐私声明**，这部分必须一起改，不能只在代码里加个 fetch 就完事。

### 8.1 硬约束

| 约束 | 实现 |
| --- | --- |
| 不执行远程脚本 | 插件安装只写文件，永不执行仓库里的任何脚本；目录里的 `.sh` / `.js` 只作为文本展示 |
| 安装命令必须可见 | 工具安装页在任何执行前把确切 argv 全文展示，用户逐条确认；不做"后台静默装" |
| 无 shell 插值 | 一律 `Command::new(bin).args([...])`，绝不 `sh -c`；PATH 与 env 用白名单构造 |
| 不 sudo | 需要提权的步骤归入"手动步骤"，给可复制的命令，不代跑 |
| 路径不越界 | 目标 skills 根目录 canonicalize 后校验写路径必须在根内；拒绝符号链接逃逸；拒绝 `..` |
| 原子写 + 备份 | 先写临时文件再 rename；覆盖前把原文件指纹存进 operation 日志，可恢复 |
| 只删自己的 | 卸载只删 `.switchelp-managed` 清单里的文件；有未在清单中的新增文件时停下来问，不递归删目录 |
| 技能内容风险可见 | 详情页展示它会让 AI 执行什么；对含 `curl … | sh`、凭据路径（`~/.ssh`、`.aws/credentials`、`config.toml` 里的 key）等模式的技能给出显著警告 |

### 8.2 需要同步修改的既有文档

| 文档 | 改什么 |
| --- | --- |
| 根 `README.md` | 隐私一节：明确"为抓取资讯会访问这些域名，只发 GET、不带任何本地数据"；否则"不上传遥测"的表述会被新功能打成不准确 |
| `docs/architecture/05-security-and-platforms.md` | 新增"外部网络访问"与"第三方技能写入"两节 |
| `docs/01-product-requirements.md` | 「当前不做」一节里 `插件市场` 需改为「插件市场（云端目录与账号体系）」，并说明本项目做的是公开源技能安装 |
| `docs/architecture/04-data-and-contracts.md` | 新增三张表与命令契约 |
| `docs/README.md`「与文档的已知偏差」 | 本文实现后逐条登记实情 |

## 9. 分批实施与验收

### 批次 A（建议先做，风险最低，可独立交付）

1. `tools_catalog` + `tools_state` 与**工具管理页只读版**（含探针缓存与状态徽章）。
2. 技能目录（GitHub 源解析）+ 技能详情预览。
3. 技能安装 / 已安装列表 / 卸载（含归属清单与冲突处理）。

**验收**：在真机上点一遍——① 工具页每个徽章都能对上一条真实探针输出；② 从市场装一个技能到 Codex，`~/.codex/skills/<name>/SKILL.md` 出现且 Codex 里能用；③ 卸载后该目录消失且**其余文件一个都没动**（用安装前后的目录快照比对）；④ 目标已有同名非我方技能时进入冲突态而不是覆盖。

### 批次 B（独立拍板）

4. 工具安装：命令可见 + 逐条确认 + 流式输出 + `manual_required` 手动指引。

**验收**：拿一个真实工具（如 `gh` 或 `ffmpeg`）走完整流程；中断安装后系统里不留半成品；需要交互的步骤落到手动指引而不是卡死。

### 批次 C（与 A 并行也可以）

5. 内容中心：订阅源管理 + AI 资讯 + GitHub 热门 + 定时与退避。

**验收**：① 断网启动时显示上次快照并标明时间，不空白；② 连续两次刷新第二次命中 304（用抓包或源站日志证明）；③ 连续失败 3 次的源进入退避且界面可见失败原因；④ 应用关闭期间不发起任何请求（在订阅源页声明并验证）。

### 测试

| 层 | 测什么 |
| --- | --- |
| 单元 | RSS / Atom 解析（含畸形 XML、缺 `published`、时区）、front-matter 解析、指纹与冲突判定、退避与 `next_fetch_at` 计算、裁剪边界 |
| 集成（tempfile） | 安装 → 校验指纹 → 卸载的往返；冲突场景；路径逃逸被拒；中断后不留临时文件 |
| 命令 | 探针超时/退出码语义；缓存过期；`refresh=true` 强刷 |
| UI | 徽章映射的单一出处在 `policy.ts`；空态/失败态/部分完成文案 |
| 人工 | **必须在真实界面走一遍并截图**，只跑测试不算完成 |

## 10. 需要你拍板的决定

| # | 决定 | 结论（2026-09-21） |
| --- | --- | --- |
| 1 | **工具安装（批次 B）做不做** | **不做**（本版）。它会替用户往系统里装第三方软件，与"克制、可回滚"的定位张力最大；而工具管理只读版已经解决了"我机器上有什么、装在哪、为什么没生效"这个真实痛点。等 A 跑顺再回来 |
| 2 | **技能装到哪些工具** | 首批只开 `Codex`（`$CODEX_HOME/skills`）与 `Claude Code`（`~/.claude/skills`）两个**我们已经能从清单里确定目录**的目标；其余工具在界面上显示"暂不支持"，不放灰按钮 |
| 3 | **MCP 要不要一起做** | **不做**。写 `mcp_servers` 是往 agent 的工具链里接外部进程，风险等级不同于写一个 markdown 文件，应当单独评审 |
| 4 | **内容源的默认清单** | 采用 §7.1 的五个源起步（另加三个 GitHub 搜索窗口），全部可删。若你希望更保守，可以默认只开两个中文源 |
| 5 | **侧栏是否接受新增三个条目** | **接受**，按 §3 分两组（「扩展」「内容」分隔标签）。若嫌长，退路是「内容中心」放到概览页做入口、不占侧栏 |

## 11. 已知不确定项

- 参考产品的插件市场在未经登录状态下**无法访问**，其目录内容、安装/更新/卸载的真实行为本轮全部未亲测；本文对它的描述止于代码路径与本机已落盘产物。
- 「技能装到 `cursor` / `hermes` / `openclaw` 等工具后的落盘位置与生效方式」未验证。所以我们首批只开两个有把握的目标（决定 2）。
- 各工具"禁用某个技能"的官方约定不一致，我们采用目录改名 `<dir>` ↔ `<dir>.disabled` 的通用做法——**这是权宜之计**，界面必须写明"本工具是通过把技能目录改名来实现禁用的"，不能让用户以为这是该工具的官方开关。
- GitHub 搜索 API 的字段与限流策略可能调整，实现前重新核对一次官方文档。

## 12. 实现记录与偏差（2026-09-21）

落地范围：**批次 A（工具管理只读 + 插件中心）与批次 C（内容中心）**。批次 B（替你安装第三方工具）按决定 1 未做。

### 12.1 代码落点

| 层 | 文件 |
| --- | --- |
| 随包清单 | `crates/switch-core/catalog/tools.json`（18 个工具，构建期 `include_str!` 进二进制，运行时只读） |
| 工具管理 | `crates/switch-core/src/toolhub/{mod,catalog,probe,detect}.rs` |
| 插件中心 | `crates/switch-core/src/plugins/{mod,skill,source,install}.rs` |
| 内容中心 | `crates/switch-core/src/content/{mod,feed}.rs` |
| 存储 | `crates/switch-core/src/storage/hub.rs`（migration v4 + `HubStore` + SQLite/内存两套实现） |
| 壳 | `src-tauri/src/{state,commands,main}.rs`（24 个新命令 + 运行时定时刷新任务） |
| 界面 | `src/features/{tools,plugins,content}/`、`src/app/App.tsx`、`src/locales/*` |

### 12.2 与设计文档的差异（都是有意为之）

| 设计文档 §| 文档写的 | 实际实现 | 理由 |
| --- | --- | --- | --- |
| 7.2 | 「并发：最多 2 个源同时抓」 | **串行**，一轮一个 | 串行是这个上界之内的一种实现，且对源站更友好、失败定位更简单。抓取总量本来就小（8 个源） |
| 5.2 | `feed_items` 存 `summary`、`lang` 等为独立列 | 只把 `published_at` / `first_seen_at` / `lang` 提成索引列，其余进 JSON 载荷 | 与既有实体表的做法一致（索引列负责排序过滤，载荷负责领域字段），加字段不必改表 |
| 5.1 | `ToolState` 含 `auth` 字段 | 收敛成 `status` 枚举 + `auth_probe_tail` | 两者回答同一个问题，合成一个枚举才不会出现「状态说已就绪、授权说没登录」 |
| 7 | 抓取间隔可在 15 分钟–24 小时之间设置 | 已实现（设置表 + 界面下拉） | 同文档 |
| — | 未提依赖 | 引入 `quick-xml 0.42` | 它本就在本应用的依赖图里（`plist` → `tauri` 间接引入），直接使用不新增下载面与许可面；手写 XML 扫描要处理的实体、CDATA、命名空间分支比依赖多 |

### 12.3 实现期发现并修掉的问题

- **CSS Module 漏声明属性被全局 `button` 规则接管**：技能卡片三行文字重叠 6–8px；工具行名称被居中。
  两处都由实测发现（审计报告 §0），已修并复测。
- **插件计划的目录名算了两遍**：`SkillPlan.dirName` 与 `TargetPlan.dir` 各自推导，冲突改名时会不一致。
  改为 `TargetPlan` 自带 `dir_name`，安装时只认计划里的那一个。
- **卸载后目录不消失**：技能带子目录（`references/`）时父目录永远「不为空」。改为先自底向上收掉空子目录。
- **`first_seen_at` 被刷新覆盖**：SQLite 的 upsert 会把载荷里的时间一起写回，等于抹掉「第一次见到它」这个事实。
  改为写入前先查既有值。
- **升级路径测试的假失败**：`tests/sqlite_repository.rs` 模拟 v1 库时只删了 v3 的表，v4 表残留导致
  「表已存在」。已把 v4 的表补进 DROP 列表，并在数据契约文档里写明「新增表要同步这里」。

### 12.4 验证

- `cargo test --workspace`：**565 项全过**（含 24 项内容中心、19 项插件、13 项工具管理、4 项存储新表与 2 项迁移）。
- `npx vitest run`（169 项全过，其中 33 项是本轮新增：工具 11 / 插件 11 / 内容 11）+ `tsc --noEmit`；三页覆盖状态映射、
  冲突默认跳过、部分完成如实分报、失败源保留旧内容等关键行为。另有 i18n 键集合守卫自动纳入新增文案。
- `npm run build`：通过。
- 真机浏览器走查：三页 × 两主题 × 两种窗口，重叠/溢出/裁切/对比度/点击目标全部为 0，见审计报告。

### 12.5 尚未在真机上跑过（2026-09-21 记）

这一批的表（`tool_probe_cache` / `installed_skills` / `feed_sources` / `feed_items`）由 schema 迁移 **3 → 4** 创建
（建表语句在 `crates/switch-core/src/storage/hub.rs`）。本机的元数据库 `metadata.sqlite` 到 2026-09-21 仍是
`user_version = 3`——**这些表在真机上还不存在**，也就是说扩展三页的持久化路径一次都没有在真库里跑过。

判断方法（下次别再从界面猜）：

```
sqlite3 ~/Library/Application\ Support/app.gptswitch.desktop/metadata.sqlite "pragma user_version;"
```

- `3`：说明写入这个库的构建早于本批代码，重启新构建后会自动迁移到 4 并建表。
- `4`：迁移已完成，那时才谈得上「抓取到底行不行」。

抓取时刻与保留策略在 2026-09-21 改成了「每天 06:00 / 18:00 + 整源覆盖」（见 §7.2），
所以下一次真机验证要同时确认三件事：时刻落点、每个源至少留下 10 条、刷新后旧条目不在了。

真机抓取还有一个已知风险点：抓取器用 `ureq::Proxy::try_from_env()` 读代理（`crates/switch-core/src/content/mod.rs`），
而 macOS 上从 Dock 启动的应用**不继承 shell 环境变量**，代理客户端常用的 `HTTPS_PROXY` 读不到就走直连；
连接超时 5 s、收包 15 s，配置代理的环境里很容易整片超时。界面上的「连续失败 N 次」是真的失败，
不是夹具——夹具里那条失败的源（Hacker News）只是合成数据。
