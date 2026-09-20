# Codex 接入可行性与字段边界

关联：[官方及本地证据](../appendix/01-source-index.md) · [配置事务](../architecture/02-configuration-lifecycle.md) · [G0 验证](../development/02-testing-and-release.md)

## 1. 结论与不确定性

用户需要的供应商、Key 和模型参数管理可实现；模型进入 Codex 原生选择器已有参考实现。最需要先验证的是：本项目能否在目标 Desktop 版本中，仅依靠官方配置与自定义目录完成菜单接入，避免完整 Bridge 的维护成本。

本机发现 Desktop 为 `ChatGPT.app`，Bundle ID `com.openai.codex`，版本 `26.908.70816 (9275)`，内置 CLI `0.154.0-alpha.6.2`。这提示安装识别必须用应用身份与二进制探测，不能只找 `Codex.app`。

已从本机二进制生成 app-server JSON schema；只进行了离线导出，没有启动测试会话或改变用户配置。本机安装包还可定位 `CODEX_CLI_PATH`、`model_catalog_json` 与自定义 provider 检测代码。这是存在接入入口的证据，**不是该组合已通过运行验证**。

## 2. 四类能力必须区分

```text
模型厂商能力 → 当前服务商端点能力 → 网关实现能力 → Codex 宿主入口能力
                           ↓
                     最终可用能力
```

交集原则：任何一层明确不支持，就不能显示为整条链路“原生可用”；任一层未知则显示待验证。转换策略形成另一条明确标注的路径，不得把转换等同于原生。

例如：某模型支持视频，但 Codex 的当前附件入口没有传视频事件，配置工具无法凭空增加该入口。PDF 通过本地工具提取文字后进入上下文，与模型原生接收 PDF 文件，是不同能力。

## 3. 配置字段映射

以下以官方配置文档、固定源码和本机 schema 交叉检查。最终由版本适配器生成，不能直接把表格当作跨版本通用配置。

| 用户设置 | 写入/执行位置 | 生效及限制 |
| --- | --- | --- |
| 默认模型 | `model` | 默认值不等于覆盖已有任务选择 |
| 自定义供应商 | `model_provider` + `model_providers.<id>` | 不覆盖内置 provider ID；受宿主配置优先级影响 |
| Base URL | 自定义 provider 的 `base_url` | 推荐指向受认证的本机网关；真实上游地址留在本工具 |
| 认证 | `auth.command` 或 `env_key`，按宿主支持选择 | helper 从系统凭据取本机访问令牌；GUI 进程未必继承终端 env |
| 协议 | Codex 侧 `wire_api = "responses"` | 本次上游 schema 的 WireApi 只有 responses；Chat 在网关转译 |
| 目录路径 | `model_catalog_json` | 启动时加载；不能承诺 per-thread 覆盖能热更新 |
| 菜单名 | 目录 `display_name` | 实际显示方式需 Desktop 真机确认 |
| 模型标识 | 目录 `slug` → 网关 alias → 上游原始 ID | 展示名可改，路由 alias 不随改名改变 |
| 上下文 | 目录 `context_window` / `max_context_window` | 全局 `model_context_window` 可能覆盖每模型值；优先目录 |
| 压缩阈值 | 目录 `auto_compact_token_limit` 或全局字段 | 与有效窗口、输出预留共同校验；不等于输出长度 |
| 最大输出 | 本工具策略 → 请求 `max_output_tokens` 或适配器参数 | 未在本次 Codex config / ModelInfo 发现通用最大输出字段；不得虚构 |
| 思考默认值 | `model_reasoning_effort` 或目录默认档位 | 当前任务覆盖优先；不代表可控制内部推理全文 |
| 思考可选档位 | 目录 `supported_reasoning_levels` | app-server 返回为 `supportedReasoningEfforts`，schema 不同 |
| 输入模态 | 目录 `input_modalities` | 本机协议有 text / image / audio，无 pdf / video 枚举 |
| 工具输出截断 | `tool_output_token_limit` | 不是模型最大输出，UI 禁止混用 |
| 推理摘要 | `model_reasoning_summary` 等已支持参数 | 是摘要请求，不是“开启思考”的万能开关 |

官方配置参考可核对字段入口；本次源码和本机 schema 对动态 reasoning 字符串、启动时目录加载等提供了更精确的版本信息。[官方配置参考](https://developers.openai.com/codex/config-reference/)

## 4. 接入路线比较

| 路线 | 模型菜单 | 切 Key / URL | 参数约束 | 风险与决定 |
| --- | --- | --- | --- | --- |
| 直接改 provider / config | 需验证自定义目录 | 常需宿主重载 | 输出等难统一 | 保留为高级导出/诊断，不作为主体验 |
| 官方配置 + 本地网关 + 自有目录 | 目标首选；先 G0 | 同身份 Key/网关策略可热发布 | 可按模型约束 | **推荐基线**，所有第三方模型归统一 provider |
| stdio Bridge + 原生 app-server | CodexSplit 有实现证据 | 可做更细请求路由 | 可控但侵入面更大 | 按版本白名单启用，专门处理官方/第三方共存 |
| 持续改宿主缓存 / 注入 Web UI | 脆弱 | 无可靠契约 | 难证明 | 不采用为默认方案 |
| 自造聊天客户端 | 自己能显示 | 可控 | 可控 | 不满足用户要改原生模型选择器的要求 |

首发第三方模式中，用户添加的多家服务商一起显示在目录，均通过 `gptswitch` provider 到网关；模型名称带供应商用于区分。官方 OpenAI API Key 可作为用户供应商。**官方 ChatGPT 订阅登录不是 API Key 供应商**，保留原生模式切换；在 G0 没证明同菜单隔离前，不把订阅模型加入第三方网关目录。

> 2026-09-20 更新：**这一条已被取代**。用户选定「同一菜单共存」为当前实现路线（见 [PRD](../01-product-requirements.md) 的 P1 与 [调研 03](03-desktop-shell-decision.md)），不再走「两模式切换」。接入点已实测：桌面端用 `CODEX_CLI_PATH` 顶替内置 codex 并与之走 stdio 上的 app-server；探针确认宿主真的会调用被注入的 CLI。上面这条结论仍然成立的部分是：**订阅模型不进我们自己的网关目录**——共存要靠 bridge 把请求交给持有登录态的宿主侧，而不是把原生 slug 塞进目录。

如果基线路线在任一目标平台失败，评估 Bridge，而不是将核心需求降级为“只在本工具显示模型”。成本仍不受控时报告不支持的版本与原因，不默认换成自有聊天应用。

## 5. 目录与模型选择的细节

1. 目录文件结构按当前 Codex `ModelInfo` 生成，不使用随意 JSON。
2. 原始目录中的 `supported_reasoning_levels[].effort` 与 RPC 的 `supportedReasoningEfforts[].reasoningEffort` 分别序列化。
3. 本机 RPC `Model` 没有通用 provider 路由字段；向 `model/list` 塞一个供应商名称不能替代真实请求路由。
4. `model/list` 支持分页；验证必须遍历 `nextCursor`，不能只看第一页。
5. 只有当前受管实例回执或 UI 观察才能证明 Desktop 已加载；另起一个 CLI 查询成功不证明现有窗口刷新。
6. `ConfigBatchWriteParams.reloadUserConfig` 明确排除部分会话静态默认项，不能借它承诺模型、推理强度即时切到所有任务。
7. 生成第三方目录前检查是否替换默认目录；如目标版本为替换语义，只写第三方模式所需集合，不能假定自动追加官方模型。

## 6. 模态与文件支持矩阵

| 用户可见能力 | 上游实现可能性 | Codex 接入处理 | 首发策略 |
| --- | --- | --- | --- |
| 文本 | 对话输入 | text | 必须验证 |
| 图片 | image URL / base64 / 文件引用 | image + 流程实测 | 端点验证后启用 |
| 音频 | 原生音频输入或转写 | 本机 schema 存在 audio，但不等于当前模型 UI 已开放 | 元数据可记录，入口/协议验证后开放 |
| PDF 原生 | file ID / inline file，服务商各异 | 无通用 pdf 枚举 | 默认“不支持当前链路” |
| PDF 转文本 / 页面图 | 本地解析或渲染 | text/image，必须告知转换 | P1，宿主必须确实提供文件入口 |
| 视频原生 | 厂商特定格式 | 无通用 video 枚举 | 默认禁用；不能用 image 冒充 video |
| 视频抽帧 | 有损转换、可能丢音轨 | 需要文件入口和独立转换组件 | 后续独立能力，不在首发承诺 |
| 代码、Markdown、TXT | 工具读取或文本附件 | 进入文本上下文 | 显示“由 Codex 文件工具读取” |
| DOCX / XLSX / ZIP | 需要工具或解析器 | 不是模型模态布尔开关 | 记录工具依赖，不承诺原生读取 |

不支持的输入在发送前明确阻止或要求用户选择已声明转换方式；不能静默去掉图片/文件继续请求。

## 7. 思考模式和长度

思考能力数据同时包含支持状态、控制类型（档位 / 开关 / Token 预算 / 无）、合法选项、默认选项和协议映射。`auto` 是本工具“不指定参数”的 UI 选项，除非上游明确支持，不能发字符串 `auto`。`none` 只表示上游明确的禁用值，不等于省略参数。

本次上游枚举包含多个预置档位并允许自定义字符串；这不意味着每个模型都支持它们。旧 Desktop 可能只认识有限档位，必须按版本限制菜单，不能将不支持的 max 偷偷映射成 high。

长度分别管理：模型声明的上下文、用户运行上限、输出硬上限、用户请求输出上限、推理预算、压缩阈值。缺失值保存为 null，不填 0、不从名称猜。不能将用户声明的 1M 当作已通过 1M 上下文测试。

## 8. 必须先完成的实验

G0 在两平台独立回答：目录能否加载；菜单能否选中 namespaced alias；请求体是否保留 alias；新旧任务如何选 provider；目录变更需重启哪个进程；多层配置怎样覆盖；原生恢复是否保留历史与登录；当前版本是否接受 auth helper。具体步骤与证据要求见测试文档。

没有运行这些实验前，状态只能是“方案可行、有源码依据、Desktop 集成待验证”。
