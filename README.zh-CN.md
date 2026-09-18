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

- **模型菜单**来自编译后的目录文件（`model_catalog_json`），不是本工具自绘的列表。
- **上游 Key 永不写入 `config.toml`**：只进系统凭据库，宿主拿到的只是本机网关令牌。
- 写 Codex 配置一律走 **计划 → 摘要校验（CAS）→ 原子替换**，提交成功最多到"等待宿主重载"，不自行宣称已加载。
- 上游是 `chat/completions` 时由网关双向翻译成 Responses；无法表达的字段**明确记为损失**，不假装生效。
- **界面中英双语**：默认跟随系统语言，可在「设置 → 外观与语言」里固定为简体中文或 English，切换立即生效并记住。两套文案的键集合由 `src/i18n.test.ts` 守住，缺一条就失败，不会静默回退成中文。

## 下载与安装

从 [Releases](https://github.com/nexsjournal/switchelp-macapp/releases) 下载：

| 平台 | 文件 | 首次打开 |
| --- | --- | --- |
| macOS（Apple Silicon） | `Switchelp_0.1.2_aarch64.dmg` | 已用 Developer ID 签名，但**尚未公证**：首次打开需**右键 → 打开**，或执行一次 `xattr -dr com.apple.quarantine /Applications/Switchelp.app` |
| macOS（Apple Silicon） | `Switchelp-0.1.2-arm64.zip` | 同上，解压后把 `Switchelp.app` 拖进 `/Applications` |
| Windows（x64） | `Switchelp_0.1.2_x64-setup.exe`（NSIS 安装器）<br>`Switchelp_0.1.2_x64_en-US.msi` | 未签名，SmartScreen 会提示“未知发布者”，点“仍要运行” |

更早的 0.1.0 产物名仍是旧的 `GPTSwitch`——它们是在产品与仓库改名之前构建的。应用标识仍是 `app.gptswitch.desktop`（有意保留，让旧版本的应用数据与凭据继续可用）。

**为什么 macOS 会提示**：签名与公证是两道关卡，本项目目前只有前者。
补齐公证需要账号所有者提供凭据，步骤见 [签名、公证与发布](docs/development/03-signing-and-release.md)；
配好之后双击即可打开，且 CI 会自动产出带公证的包。

**Windows 现状**：应用能打开，但**第三方模型在 Windows 上尚不可用**——凭据 helper 的 `.cmd`
实现仍是显式未完成的桩，只保证失败可诊断，不假装可用。详见
[证据索引](docs/appendix/01-source-index.md)。

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
把你自己的私有特征写在仓库外的 `~/.gptswitch-private-patterns` 里即可一并检查）：

```bash
printf '%s\n' 'api.your-provider.example' > ~/.gptswitch-private-patterns
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

机制层面已经端到端跑通，但**尚未用真实第三方供应商验证过**：

- 已实测：真实 Codex app-server 能列出自定义模型并路由到本机网关；`chat` 协议适配、输出上限执行、思考档位映射、模态拒绝都在真实链路里以上游收到的请求参数为证；上游断开即取消。
- 未验证：Desktop **图形界面**的模型选择器（现有证据是 app-server 层）、真实供应商的响应质量、Windows 真机、读屏实际表现。

设计与调研文档在 [`docs/`](docs/README.md)，含证据等级与未决问题清单（目前只有中文）。

## 未随仓库分发

`referimg/` 中的 9 张界面参考截图是用户提供的第三方产品素材，**不在本仓库内**（已由 `.gitignore` 排除）；若要随仓库分发，请先确认使用授权。

本仓库目前未附带开源许可证。
