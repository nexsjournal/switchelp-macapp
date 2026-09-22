# 网关、协议与模型能力执行

## 1. 路由原则

一个稳定 alias 对应 `(providerId, modelId)`，实际请求上游 ID 单独保存。示例：目录 `gs/p_a/m_1`，显示“自定义服务商 A · Coding Model”，上游仍精确使用 `vendor/model-x`。不按 `gpt-*` 字符串认定官方，也不按供应商展示名分流。

请求开始固定 `routeRevision + credentialVersion + protocolVersion`。供应商改名不变 alias；模型上游身份改变默认创建新路由版本，能力变化需要宿主重载的版本不能提前服务于旧目录实例。

## 2. 接口表面

下表路径相对于 `/i/{instanceId}/c/{catalogRevision}` 前缀。前缀确定目录版本，认证令牌限定实例，模型 alias 只能在该版本允许的集合中解析。拒绝未知前缀和 alias；不同目录版本不能互相回落。

| 接口 | 行为 |
| --- | --- |
| `POST /v1/responses` | 主路径；认证、校验、路由、响应/流式转发 |
| `GET /v1/models` | 返回该实例已发布模型，不触发上游扫描 |
| `POST /v1/responses/compact` | 只在适配器明确支持时代理；否则结构化 unsupported，绝不伪造成功摘要 |
| WebSocket Responses | 独立协商能力；首版基线为 HTTP SSE，不能声明已支持 WebSocket |
| 内部健康检查 | 通过 IPC；若需要 HTTP，仅返回最小就绪信号且需认证 |

默认只绑定 `127.0.0.1`。不监听 `0.0.0.0`，不增加“远程控制”端口。桌面管理使用 IPC，不共享 inference token 的权限。

## 3. 协议适配顺序

### Responses 原生

尽量保留事件、工具类型、response ID、usage 和错误语义；只在明确管理的字段注入 alias 映射、认证和用户策略。实际“兼容”端点仍需测试：不能因 URL 有 `/v1` 就认定兼容。

### Chat Completions

将 Codex 的 Responses 输入转换为 messages；工具声明映射 function tools；将流中 tool_call id、名称、分段 arguments 累积为正确的 Responses 事件。工具执行仍由 Codex 完成，网关不执行 shell。

重要兼容表：

| 项 | 处理要求 |
| --- | --- |
| system / developer 指令 | 按适配器能力保留；降级合并必须标记 degraded |
| developer 角色（chat 上游） | chat 没有这个角色：`input` 里的 `developer` 消息与顶层 `instructions` 合成**一条**开头的 system 消息。原样转发会被只认 system/user/assistant/tool 的上游 400（moonshot 实测 `role 'developer' is not allowed`，宿主侧只看到 `error.upstreamRejected`）；拆成两条 system 又违反「system 必须在首位」。合成本身不记损失，只有该消息原本排在对话之后、位置被提前时才记 `input.developer` |
| 多轮 tool call 与 output | 保持 call_id 对应、顺序和角色，不拼成普通聊天文字 |
| parallel tool calls | 仅在全链路通过测试时声明；否则拒绝或使用预先公开策略 |
| custom / freeform 工具 | 不是普通 function；有专用映射才允许，尤其 apply_patch |
| 上游内置工具（`web_search` 等） | 由上游执行，网关既不转译也不替代：该模型未声明支持时**摘掉并记损失**。原样转发会被没实现它的网关整条拒掉（实测小米 MiMo：`responses_feature_not_supported: tool type 'web_search' is not supported`），用户看到的却是一条与自己的操作无关的 400。宿主自己调用的 function / custom 工具不受影响 |
| reasoning 内容 | 与可见输出分开；不将供应商隐藏字段当普通文本输出 |
| usage | 可缺失，用 null；不伪造 0 Token 或收费金额 |
| 流结束 | 正常完成、输出上限截断、取消、异常分别映射 |
| 非标准事件 | 透传安全可理解字段或返回 unsupported，不能静默吞掉关键语义 |

Anthropic / Gemini 等使用同样的纯转换接口，但每家单独实现和验收，不建立一个无限增长的“万能 OpenAI 兼容开关”。

## 4. SSE 与取消

实现按字节解析 SSE，不假定一个网络 chunk 等于一个事件，也不假定 UTF-8 字符完整落在同一块。处理多行 data、空事件、keepalive、CRLF、截断和背压。

流式生命周期为 `admitted → sent → headersAccepted → streaming → completed / incomplete / failed / cancelled`。向下游提交状态头也是可观察承诺；上游已经接受但还没首 Token，不代表可随意重试。

客户端取消立即传播 Abort；停止读上游、释放连接与路由引用。只能取消对应 requestId，不能停止整个网关。超时分连接、首事件、流空闲、总时长；推理模型允许更长首事件预算，但不是无限等待。

初始建议：连接 10 s、首事件 90 s、流空闲 180 s；每供应商可覆盖，UI 显示实际值。请求体初始保护上限 32 MiB，长文本/图像需区分字符数与解码后尺寸；这只是运输保护，不代表 Token 上限。

## 5. 输出长度与上下文约束

输出设置包含：`publishedMaxOutput`、`userMaxOutput`、`requestMaxOutput`、`reasoningBudget`，全部以 Token 为单位。用户界面可输入 `32k`，必须明确解析为 32000，并展示精确值；`Ki` 不与 k 混用。

请求约束规则：

1. 对已知正整数候选取最小输出上限；全部未知时不填参数，显示“由上游决定”。
2. Responses 写 `max_output_tokens`；Chat 根据具体端点契约选 `max_completion_tokens` 或 `max_tokens`；二者不能无脑同时发。
3. 推理 Token 是否计入输出由端点定义；预算/总输出冲突则阻止发送，不自动调整而不通知。
4. 上下文联合限制用 `inputTokens + reservedOutput + overhead <= effectiveContext` 做预检；tokenizer 缺失时是估计，显示估算来源和裕度，仍以上游为最终约束。
5. 若未知上下文，允许保存草稿；发布到需要明确窗口的目录前要求用户填写声明值或选择明示的保守模板。模板标“用户策略值”，不标“模型真实上限”。
6. 压缩建议阈值可取 `min(floor(C×0.8), C - O - S)`，其中 C 为已确定有效窗口、O 为输出预留、S 为工具/系统裕度；这是本项目建议，不是 Codex 保证。结果非正数阻止应用。
7. 目录输出字段不替代请求执行；每次诊断记录 requested / effective output cap 与 adapter 字段名称。

不靠剪断返回字符串强制 Token 限制；该方式会损坏 JSON、工具参数和计费理解。上游忽略参数时显示“约束未获上游保证”，测试通过前不标硬限制已验证。

## 6. 推理与模态策略

推理枚举按模型来源获取，手工覆盖需保留来源及验证状态。禁止用温度模拟思考强度，禁止将摘要字段当作能力开关。供应商只支持 on/off 或 budget 时，本工具可编辑，但原生选择器映射必须经过兼容验证，否则只提供网关固定策略并显示“Codex 中不可切换”。

输入为 text / image / audio 的宿主目录投影，与 pdf / video / document 等业务分类分离。转换必须显式选择、可追踪，并显示数据会发往哪里。首发不添加另一个视觉模型作为文本模型的隐式补充。

## 7. Key 池与错误分类

P0 仅固定 Key。P1 引入的 failover 必须满足同供应商、同端点权限、请求尚未提交且不存在续接绑定；默认总尝试次数最多 2，指数退避加 jitter，遵守 Retry-After。

| 错误 | 健康标记 | 自动策略 |
| --- | --- | --- |
| 401 | 该凭据认证失败 | 新请求可用其他已验证 Key；当前已提交请求不重放 |
| 403 | 权限/地区/模型范围待判定 | 不一律删除或标全 Key 失效 |
| 404 | 模型或路径错误 | 修正路径/ID，不轮换所有 Key |
| 400 / 422 | 参数或协议不兼容 | 精确指出字段，不作为网络失败重试 |
| 429 | 限流或额度不足，按结构化错误区分 | 临时 cooldown；不给出未经证实的余额 |
| 5xx / DNS / TLS | 端点或运输异常 | 不标 Key 永久失效，不降低 TLS 校验 |
| 200 HTML / 登录页 | 非协议响应 | 测试失败，提示 URL/代理 |
| 已开始 SSE 后断流 | 流失败 | 终止并返回错误，不换 Key 重播 |

禁止跨供应商自动 fallback；若后续提供，必须由用户明确建立模型路由组与数据发送边界，并解决能力和上下文不等价问题。

## 8. 续接与会话

Responses 的 `previous_response_id` 可能属于供应商/账号/模型的服务端状态；opaque reasoning/encrypted items 也可能不可跨路由复用。保存 `(responseId, routeRevision, credentialVersion, expiry)` 最小绑定，不存请求正文。

更换 Key 后，普通无状态完整上下文请求可使用新 Key；带绑定的续接继续旧 Key。若旧 Key 已撤销，返回需要新任务/完整上下文重建的明确错误，不能把未知响应 ID 发给新服务商。Chat 模拟续接仅在有明确、受控的上下文缓存设计后实现；首发不默认长期保存消息。

## 9. 分层测试与连接状态

测试步骤：URL/TLS → 认证 → 模型列表（可不支持）→ 最小文本 → SSE → function call → tool result continuation → 用户选中的图片/推理/长度检查。只测文本不标“Codex 完整兼容”。

测试发送合成短提示，不发送用户代码。点击前提示可能产生少量调用费用；批量测试显示数量和取消入口，不自动遍历全部模型。延迟区分网络连接、首 Token 与总时长；显示时间、模型、Key 标签、协议、revision 和测试项目，默认 10 分钟后标结果可能过期。
