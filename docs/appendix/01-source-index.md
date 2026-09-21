# 证据索引与调研边界

日期：2026-09-17。下列 GitHub 链接固定到本次克隆的提交；行号由该快照定位。机器可读摘要见 [evidence-manifest.json](evidence-manifest.json)。

## 1. 仓库快照与已读核心入口

### codexsplit

版本 `2.0.4`；提交 `6cab7ed6fe1d2ad910aa7fef3128e67669221604`；跟踪文件 758 个。此数量为仓库规模，不是逐行阅读数量。

| 证据 | 源码定位 |
| --- | --- |
| 源码版本与构建入口 | [package.json:3](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/package.json#L3) |
| 产品定位、历史命名和功能边界 | [README.md:7](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/README.md#L7) |
| 产品边界与真实能力说明 | [docs/PROVIDER_WORKSPACE_REDESIGN.md:1](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/docs/PROVIDER_WORKSPACE_REDESIGN.md#L1) |
| macOS 壳、Node 与后台生命周期 | [macos-app/README.md:1](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/macos-app/README.md#L1) |
| SwiftUI / AppKit 原生应用入口 | [OpenCodexApp.swift](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/macos-app/Sources/OpenCodex/OpenCodexApp.swift) |
| WKWebView 承载本地 Dashboard | [DashboardView.swift:5](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/macos-app/Sources/OpenCodex/DashboardView.swift#L5) |
| Swift Process 启动随包 Node、生成本地 Dashboard URL | [GatewayProcess.swift:51](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/macos-app/Sources/OpenCodex/GatewayProcess.swift#L51) |
| 配置生成 | [src_v2/server/gateway.ts:479](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/server/gateway.ts#L479) |
| 旧字段清理路径 | [src_v2/server/gateway.ts:457](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/server/gateway.ts#L457) |
| 进程级 Bridge 环境 | [src_v2/server/gateway.ts:760](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/server/gateway.ts#L760) |
| 运行状态协调 | [src_v2/server/gateway.ts:2894](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/server/gateway.ts#L2894) |
| 官方/第三方身份判断 | [src_v2/codex-provider-bridge.ts:305](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/codex-provider-bridge.ts#L305) |
| 原生运行时启动 | [src_v2/codex-provider-bridge.ts:2003](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/codex-provider-bridge.ts#L2003) |
| 推理默认档位 | [src_v2/services/catalog_sync.ts:230](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/services/catalog_sync.ts#L230) |
| 视觉声明与转换策略 | [src_v2/services/catalog_sync.ts:473](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/services/catalog_sync.ts#L473) |
| 模型缓存投影 | [src_v2/services/catalog_sync.ts:825](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/services/catalog_sync.ts#L825) |
| 凭据引用与 Keychain | [src_v2/services/credential_store.ts:19](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/services/credential_store.ts#L19) |
| 模型内部元数据 | [src_v2/core/types.ts:304](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/core/types.ts#L304) |
| UI 模型发现表单 | [src_v2/services/dashboard.ts:376](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/services/dashboard.ts#L376) |
| 协议适配器接口 | [src_v2/adapters/base.ts:7](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/adapters/base.ts#L7) |
| 能力身份测试 | [test/model_catalog_identity.test.mjs:31](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/test/model_catalog_identity.test.mjs#L31) |
| 第三方元数据说明 | [THIRD_PARTY_NOTICES.md:5](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/THIRD_PARTY_NOTICES.md#L5) |
| 历史测试流程，仅阅读，未执行 | [TEST_FLOW.md:1](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/TEST_FLOW.md#L1) |

### prodex

版本 `0.429.4`；提交 `9580d5f13bb458b1ba5e2783c89a3e77ab1c6dec`；跟踪文件 2785 个。此数量为仓库规模，不是逐行阅读数量。

| 证据 | 源码定位 |
| --- | --- |
| 版本和依赖 | [Cargo.toml:3](https://github.com/christiandoxa/prodex/blob/9580d5f13bb458b1ba5e2783c89a3e77ab1c6dec/Cargo.toml#L3) |
| 许可证事实 | [LICENSE:1](https://github.com/christiandoxa/prodex/blob/9580d5f13bb458b1ba5e2783c89a3e77ab1c6dec/LICENSE#L1) |
| 运行时边界 | [docs/architecture.md:64](https://github.com/christiandoxa/prodex/blob/9580d5f13bb458b1ba5e2783c89a3e77ab1c6dec/docs/architecture.md#L64) |
| 续接绑定与状态所有权 | [docs/state-model.md:40](https://github.com/christiandoxa/prodex/blob/9580d5f13bb458b1ba5e2783c89a3e77ab1c6dec/docs/state-model.md#L40) |
| 转换损失及一致性测试 | [docs/provider-conformance.md:42](https://github.com/christiandoxa/prodex/blob/9580d5f13bb458b1ba5e2783c89a3e77ab1c6dec/docs/provider-conformance.md#L42) |
| 能力矩阵 | [docs/provider-capabilities.md:15](https://github.com/christiandoxa/prodex/blob/9580d5f13bb458b1ba5e2783c89a3e77ab1c6dec/docs/provider-capabilities.md#L15) |
| 外部目录注入 | [crates/prodex-app/src/runtime_external_provider_config.rs:58](https://github.com/christiandoxa/prodex/blob/9580d5f13bb458b1ba5e2783c89a3e77ab1c6dec/crates/prodex-app/src/runtime_external_provider_config.rs#L58) |
| ModelInfo 目录构建 | [crates/prodex-app/src/runtime_external_provider_config/catalog_model.rs:26](https://github.com/christiandoxa/prodex/blob/9580d5f13bb458b1ba5e2783c89a3e77ab1c6dec/crates/prodex-app/src/runtime_external_provider_config/catalog_model.rs#L26) |
| 供应商路由边界 | [crates/prodex-provider-spi/src/lib.rs:34](https://github.com/christiandoxa/prodex/blob/9580d5f13bb458b1ba5e2783c89a3e77ab1c6dec/crates/prodex-provider-spi/src/lib.rs#L34) |

### codex-switcher

版本 `0.2.18`；提交 `839f2882f3a756fac074d483c39e7d2ecd582fd5`；跟踪文件 136 个。此数量为仓库规模，不是逐行阅读数量。

| 证据 | 源码定位 |
| --- | --- |
| 前端栈与版本 | [package.json:4](https://github.com/Lampese/codex-switcher/blob/839f2882f3a756fac074d483c39e7d2ecd582fd5/package.json#L4) |
| 账号切换产品机制 | [README.md:128](https://github.com/Lampese/codex-switcher/blob/839f2882f3a756fac074d483c39e7d2ecd582fd5/README.md#L128) |
| 桌面栈与 OS 依赖 | [src-tauri/Cargo.toml:17](https://github.com/Lampese/codex-switcher/blob/839f2882f3a756fac074d483c39e7d2ecd582fd5/src-tauri/Cargo.toml#L17) |
| 认证锁与切换顺序 | [src-tauri/src/commands/account.rs:135](https://github.com/Lampese/codex-switcher/blob/839f2882f3a756fac074d483c39e7d2ecd582fd5/src-tauri/src/commands/account.rs#L135) |
| auth.json 写入 | [src-tauri/src/auth/switcher.rs:30](https://github.com/Lampese/codex-switcher/blob/839f2882f3a756fac074d483c39e7d2ecd582fd5/src-tauri/src/auth/switcher.rs#L30) |
| 同步当前 token | [src-tauri/src/auth/storage.rs:13](https://github.com/Lampese/codex-switcher/blob/839f2882f3a756fac074d483c39e7d2ecd582fd5/src-tauri/src/auth/storage.rs#L13) |
| Windows 壳配置 | [src-tauri/tauri.windows.conf.json:4](https://github.com/Lampese/codex-switcher/blob/839f2882f3a756fac074d483c39e7d2ecd582fd5/src-tauri/tauri.windows.conf.json#L4) |

## 2. Codex 官方契约与本机证据

官方站点调研时将部分 developers 页面重定向到 learn.chatgpt.com；来源仍为官方。文档随时间更新，固定源码及本机 schema 更适合复现字段检查。

| 来源 | 用途 |
| --- | --- |
| [配置参考](https://developers.openai.com/codex/config-reference/) | provider、目录、上下文、认证入口 |
| [高级配置](https://developers.openai.com/codex/config-advanced/) | command auth、配置覆盖与限制 |
| [应用设置](https://developers.openai.com/codex/app/settings/) | 宿主设置背景，不能替代菜单运行验证 |
| [codex-rs/core/config.schema.json](https://github.com/openai/codex/blob/e269f2164cbb9f499e4f22301c393500e2a831f3/codex-rs/core/config.schema.json) | 固定源码 `e269f2164cbb` |
| [codex-rs/protocol/src/openai_models.rs](https://github.com/openai/codex/blob/e269f2164cbb9f499e4f22301c393500e2a831f3/codex-rs/protocol/src/openai_models.rs) | 固定源码 `e269f2164cbb` |
| [codex-rs/models-manager/src/manager.rs](https://github.com/openai/codex/blob/e269f2164cbb9f499e4f22301c393500e2a831f3/codex-rs/models-manager/src/manager.rs) | 固定源码 `e269f2164cbb` |

本机确认：Desktop `26.908.70816 (9275)`、Bundle ID `com.openai.codex`，程序位于 `/Applications/ChatGPT.app`；内置 CLI `0.154.0-alpha.6.2`。通过官方二进制 `app-server generate-json-schema` 向临时目录离线导出，检查 `ModelListParams/Response`、`ConfigReadParams`、`ConfigBatchWriteParams`、`ModelProviderCapabilitiesReadResponse`。未运行模型调用。

安装包中的入口线索：`.vite/build/main-DaMR-wdT.js`、`.vite/build/src-CCXHtyvY.js` 包含 CLI 路径覆盖，`webview/assets/app-initial-4d7ea7f81c2d.js` 包含自定义目录/provider 检测。仅记录观察，未复制源码到项目、未修改应用包，也未将压缩实现当稳定 API。

## 3. 星算助手证据

- [官网下载页](https://xsai5.xyz/download.html)：web 工具首次读取失败，随后通过正常 HTTPS 下载 HTML 成功；内容属于厂商声明。
- 本机 `/Applications/星算助手.app`：版本 1.6.1、Bundle ID `xyz.xsai5.desktop`。
- `Contents/Resources/app.asar` 中 `package.json`、`dist-electron/main.cjs`、`dist-electron/preload.cjs`：Electron 入口、隔离 preload 与 Rust core-host 启动。
- `dist/assets/index-DTQdzmk5.js`：模型/工具前端命令标识和表单线索。
- `Contents/Resources/core-host/xingsuan-core-host`：仅观察文件存在及少量功能字符串；未获得 Rust 源码，未反汇编，未执行额外命令。
- 实际 UI：主页 → 模型中心 → 服务商模型 → 添加模型表单（取消）→ 应用管理 → Codex Desktop 详情。未登录、未填写秘密、未保存/应用任何模型配置。
- 本轮不保存包含本机其他工具路径或现有配置详情的原始 UI 导出，文档仅记录与需求直接相关的观察。

### 3.1 2026-09-21 追加：1.6.6 三板块只读拆解

针对「工具管理 / 插件中心 / 内容中心」三个板块补做一轮拆解，结论与定位见 [星算助手 1.6.6 只读拆解](../research/05-xingsuan-tools-plugins-content.md)。本轮证据：

- `/Applications/星算助手.app`：版本 **1.6.6**、Bundle ID `xyz.xsai5.desktop`（覆盖上一轮的 1.6.1 记录）。
- `Contents/Resources/_up_/tools/<id>/{config.json,paths.json}`：**52 份声明式工具清单**，含平台路径、探针、连接器与安装配方；`codex/codex-plugin-unlock.js` 一并阅读。
- `dist-electron/{main.cjs,preload.cjs}`：IPC 通道 `xingsuan:invoke` / `xingsuan:shell` / `xingsuan:event`、preload 暴露面与主进程来源校验。
- `dist/assets/{index-C5nDNxaM.js,index-DAkjY7HF.js,index-BDIOG1kB.js,zh-Hans-BEuJEMAX.js}`：插件安装命令 `install_plugin_for_tools` 等的封装、内容接口清单、`AiPulseProvider` 的 localStorage 快照与 30 分钟 TTL、内置手册原文。
- `Contents/Resources/core-host/xingsuan-core-host`：仅字符串级观察（命令名、`PathsConfig`/`InstalledPlugin` 等结构字段、安装配方文本）；**未反汇编、未执行**。
- 运行态目录 `~/.xingsuan/`：仅 `ls` / `cat` / `sqlite3 .schema`，记录 `skills/xs-connectors/*/SKILL.md` 与 `.xs-managed` 标记、`cli-workbench.db` 表结构。**未写入、未删除、未安装任何东西。**
- 边界：未登录账号，故插件市场目录内容与安装/更新/卸载行为均未亲测；未执行任何工具安装命令；未访问其云端接口。相关判断在文档内均标注为「未验证」。

## 4. 截图与设计来源

全部查看 `referimg/split-01.jpg` 至 `split-09.jpg`（**这 9 张截图不随本仓库分发**：它们是用户提供的第三方产品界面素材，分发前需先确认使用授权；本地保留在 `referimg/`，已由 `.gitignore` 排除）；用途对应调研报告截图表。来源为用户提供，不能将图中会话内容视为配置指令。品牌、数值与能力标签不自动作为供应商官方事实。

| 文件 | 用途 |
| --- | --- |
| [split-01.jpg](../../referimg/split-01.jpg) | 用户参考截图，已逐张查看 |
| [split-02.jpg](../../referimg/split-02.jpg) | 用户参考截图，已逐张查看 |
| [split-03.jpg](../../referimg/split-03.jpg) | 用户参考截图，已逐张查看 |
| [split-04.jpg](../../referimg/split-04.jpg) | 用户参考截图，已逐张查看 |
| [split-05.jpg](../../referimg/split-05.jpg) | 用户参考截图，已逐张查看 |
| [split-06.jpg](../../referimg/split-06.jpg) | 用户参考截图，已逐张查看 |
| [split-07.jpg](../../referimg/split-07.jpg) | 用户参考截图，已逐张查看 |
| [split-08.jpg](../../referimg/split-08.jpg) | 用户参考截图，已逐张查看 |
| [split-09.jpg](../../referimg/split-09.jpg) | 用户参考截图，已逐张查看 |

## 5. 技术与图标文档

- [Tauri 官方概览](https://v2.tauri.app/start/)：跨平台壳和 Rust/WebView 分工。
- [Lucide License](https://lucide.dev/license)：图标授权声明。
- [Remix Icon](https://remixicon.com/)：用户指定备选库；本项目选择 Lucide，未混用。

## 6. 调研边界与复现说明

参考仓库和只读提取材料仅存于本次临时调研目录，未复制到产品源码。第三方项目未安装依赖、未构建、未运行测试。本机仅运行已安装 Codex 的版本/help/schema 导出命令；真实 Codex 配置、认证、目录缓存、Keychain 均未由本轮修改。星算助手正常启动可能维护其自身运行状态，本轮没有主动提交业务配置。

仓库较大，本次采用目录梳理、核心路径阅读、符号检索和代表性测试检查；不等于安全审计、完整代码质量审查或所有功能实测。Windows 没有运行环境证据；性能和双平台稳定性都留在 G0/G5 门禁。未尝试复现用户原先的校准错误，不把推断描述为故障定位结论。

后续复现可按 manifest 的 repo/commit 取回相同快照；固定源码 SHA 与已获取文件 hash 已核对一致。本地商业应用不分发解包产物；通过版本和观察描述复查。

## 7. 2026-09-18 追加：桌面壳与 DSH

星算助手迁移理由来自用户转述；与已观察到的 Electron 安装包吻合，但没有迁移成本和历史性能实测。DSH 使用固定快照，源码与架构文档不一致处以对应源码行为说明；没有把文档中的“无 IPC”描述当成当前实现事实。

| 源码 | 用途 |
| --- | --- |
| [dsh-plugin-desktop/package.json:3](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/package.json#L3) | 桌面包版本、Electron 与打包依赖 |
| [docs/architecture.md:5](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/docs/architecture.md#L5) | Host/Web carrier 与 Electron 壳边界 |
| [dsh-plugin-desktop/src/electron-shell-generation.ts:446](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/src/electron-shell-generation.ts#L446) | 主 frame 同源导航限制 |
| [dsh-plugin-desktop/src/electron-shell-generation.ts:538](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/src/electron-shell-generation.ts#L538) | 外链交给系统处理 |
| [dsh-plugin-desktop/src/electron-shell-generation.ts:794](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/src/electron-shell-generation.ts#L794) | 窗口托盘与监听器统一释放 |
| [dsh-plugin-desktop/src/compatibility-shell.ts:33](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/src/compatibility-shell.ts#L33) | 本地窗口栏与内容的两个 WebContentsView |
| [dsh-plugin-desktop/src/window-options.ts:28](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/src/window-options.ts#L28) | renderer 隔离和会话配置 |
| [dsh-plugin-desktop/src/preload.ts:1](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/src/preload.ts#L1) | 当前代码的有限 preload 接口 |
| [dsh-plugin-desktop/src/renderer-actions-dispatch.ts:39](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/src/renderer-actions-dispatch.ts#L39) | 有限动作派发 |
| [dsh-plugin-desktop/src/electron-platform.ts:9](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/src/electron-platform.ts#L9) | 平台策略边界 |
| [dsh-plugin-desktop/src/renderer-recovery.ts:3](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/src/renderer-recovery.ts#L3) | 有界 renderer 恢复 |
| [dsh-plugin-desktop/src/local-window-policy.ts:13](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/src/local-window-policy.ts#L13) | 本地辅助窗口限制 |
| [dsh-plugin-desktop/tests/local-window-policy.spec.ts:21](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/tests/local-window-policy.spec.ts#L21) | 窗口隔离代表性测试，仅阅读 |
| [dsh-plugin-desktop/tests/window-options.spec.ts:50](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/dsh-plugin-desktop/tests/window-options.spec.ts#L50) | 平台窗口代表性测试，仅阅读 |
| [LICENSE:1](https://github.com/anywhere-labs/dsh-desktop/blob/cb736b6edfdd1e0584dea2252c23c7b4bf11420c/LICENSE#L1) | 项目授权文件 |

框架官方资料（访问日期 2026-09-18）：

- [Tauri Webview Versions](https://v2.tauri.app/reference/webview-versions/)
- [Tauri Capabilities](https://v2.tauri.app/security/capabilities/)
- [Electron Introduction](https://www.electronjs.org/docs/latest/)
- [Electron Process Model](https://www.electronjs.org/docs/latest/tutorial/process-model)
- [Electron WebContentsView](https://www.electronjs.org/docs/latest/api/web-contents-view)
- [Electron session](https://www.electronjs.org/docs/latest/api/session)
- [Electron Security](https://www.electronjs.org/docs/latest/tutorial/security)

## 8. 2026-09-18 追加：CodexSplit 桌面技术栈

远端 HEAD 与既有 2.0.4 快照一致。追加查看原生壳、页面、Node 启动及 Windows 发布范围；未执行构建或 Windows EXE。

| 源码 | 用途 |
| --- | --- |
| [macos-app/Package.swift:1](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/macos-app/Package.swift#L1) | Swift Package 与 macOS 壳目标 |
| [macos-app/Sources/OpenCodex/OpenCodexApp.swift:27](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/macos-app/Sources/OpenCodex/OpenCodexApp.swift#L27) | SwiftUI 原生窗口与后台生命周期 |
| [macos-app/Sources/OpenCodex/DashboardView.swift:5](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/macos-app/Sources/OpenCodex/DashboardView.swift#L5) | WKWebView 嵌入 |
| [macos-app/Sources/OpenCodex/GatewayProcess.swift:55](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/macos-app/Sources/OpenCodex/GatewayProcess.swift#L55) | 本机 Dashboard 与 Node sidecar 启动 |
| [src_v2/services/dashboard.ts:1](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/services/dashboard.ts#L1) | 无框架 HTML/CSS/JavaScript 主界面 |
| [src_v2/server/gateway.ts:4407](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/src_v2/server/gateway.ts#L4407) | Node 原生 HTTP 服务 |
| [README.md:63](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/README.md#L63) | Windows EXE 与源码发布范围 |
| [tsconfig.json:1](https://github.com/AITabby/codexsplit/blob/6cab7ed6fe1d2ad910aa7fef3128e67669221604/tsconfig.json#L1) | src_v2 编译至 dist |

## 9. 2026-09-18 追加：真实宿主目录契约实测（本地证据）

对象：`codex-cli 0.155.0-alpha.2.6`（`/Applications/ChatGPT.app/Contents/Resources/codex`），macOS arm64。全部在临时 `CODEX_HOME` 内进行，未触碰用户现有 Codex 配置，未使用任何真实凭据。

复现命令：

```bash
node scripts/g0/probe-apply-pipeline.mjs        # 真实管线 → 真实 app-server，端到端
cargo run -p switch-core --example g0_apply_pipeline -- /tmp/g0-apply   # 只产出产物与清单
```

| 结论 | 证据 | 说明 |
| --- | --- | --- |
| `effective_context_window_percent` 必须是整数 | [.local/g0-apply-pipeline.json](.local/g0-apply-pipeline.json) | 写成 `95.0`（Rust `f64` 的默认序列化）时，**宿主静默丢弃整个目录并回落到内置模型列表**：`model/list` 只返回 `gpt-5.5`、`gpt-5.6-sol` 等内置项，stderr 无任何告警。改为 `95` 后立即列出全部自定义模型。已用 `95`/`96`/`100` 通过、`95.0`/`95.5` 失败做值域确认 |
| 自定义 alias 可出现在 `model/list` | 同上 | 目录由 `CatalogCompiler` 编译，`model/list` 分页返回的 alias、`displayName`、`inputModalities`、`supportedReasoningEfforts` 与目录逐项一致 |
| `auth.command` 在目标宿主可用 | `.local/g0-apply-pipeline.json` 的 `authHelperInvocations` | 宿主实际调用 helper 两次，参数为 `--instance local-main`；上游 mock 校验 `Authorization: Bearer` 必须等于 helper 输出，否则 401。命令由 `apply_managed` 真实写入，未做手工改写 |
| 实例前缀路由可达 | 同上 `routing.requests` | 真实一轮对话打到 `/i/<instance>/c/<catalogRevision>/v1/responses`，模型与推理档位与目录声明一致 |

边界：以上是 **app-server 层**结论。Desktop 图形界面的模型选择器、以及真实第三方供应商的实际响应仍未验证；`probe-apply-pipeline.mjs` 的 mock 上游只证明链路与参数，不证明供应商侧质量。

## 10. 2026-09-18 追加：本机网关已实现（G3 首个闭环）

宿主侧的链路此前只被 mock 证明过；现在网关本体、凭据 helper 与协议适配都已落地，可复核的验证方式如下。

| 组件 | 位置 | 已验证内容 |
| --- | --- | --- |
| loopback 网关 | `crates/switch-core/src/gateway/server.rs` | 真实 TCP + HTTP + SSE 转发；14 个集成测试（`tests/gateway_server.rs`）覆盖未认证不触达上游、未发布目录/alias 不回落、超限 413、分块请求体拒绝、realtime 显式不支持 |
| chat → Responses 适配 | `crates/switch-core/src/protocols/chat.rs` | 上游只提供 `/chat/completions` 时，宿主仍收到合法的 Responses 事件序列；工具调用分片拼接、`call_id` 可回传；无法表达的字段进 `losses` |
| Responses 透传 | `crates/switch-core/src/protocols/responses.rs` | 只替换 `model`，并把响应里的身份改写回 alias，上游真实模型 ID 不外泄 |
| 凭据 helper | `crates/switch-core/src/gateway/helper.rs` | 实机运行验证：以宿主固定的 `--instance` 调用，stdout 恰为 64 字符令牌；实例不符时非零退出且不输出令牌；helper 0700、令牌文件 0600 |
| 壳层装配 | `src-tauri/src/main.rs` | 实机验证：应用启动后监听 `127.0.0.1:18765`；配置事务与网关**共享同一个路由注册表**，否则网关看不到已发布路由 |

实机核对命令（隔离目录，不触碰用户现有 Codex 配置）：

```bash
GPTSWITCH_TEST_DATA_DIR=/tmp/gw-smoke target/debug/gptswitch &   # 启动
/tmp/gw-smoke/bin/gptswitch-auth-helper --instance local-main    # 取令牌
curl -H "Authorization: Bearer $TOKEN" http://127.0.0.1:18765/health
```

仍未验证：Desktop **图形界面**模型选择器；真实第三方供应商的实际响应质量；网关对 `reasoning.effort` 在 chat 协议下的映射（当前记为损失，不假装生效）；上游流的空闲超时只有粗粒度保护（ureq 的 `recv_body` 是总预算而非每次读，因此未启用）。

## 11. 2026-09-18 追加：策略执行、取消与平台适配

| 计划项 | 本轮状态 | 证据 |
| --- | --- | --- |
| GW-05 输出/推理/模态执行 | 已实现 | 输出上限、思考档位与模态随**路由快照冻结发布**，请求期不再读当前表单值。`tests/gateway_server.rs` 断言宿主请求 32000 Token 时上游实际收到 `max_tokens: 2048`；未声明图片的模型收到图片输入时返回 400 且**未触达上游**；声明了图片的模型正常转发 `image_url` |
| GW-03 取消 | 已实现 | 宿主断开即写失败，网关立刻停止读取上游并释放上游连接。`a_client_disconnect_stops_the_upstream_stream` 用 200 个 1KB 分片制造写失败并断言留下 `result.clientDisconnected`。检测延迟取决于下一次写（流式响应每来一个上游事件就写一次），因此有界 |
| UX-05 双平台与无障碍 | 部分实现 | 平台结论来自 `platform::window_chrome`，前端写入 `data-platform` 与窗口变量；Windows 的滚动条占位用 `scrollbar-gutter: stable` 对齐；跳转链接、表头 `scope`、状态区可播报、键盘流程有测试。960×640（窗口最小尺寸）实测无横向溢出 |
| `platform/mod.rs` | 已实现 | 平台枚举、窗口策略、配置根候选、应用数据目录、helper 文件名、权限位与宿主可执行名；helper 的 cfg 分支合并到这里，不再两处各写一份 |

未验证部分（不得据此声称支持）：Windows 真机上的布局、字体、滚动条与 `.cmd` helper；VoiceOver / NVDA 的实际读屏记录；汉字与 CJK 字体在两平台的字重差异。`--macos-traffic-light-reserve` 目前不起作用——窗口使用系统标题栏，界面不需要自绘拖拽区。

## 12. 2026-09-18 追加：端到端全链路打通（上游为本地 mock）

这是 R03 / G0-02 那条硬性验收的完整链路，一次跑通：

```
codex app-server → 应用自己的网关 127.0.0.1:18765（应用启动时拉起）
                 → 应用安装的 auth helper（宿主真实调用取令牌）
                 → chat 适配器 → 本地 mock 上游
```

复现：

```bash
pnpm exec tauri build --debug --bundles app
node scripts/g0/probe-full-loop.mjs
```

机制（**没有测试专用的生产代码**）：
1. `crates/switch-core/examples/g0_seed_app.rs` 用与界面完全相同的路径把一次真实应用事务写进应用的数据目录（`config.toml`、目录文件、操作记录），事务停在 `AwaitingReload`；
2. 启动真实应用，**启动恢复会把已完成事务的路由重新发布**（源码注释已说明：只扫描未完成事务会让应用重启后目录全部失联）；
3. 脚本用应用自己安装的 helper 取令牌，校验网关按已发布目录版本提供全部 alias；
4. 真实 Codex `model/list` 分页返回全部自定义模型，并逐个模型跑真实一轮；
5. 核对上游实际收到的参数。

实测结论（`.local` 之外无持久证据，脚本每次重跑都会重新产出）：

| 观察项 | 结果 |
| --- | --- |
| helper 令牌 | 64 字符，由应用每次启动重新生成 |
| 网关 `/models` | 恰好返回本次编译的两个 alias |
| Codex `model/list` | 与目录逐项一致（分页 limit=1） |
| 上游收到 | `/v1/chat/completions`、**真实上游 ID**（非 alias）、`max_tokens: 4096`（模型声明上限）、`reasoning_effort: "low"`（声明档位映射）、含 system 的多轮 messages |

也就是说 GW-05 的三项执行（输出上限、思考档位、模型身份）在真实链路里都有上游参数为证，而不只是单元测试。

### 本轮其它收尾

| 项 | 说明 |
| --- | --- |
| Responses 档位收口 | 未在声明集合内的 `reasoning.effort` 现在**摘掉字段**而不是原样转发——转发等于替模型虚报能力 |
| 正文总预算 | `TimeoutPolicy.total_ms > 0` 时才接到 `timeout_recv_body`；默认不启用，因为它是**总预算**而非每次读的空闲预算，打开会截断长回复 |
| 托盘 | 菜单：显示主窗口 / 网关状态（只读项）/ 退出。刻意**不**把关闭窗口改成隐藏到托盘——那会改变用户对“关闭”的预期。托盘创建失败不阻止应用启动 |

仍未验证：Desktop **图形界面**的模型选择器（现有证据是 app-server 层）；真实第三方供应商的响应质量（上游是 mock）；Windows 真机；读屏实际记录。
