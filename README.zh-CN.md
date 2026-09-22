# Switchelp

[English](README.md) · **简体中文**

让第三方供应商的模型出现在 **Codex 自己的模型选择器**里，并管理供应商、API Key、每模型的上下文 / 输出上限 / 输入能力 / 思考档位。

本机 macOS 配置工具。Tauri 2 + React 18 + TypeScript 外壳，业务判定全部在一个不依赖窗口框架的 Rust 核心（`crates/switch-core`）里。

## 它怎么工作

```
Codex  →  ~/.codex/config.toml（本工具受管字段）
       →  model_providers.gptswitch → 本机网关 127.0.0.1:18765
       →  auth helper 取本机令牌 → 按 alias 路由到供应商 → 上游
```

- **模型菜单**来自编译后的目录文件（`model_catalog_json`），不是本工具自绘的列表。注意 `model_catalog_json` 是**整体替换**宿主的模型列表而不是追加：工具处于已应用状态时，原生模型不在选择器里，要「还原为原生 Codex」才会回来。
- **共存模式（Bridge）**让菜单里同时保留官方模型。它不改你真实的配置，而是在应用数据目录里写一份托管 profile，并通过 `gptswitch-bridge` 启动 Codex：那个 bridge 起两根 codex（你那根原样不动、托管那根读我们写的配置），合并 `model/list` 与 `thread/list`，并把每个会话钉在它当初所在的那根上。目前只有 macOS 装配完整；它由本应用启动宿主时生效（自己从 Dock 重开 Codex 会回到纯原生，界面会如实告诉你）。

- **上游 Key 永不写入 `config.toml`**：只进系统凭据库，宿主拿到的只是本机网关令牌。
- 写 Codex 配置一律走 **计划 → 摘要校验（CAS）→ 原子替换**，提交成功最多到"等待宿主重载"，不自行宣称已加载。
- 上游是 `chat/completions` 时由网关双向翻译成 Responses；无法表达的字段**明确记为损失**，不假装生效。
- **界面中英双语**：默认跟随系统语言，可在「设置 → 外观与语言」里固定为简体中文或 English，切换立即生效并记住。两套文案的键集合由 `src/i18n.test.ts` 守住，缺一条就失败，不会静默回退成中文。

## 下载与安装

从 [Releases](https://github.com/nexsjournal/switchelp-macapp/releases) 下载。

**当前版本 0.2.0 只发布 macOS Apple Silicon 两个包**；Windows 与 Intel Mac 的产物没有附在这次发布上，原因与替代做法见表格下方。

| 平台 | 文件 | 首次打开 |
| --- | --- | --- |
| macOS（Apple Silicon） | `Switchelp_0.2.0_aarch64.dmg` | 已用 Developer ID 签名，但**尚未公证**，首次打开会被拦截——见下方两条路 |
| macOS（Apple Silicon） | `Switchelp-0.2.0-arm64.zip` | 同上，解压后把 `Switchelp.app` 拖进 `/Applications` |

**为什么 macOS 会拦住，以及怎么过去**：签名与公证是两道关卡，本项目目前只有前者，所以首次打开会被系统拒绝。两条路，先试第一条：

1. **系统设置 → 隐私与安全性**，在「安全性」一栏里点被拦截应用旁边的**仍要打开**，再输入密码确认。这是 Apple 现在官方文档里写的路径——它已经不再提「右键 → 打开」那个旧办法，所以右键没反应是正常的。
2. 打开一次终端，执行下面这行，之后正常双击即可：

   ```bash
   xattr -dr com.apple.quarantine /Applications/Switchelp.app
   ```

补齐公证需要账号所有者提供凭据，步骤见 [签名、公证与发布](docs/development/03-signing-and-release.md)；
配好之后双击即可打开，且 CI 会自动产出带公证的包。

更早的 0.1.0 产物名仍是旧的 `GPTSwitch`——它们是在产品与仓库改名之前构建的。应用标识仍是 `app.gptswitch.desktop`（有意保留，让旧版本的应用数据与凭据继续可用）。

**Windows 现状**：0.2.0 **没有发布任何 Windows 产物**。构建任务本身在（[`release.yml`](.github/workflows/release.yml) 的 `build-windows`），但它只在手动触发并勾选 `with_windows` 时才跑——打标签发布永远不会构建它。

即便构建出来，**这个应用在 Windows 上会拒绝应用配置**，并在界面上说明原因，而不是照写不误：凭据 helper 在 Windows 上还没有实现，没有它 Codex 无法对本地网关鉴权，每一次请求都会失败。以前的做法——配置照写、界面报成功——比没用更糟：它把一个本来可用的 Codex 弄坏，还让人以为是上游的问题。详见[证据索引](docs/appendix/01-source-index.md)。

**Intel Mac 现状**：发布矩阵里**是**包含 `x86_64-apple-darwin` 的，但 `build-macos` 任务只在 CI 配好 Apple 签名密钥时才运行，而 0.2.0 的包是本机在 Apple Silicon 上签的。需要 Intel 包请从源码构建（`pnpm exec tauri build`）。

## 构建与验证

```bash
pnpm install
pnpm typecheck && pnpm test          # 前端：类型 + 单元测试
cargo test -p switch-core            # 核心：集成 + 单元测试
pnpm exec tauri build --debug --bundles app
```

端到端验收（真实应用 + 真实网关 + 真实 Codex，上游为本地 mock）：

```bash
pnpm exec tauri build --debug --bundles app
node scripts/g0/probe-full-loop.mjs
```

发布或推送前跑一次隐私扫描（规则是通用的，脚本本身不含任何个人标识；
把你自己的私有特征写在仓库外的 `~/.switchelp-private-patterns` 里即可一并检查）：

```bash
printf '%s\n' 'api.your-provider.example' > ~/.switchelp-private-patterns
scripts/check-publish-safety.sh
```

## 安全边界

| 项 | 做法 |
| --- | --- |
| 上游 API Key | 只写系统凭据库（macOS Keychain / Windows 凭据管理器）；SQLite 只存引用与掩码 |
| 本机网关 | 只绑 `127.0.0.1`；每次启动重新生成令牌；拒绝带 `Origin` 与浏览器预检的请求 |
| 令牌下发 | helper 从应用数据目录（0700）读取令牌文件（0600），stdout 只输出令牌 |
| 诊断日志 | **allowlist 结构化提取为主**：不在白名单的字段名直接丢弃，值再过一道脱敏；诊断包先预览再保存 |

## 当前状态

机制层面已经端到端跑通，并且**已在 macOS 上用真实第三方供应商验证过**：

- 已实测：真实上游返回了一次经本机网关路由的完整推理；真实 Codex 的 `model/list` 能列出受管模型；Desktop app-server 会用受管模型真实开会话（`logs_2.sqlite` 里有 `thread/start`，`client_name="Codex Desktop"`，模型是受管别名）；应用配置会提交并重启宿主；`chat` 协议适配、输出上限执行、思考档位映射、模态拒绝都以真实上游收到的请求参数为证；上游断开即取消。
- **应用之前值得先读的一条**：`model_catalog_json` 是**整体替换**宿主的模型列表。工具处于已应用状态时，原生模型从 Codex 选择器里消失，只有「还原为原生 Codex」才能拿回来。已由 `model/list` 只返回 1 条证实。修复方案与「应用弹窗里明确警告」记在 [2026-09-20 审计](docs/audits/2026-09-20-audit-synthesis.md)。
- 未验证：Windows 真机、读屏实际表现，以及 Desktop 图形界面选择器的**视觉**表现——上面的证据来自 app-server 与日志层，不是看着那个菜单得出的。

设计与调研文档在 [`docs/`](docs/README.md)，含证据等级与未决问题清单（目前只有中文）。

## 未随仓库分发

`referimg/` 中的 9 张界面参考截图是用户提供的第三方产品素材，**不在本仓库内**（已由 `.gitignore` 排除）；若要随仓库分发，请先确认使用授权。

以 [MIT 许可证](LICENSE) 发布。
