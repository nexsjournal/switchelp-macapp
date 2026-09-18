# GPTSwitch

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

设计与调研文档在 [`docs/`](docs/README.md)，含证据等级与未决问题清单。

## 未随仓库分发

`referimg/` 中的 9 张界面参考截图是用户提供的第三方产品素材，**不在本仓库内**（已由 `.gitignore` 排除）；若要随仓库分发，请先确认使用授权。

本仓库目前未附带开源许可证。
