# Jev / Laya 中文社区 + 国际社区实操调研

- **日期**：2026-09-30
- **范围**：Jev（TypeSafe AI，2026-09-15 发布的首个 System One 决策模型）、Laya（Convai Innovations，Apache-2.0 开源非自回归决策模型）、以及 open-jev / kev / von / SemIf / Nimble / Kev-4B / diffusiongemma 等复刻家族。
- **关注点**：别人**具体怎么用**、**踩了什么坑**、**怎么免费跑**，以及能否落到 Switchelp（Tauri 2 + React + Rust 桌面端，管理 Codex 供应商/Key，本机网关转发）。
- **档位说明**：【官方】官方文档/公告；【仓库/代码】可读源码或 README/权重元数据；【社区实测】有具体命令/数字的个人实测（可能不可复现）；【第三方文章】媒体或教程文章（可能含 AI 生成内容）；【推测】本报告作者的推断。
- **可信度分级**：本文区分「**社区传闻**」（无数据、只有观点）与「**可复现实测**」（给了硬件、命令、样本量与数字）。

---

## ① 一句话结论

Jev 在中文圈已是现象级话题（B站单条教程 17 万播放、掘金同题 20+ 篇、CSDN 30+ 篇），但社区共识是**它不是一个"更小的 LLM"，而是一个"带概率的智能 if"**——真正落地的场景集中在分类/路由/评分/护栏四类，而 Laya + GGUF 量化（Q4 约 272–524 MB）让"免费本地跑"在 4GB 内存/纯 CPU 机器上已经成立【社区实测】；代价是 Laya 在「选项多 + 中文推理」场景明显掉点（Banking77 77 类意图仅 42.5%，14 条 SQL 逻辑判断只对 6 条），因此社区主流做法是 **Laya/local 兜底 + Jev 兜高价路径 + 低置信度转人工**的混合路由。

---

## ② 中文社区

### 2.1 B站（信息密度最高，踩坑内容最真实）

B站搜索走 `api.bilibili.com/x/web-interface/wbi/search/type`（无需登录可取标题/UP主/播放/发布日期/简介）【仓库/代码：本机实测接口可读】。

| 标题 | UP主 | 日期 | 播放 | 链接 | 要点 | 可信度 |
| --- | --- | --- | --- | --- | --- | --- |
| 全网刷屏的 Jev 模型正式开放！保姆级教程 + 实战测评 | 程序员鱼皮 | 2026-09-22 | 17.1万 | https://www.bilibili.com/video/BV1wDhj6wEa8 | 从零到接入 Codex，并与 DeepSeek V4 Flash 比速度；中文圈最大流量的一条 | 【社区实测】 |
| 【闪客】这些 Jev 的案例都是骗人的！揭秘 Jev 玩我的世界效果骗局 | 飞天闪客 | 2026-09-23 | 14.1万 | https://www.bilibili.com/video/BV1ZLht69E7b | **反方代表**：指大量"Jev 玩 MC"演示是剪辑造假/人工接管 | 【第三方文章（视频）】 |
| 刷屏的 Jev，一个只做判断的 AI，到底有什么用？ | 神烦老狗 | 2026-09-21 | 10.5万 | https://www.bilibili.com/video/BV14EhB6YEex | 定位讲解，无实测数字 | 【第三方文章】 |
| JEV 模型终于不用排队了！0.4B 仅 2.37GB，完全兼容官方 API/SDK，免费注册直接拿 Key | 鲲鹏Talk | 2026-09-19 | 2.2万 | https://www.bilibili.com/video/BV1CZe86yEst | **免费路子**：第三方把 0.4B Jev 兼容模型挂到 "Omni Labs"，注册即给免费 Key，支持官方 SDK | 【社区实测】 |
| 0.4B 决策模型 Laya，Github 一周 2 万 Star，一键部署 | 小北AI开源 | 2026-09-24 | 1.7万 | https://www.bilibili.com/video/BV1dJaF63EPH | 给出 `NandhaKishorM/laya` + `1Panel-dev/laya-server` Docker 版 | 【社区实测】 |
| 开源决策模型 Laya 实测：14 道 SQL 只对 6 道，微调 3.9 小时后能追平 Jev 吗 | WarlockTome | 2026-09-23 | 6252 | https://www.bilibili.com/video/BV1Pfhf6dEkN | **关键踩坑**：14 条 PostgreSQL 逻辑判断 Jev 13/14、Laya 6/14；20 张任务卡片选档位 Jev 17/20、Laya 最好 9/20 | 【社区实测】 |
| Jev AI 完整教程：三种决策模式详解，接入 Codex 完整流程 | 杰森的效率工坊 | 2026-09-22 | 5652 | https://www.bilibili.com/video/BV1E6hJ6iEb3 | 邮件分类/情侣聊天案例 + Choice/Score/Noul 三型 | 【社区实测】 |
| 10分钟讲清楚火爆全网的JEV | 清华姜学长 | 2026-09-24 | 5644 | https://www.bilibili.com/video/BV1yoaK6gE34 | 概念向 | 【第三方文章】 |
| 开源决策引擎 Laya 实测：能平替 Jev 吗？ | 五里墩茶社 | 2026-09-25 | 3579 | https://www.bilibili.com/video/BV1X2ag6YEgZ | 用与上一期 Jev 相同的 662 次测试样本对比 Laya（视频含聚合平台 DMXAPI 广告） | 【社区实测】 |
| 开源决策模型 Laya（Jev开源版）架构与训练详解 | 偷星九月333 | 2026-09-26 | 3845 | https://www.bilibili.com/video/BV1LshX6dE1x | 拆 Bert 编码器 + 选项决策头 + RLCD 概率训练；代码放在"私有仓库"，需充电后在评论区留 GitHub 账号被邀请 | 【第三方文章】 |
| JEV快速决策模型 本地部署各大开源方案实测 System One | 麻仓月轩 | 2026-09-29 | 461 | https://www.bilibili.com/video/BV17uaX6fESz | 7 个 System One 式模型抢答 749 道真实判断题（飞书消息分诊/扫地机指令/保险条款），逐题给准确率与延迟 | 【社区实测】 |
| Jev 太慢了！本地部署 Laya，45ms 决策还 ¥0 | AIStarter | 2026-09-24 | 3178 | https://www.bilibili.com/video/BV17baK68Eay | 宣称 Laya 45ms、零成本；提供百度网盘 demo 与 panelai.cn 一键整合包（第三方打包，来源需谨慎） | 【社区实测（未独立验证）】 |
| 比Jev快10倍！Laya 本地决策 AI 引擎本地部署教程 | X超哥来了 | 2026-09-23 | 2271 | https://www.bilibili.com/video/BV1c7h86wELk | 部署教程，资料在第三方站点 xgdn.com | 【第三方文章】 |
| SiliconFlow × Jev：三款开源平替 PK 竞技场！ | 沧海九粟 | 2026-09-28 | 1522 | https://www.bilibili.com/video/BV1ZAaq6mE4e | **国内聚合平台落地**：在**硅基流动**上体验 Kev、SemIf，用 LangChain 搭双模型 Arena 观察"伪授权"下的放行/拦截差异 | 【社区实测】 |
| 告别模型选择困难！用 Jev 决策模型为 Codex 实现全自动路由实测 | 胡昊聊AI | 2026-09-20 | 1297 | https://www.bilibili.com/video/BV1pCeY6QEQT | **与 Switchelp 场景最接近**：按任务复杂度在 GPT-5.6 Luna / Sol / GPT-6 Astra 间自动路由 | 【社区实测】 |
| 这里完全免费提供Jev模型API调用，你来！ | KnoxCore | 2026-09-22 | 1431 | https://www.bilibili.com/video/BV1zWhC6eEFV | 宣称完全免费提供 Jev API（未在视频简介给出域名，**未能核实**） | 【社区传闻】 |
| Jev 全面开放！1.2 亿 Token 免费领｜这不是诈骗 | JayCode | 2026-09-21 | 1415 | https://www.bilibili.com/video/BV1tph66BEK4 | 宣称 TypeSafe 放开排队并发免费 Token（**未能核实**） | 【社区传闻】 |
| Jev模型保姆级使用教程（含免费方式） | 是子鱼AI | 2026-09-25 | 685 | https://www.bilibili.com/video/BV1FSh265EVt | 配置进 Codex + "Jev 搭配 GPT-6 干活" | 【社区实测】 |
| 基于JEV的PC微信助手 正式开源 | 你们喜爱的老王 | 2026-09-24 | 2565 | https://www.bilibili.com/video/BV1GiaK6REgJ | 指向 `ops120/wechat-triage-hud` | 【社区实测】 |
| 当你把 JEV 接入微信 | 无牙仔仔看世界 | 2026-09-21 | 1.44万 | https://www.bilibili.com/video/BV1ahhz65E2L | 微信接入演示（**隐私风险高**，见 ④） | 【社区实测】 |
| 3分钟搞定 Jev：安装配置 + Codex 调用 | 花萍雨呀 | 2026-09-22 | 756 | https://www.bilibili.com/video/BV1c9hj6FEte | 创建 API Key → 装 TypeSafe Skill → Codex 首次调用 | 【社区实测】 |
| Jev决策模型适不适合炒股？ | 长寂无光 | 2026-09-20 | 515 | https://www.bilibili.com/video/BV1uVee6wEWG | 质疑把决策模型用于涨跌预测 | 【第三方文章】 |
| Hermes v0.21.5！Jev的开源替代方案来了！ | 愈解忧 | 2026-09-26 | 2777 | https://www.bilibili.com/video/BV12UhX6uEu4 | 客户端接入开源 Laya 做本地替代 | 【社区实测】 |
| TypeSafe AI 发布了全新模型 Jev：放弃"聊天"，只做"决策" | （搬运） | 2026-09-16 | 4914 | https://www.bilibili.com/video/BV1VWec6LE97 | 搬运创始人 X 视频 | 【第三方文章】 |
| 7分钟看懂Jev概率模型 \| TypeSafe AI的RLCD新范式 | （搬运） | 2026-09-21 | 1694 | https://www.bilibili.com/video/BV17Nhk6ZE3T | 译自 YouTube `vj7hysh0mOI` | 【第三方文章】 |

**B站生态观察**：搬运自 YouTube 的"中配"视频已成规模（RUNTIME.、Sam Witteveen、GitButler 的内容都有中文配音版），说明中文圈对 Jev 的一手信息**高度依赖国际社区翻译**；同时"一键整合包 / 微信/QQ 群 / 知识星球"式的分发（含百度网盘链接）是这个赛道的主要获客方式【推测】。

### 2.2 知乎（正文被反爬拦截，仅有标题级证据）

**⚠️ 未能访问**：`zhuanlan.zhihu.com` 对 curl 与 WebFetch 均返回 403 / 验证码；`www.zhihu.com/api/v4/search_v3` 返回 400。以下条目**只有搜索结果标题与摘要**，正文细节**未核实**。

| 标题 | 链接 | 要点（来自搜索摘要） | 可信度 |
| --- | --- | --- | --- |
| Laya 开源：比Jev快4倍！421M 参数，33 毫秒完成 System 1 决策 | https://zhuanlan.zhihu.com/p/2085760548034167437 | 摘要称"Convai Innovations 开源了非自回归 System 1 决策模型 Laya……开源不到 2 天升至 HuggingFace 热门模型"；发布时间 2026-09-22 | 【第三方文章，正文未核实】 |

### 2.3 掘金（20 篇同题文章，含最细的中文实测）

掘金搜索 API（`api.juejin.cn/search_api/v1/search`）与文章页 curl 均可读【仓库/代码：本机实测接口可读】。

| 标题 | 作者 | 链接 | 要点 | 可信度 |
| --- | --- | --- | --- | --- |
| 实测三个 Jev 决策模型(均开源)：Kev-4B、SemIf、diffusiongemma | — | https://juejin.cn/post/7688895674230833179 | **国内聚合平台关键证据**：API 端点为 `https://api.siliconflow.cn/v1/systemone`。AUC(Noul)：Kev-4B 0.944 / SemIf 0.888 / diffusiongemma 0.836；ACC@0.5：84.6% / 82.1% / 82.1%；最优阈值下 87.2% / 84.6% / 82.1%；平均延迟 306ms / 326ms / 692ms。**踩坑**：走 `/v1/chat/completions` 返回 400 `Model does not exist`，**只能走 `/v1/systemone`**；diffusiongemma 严重未校准（39 条输出里 37 条是精确 0.0/1.0） | 【社区实测】 |
| 给 Codex 配上 Jev，直接起飞。 | 沉默王二 | https://juejin.cn/post/7688401277182066738 | 完整可复制流程：`claude plugin marketplace add typesafe-ai/skills` 或 `npx skills add typesafe-ai/skills --skill typesafe-ai`；注册官网约 2 分钟拿到 Key，`export TYPESAFE_API_KEY=`；给了置信度阈值范式（≥0.8 执行、<0.8 转 human_review、"Jev 调用失败就停止、不允许 Codex 自行补充判断"）；返回示例 `model: jev-1.13.0` | 【社区实测】 |
| 别吹 Jev 了 | stormzhangV | https://juejin.cn/post/7687793891199418374 | **反方代表**（96 赞）："本质就是 JSON 分类器套了层前沿模型的皮"；能力与 DeepSeek Flash 打平；"不会幻觉"是重新定义的（OpenAI 结构化输出/Anthropic tool use 早就有）；**批量处理文档实测：每千份 $0.22，DeepSeek Flash $1.31**；"护城河 48 小时就塌了，推特老哥两小时拿 Qwen 搓出同款" | 【社区实测 + 观点】 |
| 发布 3 天登顶 HN：不生成一个字的模型 Jev，我把它的源码和黑料都扒了一遍 | — | https://juejin.cn/post/7686669083098775562 | 官方 193.6×/444.6× 数字**是自评**、无独立复现；AI SDK 7.0.105+ 的 `experimental_evaluate` 走 `${baseURL}/evaluation-model`，头带 `ai-evaluation-model-specification-version: 4`；原生 API 单端点 `POST /v1/systemone`，SDK 默认别名 `jev-latest`；**扑克实测 150 个决策点只 63% 与求解器最优一致，且最错的答案置信度最高 0.86**；Doom 演示约 **$7/小时**；13 个问题合并成 1 次请求比 13 次分开便宜 11.5×/快 9.6×；`state`+`questions` 共用约 32K token 预算；Choice 上限 255 | 【第三方文章 + 引用官方/社区】 |
| 给 Agent 加一个"判断器"：聊聊 Laya、Jev，以及怎么部署和选择 | 杨杨杨大侠 | https://juejin.cn/post/7688528701483515945 | 中文圈**最严谨**的 Laya 说明：checkpoint 对照表——英文版 ModernBERT-large/约 421M/512 tokens；多语言版 mmBERT-base/约 322M/1024 tokens；typed-decisions ModernBERT-large/约 421M/1024 tokens（多语言版问题预算 256 tokens，材料约剩 768）。明确提醒"**别把'不会生成候选项以外的标签'理解成'不会判断错'**"。作者自注"未实际下载权重、未运行推理" | 【第三方文章，作者已自陈未实测】 |
| Jev 火了两周，开源生态已经长出 28 个项目 | Hey_AI_Coder | https://juejin.cn/post/7688569614586789914 | 28 个项目的横向数字（见 ③ 国际社区的同一批模型）：SemIf 3090 上 21 个二元判断 1.023s（同模型生成同等 JSON 5.332s），一致率约 84.5% vs Jev 公布 88.3%；Laya T4 上单问题 p50 约 32.8ms、10 问题打包 72.3ms，但 **Banking77（77 类意图）只有 42.5%，Jev 87.0%**；Verdict 约 150M，2000 条留出决策上 77.10% / Brier 0.0636 / 置信度头 ECE 0.0144 / 选项顺序翻转率 4.76%；Kev-9B 83.7% vs Jev 85.7%；Nimble（Qwen3.5-9B + LoRA）90.12% vs Jev 93.21% | 【第三方文章，转述源文章数字】 |
| Jev 使用完整指南：从申请 API Key 到置信度路由 | — | https://juejin.cn/post/7688009718430662666（另有 7687555729299324943） | 从申请 Key 到把置信度阈值写进代码的完整流程 | 【第三方文章】 |
| 不做更强的 LLM，而是更快的 if：聊聊 Jev 决策模型的选型四原则 | — | https://juejin.cn/post/7687804226240069670 | 选型框架：把 Jev 用在"审"而非"写" | 【第三方文章】 |
| Jev是什么AI模型？不做自然语言生成为何引发热议 | 孟健 | https://juejin.cn/post/7687583607784636426 | 作者把 Jev 接入自家 ShipSite，**只承担流程中的辅助判断**，不替换主 LLM | 【社区实测】 |
| 外网 3500 万人围观的 Jev 模型，有这 10 个神奇玩法 | — | https://juejin.cn/post/7687505412875812864 | 10 个玩法汇编 | 【第三方文章】 |
| 基于Jev的浏览器Agent插件狂揽 21k star | — | https://juejin.cn/post/7690762523469479977 | 讲 `browser-use/jev-ultrafast` | 【第三方文章】 |
| Jev 入门第一课 / Jev模型深度解读 / 深度解析 Jev 模型 / Jev：不是聊天机器人，而是一个智能 if 语句 / Jev是什么？哑巴模型居然全网爆火 / Jev到底是个什么东西 / 为什么最近开始关注 JEV / AI 圈爆火的 Jev 是什么？如何在 TraeCode 中使用 | 多位 | 7686925590315696138 / 7687446787749707782 / 7687164781725007906 / 7688159506153095209 / 7687630544474701870 / 7687899083843010587 / 7686808742222856211 / 7688018731444584457 | 同质化极高的科普/导流文，**信息价值低**，但反映中文圈传播强度 | 【第三方文章】 |

### 2.4 CSDN（30 篇同题，但**质量风险高**）

CSDN 搜索 API（`so.csdn.net/api/v3/search`）返回 30 条【仓库/代码：本机实测接口可读】。典型条目：

- `Jev 模型深度解析：不生成文本的 System One 决策模型（原理/性能/工程落地）` — https://blog.csdn.net/强化学习/article/details/166579956
- `Jev 深度技术分析：一种专为 AI Agent 设计的决策模型` — https://blog.csdn.net/强化学习/article/details/166492173
- `用 Jev 决策模型玩贪吃蛇，我试了一下` — https://blog.csdn.net/强化学习/article/details/166238746
- `Jev 接入 Codex 指南：TypeSafe 决策模型与 model_provider 实践` — https://blog.csdn.net/weixin_29062255/article/details/166762980
- `决策模型 Jev，TaoToken 只做 Key 分发` — https://blog.csdn.net/密钥管理/article/details/165862358 （重复请求返回 HTTP 521，正文未取到）

**⚠️ 重要负面发现**：上表 `Jev 接入 Codex 指南` 一文自称讲 TypeSafe 的 Jev，但正文把 Jev 描述成"**推理调度引擎**"、出现 `jev-max` / `jev-mini` 这类**不存在的模型名**，并把 `ccswitch`（第三方 Codex 供应商切换工具）说成 Jev 生态组件。这与 TypeSafe 官方定义（**决策模型，不做模型调度**）直接冲突【官方 vs 第三方文章】。**结论：CSDN 上这批 Jev 文章大量是 SEO/LLM 生成内容，不可作为技术依据。**【推测，但有具体矛盾证据】

### 2.5 微信公众号（标题可得、正文多已失效）

通过搜狗微信搜索（`weixin.sogou.com`）取到标题与摘要【仓库/代码：本机实测接口可读】；但**多数正文链接已 404/被删除**（实测一条返回"该内容已被发布者删除"）。

| 标题 | 日期 | 要点（摘要） | 可信度 |
| --- | --- | --- | --- |
| 一口气看完 5721 个 Jev 案例：6 类场景值得落地，5 个坑先别踩 | 2026-09-22 | 摘要：**发布一周被开发者做出 5721 个作品**；有人拿它搭全自动交易机器人**亏了 3 万多美元**；Vercel 用 Jev 审查 Agent 要执行的每条命令，**p95 延迟 6.6 秒 → 0.4 秒**；日本一家法令检索服务已进生产，"每个问题成本从 20 日元……"（截断） | 【第三方文章；正文未取到】 |
| Laya 轻量决策模型部署指南：硬件配置、Agent 路由与应用场景 | — | 摘要：给一封邮件/工单/JSON 直接判断部门、是否退款、风险等级、是否转人工、是否调用某工具，且**一次前向推理同时完成** | 【第三方文章；正文未取到】 |
| TypeSafe AI 实时决策模型 Jev 语音智能体案例分享 | — | 日报类 | 【第三方文章】 |
| Qwen-Image-2.1 开源，TypeSafe Jev 决策模型火爆｜AI开发者日报·0921 | — | 提到"生态落地争议：**彻底放弃传统自回归生成、专职路径决策与状态仲裁的 Jev 在用户群体掀起狂潮**" | 【第三方文章】 |
| Laya 开源决策模型，比 TypeSafe Jev 快 7.8 倍 | — | 摘要含关键商业判断："Apache 2.0 开源协议，**可惜只能自托管，没有商用 API，不太适合面向多用户调用**" | 【第三方文章】 |
| 让 AI 只做判断题：TypeSafe Jev 决策模型（附申请方式与实测成本） | — | 描述 Jev 为"约等于一个部署在全球的**智能 if 语句**" | 【第三方文章】 |
| 不聊天、只做选择题：TypeSafe Jev 决策模型从入门到落地（机制、调用、申请、案例全讲透） | — | 入门向 | 【第三方文章】 |
| Jev 不说话：TypeSafe 的 System One，是给软件用的决策模型 | — | 提到"类名借自卡尼曼的 System 1" | 【第三方文章】 |
| TypeSafe AI 推出"决策专用"模型 Jev | — | "比 ChatGPT 快 10 倍、便宜 10 倍"（与官方 193×/444× 口径不一致，**疑似夸大或旧稿**） | 【社区传闻】 |

### 2.6 博客园 / 其它中文站点

- **博客园：未找到**。`zzk.cnblogs.com/s?w=Jev` 返回 0 篇且要求人机验证；搜索结果里出现的 "Laya 性能优化/统计面板" 均为 **LayaAir 游戏引擎**（同名不同物），**不要混淆**。
- **极道 jdon.com**（中文译文站，多篇）：`Jev决策引擎解析：单次前向传播替代自回归解码，推理70毫秒`、`非自回归决策模型对比：Jev 和 Laya 在工单分类场景下谁更快`、`开源 Nimble：一个号称"开放式 Jev"的决策模型`（Bespoke Labs，9B 本地类型化决策）【第三方文章】
- **一聚教程网 111cn.net**：`Jev 入门教程：结构化决策、模型校准与 Python 实战`（讲 Choice/Score/Noul、Python SDK、confidence 校准、"零类型幻觉"与生产边界）【第三方文章】
- **typesafe-jev.com**：独立社区维护的 Jev 中文指南（"能力边界、API 用法、应用场景与开源生态"）【第三方文章】
- **whatisjev.com/zh**、**jevai.dev/zh-Hant**：中文/繁中入门站与 Playground【第三方文章】
- **台湾繁中圈**（内容量意外地大，几乎全是 SEO 站点，信息同质）：bnext.com.tw/article/92319、klab.tw、aiposthub.com、grenade.tw、ai.com.tw、techhanlin.tw、yololab.net【第三方文章】

### 2.7 中文 GitHub 仓库（与 Switchelp 场景最近的一批）

| 仓库 | Star | 语言/许可 | 要点 | 可信度 |
| --- | --- | --- | --- | --- |
| `jev-chat/jev-chat-jarvis` | 7155 | Kotlin / MIT | 手机端对话副驾：**只读屏幕**（不 hook 不改包），在 QQ/X/飞书里给候选回复，发送永远手动 | 【仓库/代码】 |
| `jev-chat/jev-chat-windows` | 698 | Python | 窗口截图 + **本地离线 OCR** → Jev 判断意图 → 3 条候选填入 | 【仓库/代码】 |
| `jev-chat/jev-chat-jarvis-mac` | 446 | Python | macOS 悬浮窗版本，屏幕感知 + 本地小模型判断风险 | 【仓库/代码】 |
| `Devine-AXIS/jev-dsh-decision` | 335 | JavaScript | **面向 Agent Harness 的决策插件**：DeepSeek Harness 原生插件；通过 iPolloWork 支持 OpenCode / Codex Harness；明确声明"**不执行推荐动作、不自动切换模型或路由所有请求**"、"**Jev 不返回文字推理过程，Agent 的解释不能冒充 Jev 的原始输出**" | 【仓库/代码】 |
| `FerryCorleone/crush-monitor` | 262 | TypeScript / MIT | "Crush 好感监控器"，微信聊天情绪/意图分析，本机运行自带 Key | 【仓库/代码】 |
| `1Panel-dev/laya-server` | 83 | TypeScript / Apache-2.0 | **国内团队（1Panel）做的 Laya 自托管服务**，兼容 TypeSafe `/v1/systemone` 线协议，`docker run` 一条命令起服务，自带 Web 控制台与 API Key 管理 | 【仓库/代码】 |
| `shengjidaguai-china/goutoujunshi-jev-chat` | 214 | Python | Mac 微信读屏 + 关系分析 + 回复草稿悬浮窗 | 【仓库/代码】 |
| `Liyucheng1997/332_lab-jev-chat` | 161 | Kotlin | 电脑版微信意图判断 + DeepSeek 建议回复 | 【仓库/代码】 |
| `yzyialy/crush-monitor` | 2 | TypeScript | "**数字全部由程序计算，数据只存在本机**"——本地化改良版 | 【仓库/代码】 |
| `ops120/wechat-triage-hud` | — | — | B站"基于JEV的PC微信助手"所指仓库 | 【仓库/代码】 |
| `KKiJJ1024/crush-monitor2` | 1 | — | 自述"**免去了 jev 模型钥匙的获取，直接接入 deepseek**，效果会有差异"——中文圈规避 Key/付费的典型做法 | 【仓库/代码】 |
| `huoyun0427/FerryCorleone-crush-monitor`、`RYANFFY/crush-monitor-pack` | 2 / 2 | — | 第三方 fork 与第三方打包安装包（**来源可信度低，谨慎下载**） | 【仓库/代码】 |
| `yunhai-dev/laya2typesafeapi` | 0 | Python | 把自托管 Laya 包成 **TypeSafe 兼容 API**（自建网关思路的最小实现） | 【仓库/代码】 |

**crush-monitor README 里的免费额度表**（这是中文圈最实用的"省钱"证据，核对日期 2026-09-22）【仓库/代码】：

| 平台 | 免费额度（原文口径） | 备注 |
| --- | --- | --- |
| TypeSafe 官方 | "此前新用户有 **$5 试用额度**，后续是否赠送及金额以控制台为准" | 控制台 https://console.typesafe.ai/ |
| Vercel AI Gateway | 免费档 **每月 $5** | **必须绑卡验证**，否则 403；购买额度后不再享每月赠额 |
| OpenRouter | "新用户有少量试用额度，官方未公布固定金额；**Jev 为付费模型，不属于免费模型**" | — |

---

## ③ 国际社区

### 3.1 YouTube（信息密度最高，且已形成"Jev vs 本地模型"的固定选题）

标题/频道/播放量/发布日期/简介均通过 `youtube.com/results` 与视频页 curl 取得【仓库/代码：本机实测接口可读】。

| 标题 | 频道 | 播放 | 日期 | 链接 | 要点 | 可信度 |
| --- | --- | --- | --- | --- | --- | --- |
| Jev explained in 7min.. | Caleb Writes Code | 73.2万 | 2026-09-18 | https://www.youtube.com/watch?v=vj7hysh0mOI | 全站最大流量；简介明确"赞助：Junie（JetBrains）"，即**商业赞助内容**；讲 RLCD 与 LLM vs Jev | 【第三方文章（含赞助）】 |
| I Tested Jev vs 12 Local Decision Models. Here's What I'd Use... | The AI Automators | 1.84万 | 2026-09-28 | https://www.youtube.com/watch?v=zBw5BMrlZLo | **最有信息量的一条**：作者开源了 `theaiautomators/jev-arena`（测试代码+结果）；参赛 14 个模型：Winnow 12B、Decider 4B、Laya、SemIf 4B、Nimble 9B、CLM 8B、Plumb 4B、Qwen 3.5 4B、ModernBERT NLI 等；引用 JevBench（`fstandhartinger/jevbench`）与 `Hanno-Labs/decision-bench-leaderboard` | 【社区实测（代码可复现）】 |
| Open-Source Jev? Install Laya Locally + 3 Useful Demos | RUNTIME. | 4.49万 | 2026-09-21 | https://www.youtube.com/watch?v=J-Cn9UUJtdA | **最实用的 Laya 上手**：测试环境写死在简介里——"**Python 3.12 · Linux ARM64 / DGX Spark · Laya 0.3.5 · CPU**"；三个 demo（工单分诊并检查"接近临界"的判断、不确定就问、接本地 LLM 起草回复）；含 troubleshooting 与内存占用章节；配套代码 `Runtime-weekly/runtime-tutorials` | 【社区实测】 |
| How to Run Laya Locally for FREE — Jev vs Laya | Jigs Dev | 1.01万 | 2026-09-27 | https://www.youtube.com/watch?v=0ldz0pjDQB0 | 明确 "FREE"；含 Laya vs Jev 对比与**卸载/清模型**步骤；配套 `jigs10/laya-local-setup` | 【社区实测】 |
| 12 Jev Use Cases | Nate Herk \| AI Automation | — | — | https://www.youtube.com/watch?v=ymgH8jS6Wb8 | 12 个用例合集 | 【第三方文章】 |
| Jev by TypeSafe AI \| What is a System-1 Decision Model | CampusX | — | — | https://www.youtube.com/watch?v=0zFfcEr1e9U | 教学向 | 【第三方文章】 |
| Jev - The Ultimate Classification Model? | Sam Witteveen | — | — | https://www.youtube.com/watch?v=X117w2Rark8 | 作者另有 `Open Jev Models Are Here!!`（评 7 个开源 Jev 风格模型：SemIf、Nimble、D 等）与 `Using Jev In Your Agent Harness` | 【第三方文章】 |
| What is Jev? (and is laya better?) | GitButler（Scott Chacon） | — | — | https://www.youtube.com/watch?v=ty622HPl600 | **把 Jev/云端 API、本地 Laya、Kev 三方放在俄罗斯方块 + GitHub 设置筛选两个场景里对比** | 【社区实测】 |
| Laya vs Jev: We Tested the "40x Faster" AI Decision Model \| Toravo Lab #1 | Toravo | — | — | https://www.youtube.com/watch?v=-rf3ZzJ4HpQ | 直接检验"40× 更快"话术 | 【社区实测】 |
| Open-Source JEV AI is HERE! Install Laya Locally in 5 Minutes | Kev Builds Apps | — | — | https://www.youtube.com/watch?v=YJptns7lN8U | 5 分钟安装 | 【社区实测】 |
| Run Jev and Laya locally for free with TurboLLM | TurboLLM | — | — | https://www.youtube.com/watch?v=n3ltT5xKbz0 | 用第三方运行时免费跑 | 【社区实测】 |
| Jev explained in 17 minutes with Code / JEV Explained: The AI Model Designed for AUTOMATION / Jev Explained in 90 seconds / LLMs vs. Jev, Clearly Explained / Jev vs Laya: Do AI Agents Really Need an LLM for Every Decision? | codebasics / Neural Pulse / Singularity Feed / The Vibe Engineer / AgenticEngineering | — | — | `_yD790y_gq4` / `uJvuyc1lJMA` / `UA1BkYkP1DU` / `3dcqA8WtBB8` / `IisYIYtkHwE` | 大量高同质解释类内容；**同名"7min"视频至少有 5 个不同频道**（Caleb Writes Code、Arun singh、Bruno Vega、Build Alone、Smart Stack） | 【第三方文章】 |

### 3.2 Reddit（直连 403，改用 pullpush 归档 API 取到标题与部分正文/评论）

**⚠️ 未能访问**：`reddit.com`、`old.reddit.com`、`r.jina.ai` 代理全部返回 403（"blocked by network security"）。以下通过 `api.pullpush.io` 归档取得标题、正文，`num_comments` 字段在归档中普遍为 0（**评论数不可信**）【仓库/代码：本机实测接口可读】。

| 标题 | 子版 | 链接 | 要点 | 可信度 |
| --- | --- | --- | --- | --- |
| The hype of Jev pisses me off | r/LocalLLaMA | https://reddit.com/r/LocalLLaMA/comments/1wqcgfy/ | **最有价值的反方实测**：作者用 **$0.75 的 OpenRouter 预算** + PI agent，把"有用的部分"用普通 LLM 复刻出来。关键细节：① 想蒸馏时**查到 TypeSafe 服务条款明确禁止用 Jev 输出训练模仿/竞争模型**，于是改用公开的 `jev-bench`（Praveenrajus，**22,773 道测试题、22 个来源**）；② 做法极简——把候选答案编号成 0..N，要求只输出整数索引，取 1 个 token（DeepSeek tokenizer 里 0–150 都是单 token，151 选项也能一次决定）+ 可选 logprobs；③ 成本对比：Jev $0.042/MTok 很便宜，但 **Qwen 3.7 Flash 约 $0.03/MTok、Granite 约 $0.017/MTok**，而输出只有 1 个 token，所以"输出免费"这个卖点基本失去意义；④ 用 LEVI 跑 100 次 prompt 优化后，**DeepSeek 在 150 题开发面板上到 71.75%，而归档的 Jev 是 69.9%**（更大集合上 DeepSeek 掉回下方） | 【社区实测（含可复现方法）】 |
| 10 Technical Questions About Jev | r/LocalLLaMA | https://reddit.com/r/LocalLLaMA/comments/1wrn9zf/ | 系统列出官方**未披露**项：参数量/层数/隐层/注意力机制/backbone 全未公布；是否 Transformer-based 未确认；parallel sampler 内部实现未知；RLCD 具体训练细节未知 | 【社区传闻（提问而非结论）】 |
| TypeSafe / Jev API key suddenly returning 401 after previously working — anyone else seeing this? | r/AI_Agents | https://reddit.com/r/AI_Agents/comments/1wtck6j/ | **可用性踩坑**：现成可用的 Key 突然 401（正文被 `[removed]`，只有标题） | 【社区传闻】 |
| Couldn't get a Jev account, so I built a hosted API around the open Laya model | r/SideProject | https://reddit.com/r/SideProject/comments/1wssv4o/ | "我想注册时 Jev 已经关闭了"——第三方做了 **jein.dev**，端点 **Jev 兼容（Jev SDK 直接可用）**，**有免费额度 + sandbox**；作者用它在"维基百科游戏"里选下一个链接（支持回溯分支） | 【社区实测】 |
| Run Laya (open-source Jev) Locally on just 4GB RAM! | r/LocalLLM | https://reddit.com/r/LocalLLM/comments/1wsi601/ | **免费/低配关键证据**（帖为图片，正文为空）：4GB 内存本地跑 Laya | 【社区实测（图片，数字未独立验证）】 |
| The Security Gamble Behind "Jev Can't Hallucinate". | r/AIPractitionerGuides | https://reddit.com/r/AIPractitionerGuides/comments/1wtbwqq/ | CISSP 视角：Jev 在 **AI Agent 安全**里能用在哪、在哪停下、攻击者会打哪里（指向 substack 长文） | 【第三方文章】 |
| I turned Qwen3.8-27B Q2_64 + llama.cpp into a fully TypeSafe AI-compatible Jev-like system | r/LocalLLaMA | https://reddit.com/r/LocalLLaMA/comments/1wo6x7e/ | **具体配置数字**（标题）：`<10 GB VRAM`、**RTX 3090 上 170 ms**、聊天约 140 tok/s、在 22,000 条类型化决策基准上 **76% vs Jev-1.13 的 88%**，同时保留 OpenAI API 兼容 | 【社区实测】 |
| Mica v0.1 4B: open Jev-style decision model … runs on an 8 GB GPU — trained for under $30 of GPU time | r/LocalLLaMA | https://reddit.com/r/LocalLLaMA/comments/1wqag1i/ | 8GB 显存、训练成本不到 $30 | 【社区实测】 |
| I moved tool selection out of the main LLM and cut ~90% of token usage in one Codex workflow | r/LLMObservability | https://reddit.com/r/LLMObservability/comments/1wt0ifb/ | **与 Switchelp 场景一致**：把工具选择从主 LLM 挪走，Codex 工作流省约 90% token | 【社区实测】 |
| Small decison models are bubble? New models Jev, Laya, Drex, which one is the best? | r/AI_Agents | https://reddit.com/r/AI_Agents/comments/1wrur15/ | 泡沫质疑（正文被 removed） | 【社区传闻】 |
| Jev vs Laya: Which One Should You Actually Use? / Jev vs Laya vs Jevos / Has anyone compared Laya, Jev and Drex? | r/LLM、r/LocalLLM、r/SideProject | `1wtm2s5` / `1wtu6ar` / `1wrlb0x` | **"Jev vs Laya"已成固定选题**，且出现新名字 Drex、Jevos、Mica、Nagi、Credence、Jeff、Winnow、Decider、Plumb、CLM | 【社区传闻】 |
| I Spent 41,000 API Calls Trying to Break an AI Security Judge. Here's Where It Held and Where It Didn't. | r/LLM | https://reddit.com/r/LLM/comments/1wo4rnx/ | 4.1 万次 API 调用做对抗性测试（对象是"AI 安全裁判"） | 【社区实测】 |

**Reddit 上的重要噪音信号**：从 `1wtck6j`、`1wqcgfy`、`1wtbwqq` 三条帖子正文被 `[removed]`、以及 `1wqcgfy` 的评论（"This is too much slop to handle." / "ai slop" / "Violates Rule Three: LLM-generated content"）可以看出：**r/LocalLLaMA 版主在批量删 Jev 相关帖子**，理由是 LLM 生成内容与推广泛滥【仓库/代码：归档正文可见】。

### 3.3 Hacker News（帖子量大、共识分裂明显）

通过 HN Algolia API 取得【仓库/代码：本机实测接口可读】。

**主帖**：`Introducing System One Models and Jev` — **1989 分 / 491+ 评论**（https://news.ycombinator.com/item?id=49717558）。

HN 评论中的**关键共识与分歧**（通过 Algolia items API 取原评论）【第三方文章：HN 用户观点】：

- **怀疑基准可比性**（`ramon156`，高赞）："'RLCD' 和 'parallel sampling' 背后什么都没有；'70–500ms vs 3–329 秒'是拿苹果比橘子，除非 LLM 基线在做同等工作（比如长 CoT）。如果 Jev 对窄结构化任务完全跳过生成，那它当然更快。"
- **隐私/采购阻力**（`mushufasa`）："希望这类东西能通过 OpenRouter 或 AWS Bedrock 提供；直接新增一个模型供应商，在隐私与安全审查上很难过合规/采购流程。"
- **要求给代码**（`himata4113`）："他们从没展示具体怎么用，只有一堆'它在工作'的动画。想看 demo 的真实代码！"（后续确有社区帖子专门收集 demo 代码，见下）
- **命名困惑**（`andai`）："为什么叫 System One？'System One tasks / System One shaped queries'到底指什么？它是不是意味着模型很小？"——**官方从未公布模型规模**。
- **定价逻辑质疑**（`initsecret`）：对比表里 LLM"输出 token 比输入贵约 5 倍"，而 TypeSafe 写"输出 token 免费（便宜到无法计量）"，读者表示困惑。
- **正面落地预期**（`jrickert`）："我猜它能替换某条流水线里 **40–70% 的 LLM 调用**，把那部分 API 成本降一个数量级。"

**第二个高热度帖**：`Jev – A curation of Jev demos on X, tools, skills, and integrations` — **95 分 / 34 评论**（https://news.ycombinator.com/item?id=49802160）。其中：

- **真实成本数据**（`davidweatherall`）："我做的是用户打字时实时识别名词……**已经发了 1000+ 次请求，花了 $0.01（大概还四舍五入上去了）**。"
- **效率提升实测**（`liquicity`）："我的 side project 有个 'smart categorise' 按钮，把条目分到约 20 个预设清单。原来用 DeepSeek V4 Flash 要 **30–60 秒**，Jev **<1 秒**，成本相同。"
- **⚠️ 刷榜指控（社区传闻，议题重要）**：
  - `upupupandaway`："不想当阴谋论者，但过去两周看起来像一场**协调一致的 Jev 推广活动**。"
  - `GodelNumbering`："我极难相信 Jev 团队没有在做大规模**水军营销**。Reddit 上也是，所有 LLM 子版都被 Jev 帖子淹没，很多是只谈 Jev 的新账号，很多是伪装成分享知识的广告（还给了 r/LocalLLaMA 上已被删除的具体帖子链接）。"
- `lewisjoe` 提问："谁能用最简单的话解释，**作为 LLM API 的终端用户**，为什么 Jev 这类模型重要？"
- `ChaitanyaSai` 吐槽该推广帖文风（"please stop using Claude"），侧面印证**推广文案大量由 LLM 生成**。

**HN 其它值得记的帖子**（标题级）【仓库/代码：API 标题可读】：
`Show HN: CUA-S1 – A System One Model for Computer Use`（95 分，trycua/cua）；`How do you run System One decision models locally?`（stackness.dev，讲 Ollaya / Laya MLX / runtime slot）；`Turning GLM-5.3-Flash into a Jev-like decision model`（privatemode.ai，137 分 / 59 评论）；`Does an open-weight decision model beat a hosted one? Jev vs. Laya`（astgl.com，15 分）；`Laya, an open decision model that plays Tetris by itself`（brainfunctioncollapse.com）；`Sub-15ms, non-autoregressive, local drop-in alternative to TypeSafe Jev`（wfzyx/von）；`Typesafe's Jev is the fish at the poker table`（backnotprop.com/blog/jev-poker/）；`TypeSafe's Jev Can't See. I Made It Guess What I Drew Anyway`（mikulskibartosz.name）；`A deep dive into Jev, TypeSafe's System One model`（flaviocopes.com/jev/）；`An early-access test of TypeSafe's Jev: calibrated judgments for half a cent`（lindfors.no）；`Show HN: OpenDecision – a 400M zero-shot model makes local decisions, plays Doom`。

### 3.4 X / Twitter（只能单条读取，无法搜索）

**⚠️ 部分未能访问**：`x.com` 搜索与时间线无法读取；但 `publish.twitter.com/oembed` 端点可用，**可精确核对已知推文内容**【仓库/代码：本机实测接口可读】。

- **创始人首发推文**（已核实原文）：Diogo Almeida（@CompleteSkeptic），2026-09-15，"After co-inventing ChatGPT, I kept asking myself: why have superhuman chat models not led to AGI? I've spent the last 2 years in stealth building a new way to train models (RLCD), and a new type of frontier AI model that we are releasing today: Jev • 20-200x faster • 40-400x…" — https://x.com/CompleteSkeptic/status/2099925682726002904
  - **注意**：官方博客写的是 **40x–200x faster / 193.6× 与 444.6× 成本**，而创始人推文写的是"**20-200x faster / 40-400x**"，两处口径不一致【官方 vs 官方】。
- **中文圈转述的 X 内容**（透过 B站搬运视频与 AMD/社区文章间接引用，**未直接核实原文**）：Vercel 用 Jev 审查 Agent 命令、日本法令检索进生产、`@CompleteSkeptic` 的"Jev 实时换装"演示（B站有搬运："海外Jev模型实时换装刷屏，40秒拆解724支广告成本仅需0.09美元"，https://www.bilibili.com/video/BV1NteB6MEpo ）【社区传闻】
- HN 上被引用的 X 演示还包括：`@dWeaths`（实时名词识别）、`@RaahelSaidWhat`（hn4me.xyz 用 Jev 按兴趣策展 HN）【第三方文章】

---

## ④ 踩坑与争议汇总

### 4.1 校准与错误率（**最实质的一类问题**）

1. **"零幻觉"是语义游戏**。官方口径是"不可能输出预设类型之外的值"，但社区一致指出它会**高置信度地选错**。掘金深度文实测：**150 个扑克决策点只有 63% 与求解器最优一致；最错的答案置信度反而最高（0.86），最接近正确的只有 0.09；且错误高度可复现，不是噪声。**【第三方文章】
2. **官方自己承认校准是"群体级"**。文档说明校准是 group-level，**不保证单个答案**；低置信度请求应路由给人工。Jev 官方文档另有 "model-jaggedness" 页面（`docs.typesafe.ai/model-jaggedness/jev-1.13`）专门讲模型脾气【官方 + 社区引用】。
3. **Laya 官方承认过度自信**。模型卡明确写"overconfident and needs domain-specific calibration"、**仅英文**（早期 checkpoint）、"**accuracy falling as the option list grows**"（选项越多越不准）【仓库/代码】。
4. **量化后校准会进一步崩坏**。diffusiongemma 在 39 条输出里 37 条是精确 0.0/1.0【社区实测】；Laya 的三方实测建议**必须自己分桶验证**（astgl 用 50–100 条标注样本冻结阈值）【第三方文章】。
5. **阈值必须自测**。掘金实测发现同一模型"最优阈值下"比"0.5 阈值"高 2–3 个百分点且各模型最优阈值不同（Kev 0.35 / SemIf 0.70 / diffusiongemma 0.95）【社区实测】。

### 4.2 把决策模型当 LLM 用的误用

- **官方定位明确**：Jev 不生成文本、不做模型调度、不做推理。但 CSDN 的《Jev 接入 Codex 指南》把它写成"推理调度引擎"并虚构 `jev-max`/`jev-mini` 模型名【第三方文章（**内容错误**）】。
- **中文圈大量"Jev 玩 MC / 玩植物大战僵尸"演示**：B站"飞天闪客"（14 万播放）专门做了一期《这些 Jev 的案例都是骗人的！揭秘 Jev 玩我的世界效果骗局》【第三方文章】；HN 同期也有人抱怨 demo 只给动画不给代码【第三方文章】。
- **`Devin-AXIS/jev-dsh-decision` 的自我约束值得抄**：明确写"**不执行推荐动作，也不自动切换模型或路由所有请求**"、"**Jev 不返回文字推理过程，Agent 的解释不能冒充 Jev 的原始输出**"——这正是防误用的正确姿势【仓库/代码】。
- **接口误用**：国内聚合平台上走 `/v1/chat/completions` 会返回 400 `Model does not exist`，**必须走 `/v1/systemone`**；且不同平台的 Noul/Score 实现质量差异极大（SemIf 的 score 好于 noul）【社区实测】。

### 4.3 隐私

- **本项目最需要警惕的一点**：中文圈最火的一批应用（`crush-monitor`、`jev-chat-*`、`wechat-triage-hud`）**都是把完整的微信/QQ/飞书聊天记录发给决策模型的**。crush-monitor README 自己写"**本机运行，自带 Key**"，但"本机运行"只指服务在本机，**数据仍然出网到 TypeSafe/Vercel/OpenRouter**；它的替代版 `yzyialy/crush-monitor` 才明确强调"数字全部由程序计算，**数据只存在本机**"【仓库/代码】。
- HN 高赞评论指出，**企业侧的主要障碍不是能力而是合规**："直接新增一个模型供应商，在隐私与安全审查上很难过合规/采购流程"，希望走 OpenRouter / Bedrock【第三方文章】。
- 反面：`jev-chat/*` 系列刻意做成"**只读屏幕、不 hook、不改包、发送永远手动**"的非侵入设计，这是中文圈在隐私上少见的正面实践【仓库/代码】。

### 4.4 成本陷阱

- **输出免费 ≠ 不要钱**：input 按 **$0.042/MTok** 收费，`state` + `questions` **共用约 32K token 预算**，问题本身也计费。高频闭环（如 Doom 演示）实测**约 $7/小时**【第三方文章】；但轻度用途极便宜（**1000+ 次请求 $0.01**）【社区实测】。
- **"输出免费"的卖点可被替代方案消解**：如果只让模型输出一个 token（如整数索引），输出成本本来就趋近于 0，因此 **Qwen 3.7 Flash（≈$0.03/MTok）与 Granite（≈$0.017/MTok）在输入侧比 Jev 更便宜**——这是 r/LocalLLaMA 反方最有力的论点【社区实测】。
- **免费额度有绑定门槛**：Vercel AI Gateway 免费档 $5/月**必须绑卡**，否则 403；OpenRouter 上 Jev 是付费模型；TypeSafe 官方 $5 试用额度"后续是否赠送以控制台为准"【仓库/代码】。
- **排队/关停**：早期需要排队申请 Key（B站多条视频以"不用排队了"为卖点），且出现过"想注册时已经关闭"与"Key 突然 401"【社区传闻 / 社区实测】。
- **Laya 的隐性成本**：没有商用 API，"**只能自托管，不太适合面向多用户调用**"；本地部署省了 token 但换来硬件、电、存储与运维（astgl 作者明确说他没有算盈亏平衡点）【第三方文章】。

### 4.5 许可证与"空壳仓库"现象

- **许可证**：Laya **Apache-2.0**（可商用）、von **Apache-2.0**、kev **Apache-2.0**、SemIf **MIT**、`1Panel/laya-server` **Apache-2.0**、`typesafe-ai/skills` **MIT**。**未发现 Laya 侧许可证争议**【仓库/代码】。
- **⚠️ 但 Jev 侧是闭源 + 限制性条款**：r/LocalLLaMA 反方作者查到 **TypeSafe 服务条款明确禁止用 Jev 的输出训练模仿/竞争模型**，因此放弃蒸馏路线（这也是社区转向 `jev-bench` 等独立基准的原因）【社区实测，条款原文未直接核实】。
- **"空壳仓库 / 星级 farming"现象确实存在**：GitHub 上至少 **4 个高度同名的 "awesome-jev" 目录**同时冲到高星——`yibie/awesome-jev`（2000★）、`heyjunpenn/awesome-jev`（903★，自述收录 962 个项目）、`v-modal/awesome-jev-tools`（738★）、`AnotiaWang/awesome-jev`（578★）、`OmniJev/awesome-jev-gallery`（479★）、`cobanov/awesome-jev`（469★）、`moritzkremb`、`hellogumbo`、`valentynkit` 等。多个仓库**内容基本都是 README + 分类目录**（`yibie` 版全仓仅 1.4 MB、`AnotiaWang` 版 886 KB）【仓库/代码】。
- **同期出现大量"目录站/榜单站"**：`jevmeter`、`jev-architect`、`decision-index`、`heroku16/openjev` 等，以及 r/LocalLLaMA / r/Jev / r/JEVs / r/typesafe / r/typesafe_jev 等**多个新子版**同时冒出来【仓库/代码】。
- **HN 与 Reddit 的"水军"指控**是本轮讨论中最一致的一条负面共识（见 3.3 / 3.2）【社区传闻】。**建议：评估生态时要按"有可运行代码 + 有 benchmark 数字 + 有非全新账号的讨论"三条筛，不要按 star 数筛。**

### 4.6 其它已知失败模式

- **CJK / 多语言**：Jev 被指"English-centric，中日韩准确率更低"（成本节省可能被人审抵消）；Laya 早期 checkpoint **仅英文**，需显式用 `laya-multilingual`（mmBERT-base，322M）【第三方文章 / 仓库代码】。
- **长文档截断**：`laya-multilingual` **默认 1024 token 上限会截断长文**，必须显式传 `max_len=8192`；官方自测"约 4000 token 文本前 20 题对 16–18 题，再长就飘（8–17/20）"，4000 token 输入在 Apple GPU 上约 **1.7 秒**【仓库/代码，官方自测】。
- **选项数量上限**：Jev Choice 上限 **255**，更多需两级打分；Laya 在 astgl 的部署里把 **Choice 上限收在 20**，且选项变多准确率下降【官方 + 社区实测】。
- **依赖裸 `transformers` 会出错**：用 `AutoModel` 只加载到编码器形状，**必须用官方发布的 Laya runtime**（astgl 实测踩坑）【社区实测】。
- **Jev Boolean（Noul）没有置信度**：`P(true)=0.5` 只代表 50/50【第三方文章】。
- **延迟在真实负载下会退化**：astgl 在同一台机器上，留出集 p95 从 **143.54 ms 涨到 466.69 ms**（busy host），超过 300 ms 网关超时并触发 fallback【社区实测】。

---

## ⑤ 免费 / 省钱路子

### 5.1 官方与主流平台的免费额度

| 渠道 | 免费额度 | 门槛 | 可信度 |
| --- | --- | --- | --- |
| TypeSafe 官方 | **$5 试用额度**（新用户，核对日期 2026-09-22，后续以控制台为准） | 官网注册 + 邮箱验证（有视频称约 2 分钟） | 【仓库/代码 + 官方】 |
| Vercel AI Gateway | **$5/月**免费档，可调 Jev | **必须绑卡**，否则 403；买过额度后不再享每月赠额 | 【仓库/代码】 |
| OpenRouter | 新用户少量试用额度；**Jev 是付费模型**（`typesafe/jev-1.13`：prompt $0.000000042/token、completion $0（即 $0.042/MTok）、ctx 32000、maxout 28800）；另有路由模型 `typesafe/jev-router` | 注册即用 | 【官方：OpenRouter API】 |
| Hugging Face Space | **`convaiinnovations/laya-demo`**（Gradio，251 likes）**正在 `zero-a10g` 免费 GPU 上运行**，可直接试 Laya | 无需 Key | 【仓库/代码】 |
| 硅基流动 SiliconFlow | 模型广场提供 **Kev-4B / SemIf / diffusiongemma**，走 `https://api.siliconflow.cn/v1/systemone` | 需注册账号 | 【社区实测】 |
| jein.dev（第三方） | 用开源 Laya 自建托管，**Jev 兼容端点**（Jev SDK 可直接指过去），**有免费额度 + sandbox** | 注册 | 【社区实测】 |
| 中文第三方"免费 Jev Key" | B站"鲲鹏Talk"把 0.4B Jev 兼容模型挂到 **Omni Labs**，注册即给免费 Key（22k 播放）；B站 KnoxCore 称"完全免费提供 Jev API"；CSDN 提到 **TaoToken 只做 Key 分发** | 注册 | 【社区实测 / 社区传闻】——**第三方中转会看到你的全部请求内容，谨慎** |

### 5.2 本地零成本跑 Laya（**最推荐的省钱路线**）

1. **一句话安装**：`python -m pip install laya`（Python ≥3.10），`Router()` 首次使用自动下权重；可选 extras：`laya[serve]`（HTTP 服务）、`laya[mcp]`（MCP server）、`laya[langchain]`、`laya[llamaindex]`、`laya[crewai]`、`laya[onnx]`、`laya[fast]`（TileLang GPU 快路径）【仓库/代码】
2. **Docker 一条命令**（国内团队维护，带 Web 控制台与 API Key 管理，兼容 TypeSafe 线协议）：
   `docker run -d --name laya-server -p 8080:8080 -v laya-data:/data -e LAYA_ADMIN_USERNAME=admin -e LAYA_ADMIN_PASSWORD='...' 1panel/laya-server:latest`，随后 `POST /v1/systemone`【仓库/代码】
3. **GGUF 量化尺寸**（决定需要多少内存/显存）【仓库/代码：HF API 文件列表】：

| 量化 | 英文 Laya | 多语言 Laya |
| --- | --- | --- |
| F16 | 846 MB（`mys/laya-GGUF`）/ 791.5 MB（`fr0stbit3/laya-gguf`） | 663.3 MB（`mys/laya-multilingual-GGUF`） |
| Q8_0 | 451.5 MB / 421.4 MB | 361.7 MB |
| **Q4_K_M** | **419.9 MB / 272.2 MB** | **523.9 MB** |

（`fr0stbit3/laya-gguf` 另含 `laya-head.safetensors` 106.1 MB；社区另有 `aac6fef/laya-multilingual-coreml-ane`、`FluidInference/laya-coreml`（41 likes，Apple CoreML）、`litert-community/Laya-Multilingual-LiteRT`、`sahilchachra/Laya-*-MXFP4/MXFP8`、`AXERA-TECH/Laya`、`inferenceprince/laya-onnx` 等硬件专用版本）

4. **性能参照**：T4 上单问题 p50 **32.8 ms**、10 问题打包 **72.3 ms**（来自官方 Laya 的对比口径）【第三方文章转述】；B站实测 **45 ms / ¥0**（AIStarter）【社区实测】；CPU-only 在 Linux ARM64 / DGX Spark 上可跑（RUNTIME. 的测试环境）【社区实测】；**4GB 内存**可跑（r/LocalLLM）【社区实测（图片）】；4000 token 输入在 Apple GPU 约 1.7 s【仓库/代码：官方模型卡自测】。
5. **其它本地替代**：`wfzyx/von`（395M、OpenVINO/CUDA/ROCm/Apple MPS、**CPU 上 sub-15ms**、权重约 3 GB、drop-in `/v1/systemone`）、`jaredpalmer/kev`（Qwen3.5 底座，0.8B/4B/9B）、`TheoLeeCJ/SemIf-OpenJev`（不训练，读现成模型 logits，一张 3090 甚至 WebGPU 就能跑）、`razorback16/openjev`（DiffusionGemma 路线）、`ekzhang/openjev-sglang`（prefill-only）。
6. **⚠️ 未找到**：**Cloudflare Workers AI 未提供 Laya/open-jev**（查询其模型搜索 API 未命中相关模型）；**Netlify 未找到相关免费推理**。HF Spaces 侧除 `convaiinnovations/laya-demo` 外，还有 `abhishekbakhat/laya-multilingual-demo`、`thaitea/laya-vision-demo`、`mizchi/laya-web-demo`、`FINAL-Bench/Tetris-JEV-LAYA-ZTC`（浏览器/静态版，可自部署）【仓库/代码】。

---

## ⑥ 对 Switchelp 的启示（**全部为推测**）

Switchelp 的既有页面（概览/网关/配置/用量统计/插件中心/内容中心/免费额度/工具管理/设置）与本次调研到的社区实践有四处天然接合点：

1. **"网关 + 决策路由"是社区已验证的需求，且痛点就是 Switchelp 的强项**。
   - 证据：B站《有了 Jev、Laya 决策模型，AI 网关的路由决策，稳准狠！》（2k 播放，直接讲 1Panel AI 网关 + laya-server）、B站《告别模型选择困难！用 Jev 决策模型为 Codex 实现全自动路由实测》（1.3k 播放，在 GPT-5.6 Luna/Sol/GPT-6 Astra 之间自动路由）、r/LLMObservability "把工具选择移出主 LLM，Codex 工作流省 90% token"、HN `jrickert` 预期"替换 40–70% 的 LLM 调用"。
   - 落地建议：Switchelp 本机网关可以把"**低成本预判 → 高成本执行**"做成内置能力：对每个入站请求先用 Laya/本地决策模型做一次 Choice（该发哪个 provider/model 档位、是否需要联网/工具、是否直达人工），低置信度再走原路径。**默认关闭、显式启用**，并像 `jev-dsh-decision` 那样承诺"不自动切换模型、不冒充决策模型的推理"。
2. **"免费额度"页可以直接把这些渠道产品化**。
   - 社区已验证的免费路线清单：TypeSafe $5 试用、Vercel AI Gateway $5/月（需绑卡）、OpenRouter 试用、HF Space 免 Key 的 Laya demo、硅基流动的 `/v1/systemone`（Kev/SemIf/diffusiongemma）、jein.dev 免费额度、本地 Laya（Q4 仅 272–524 MB）。
   - 落地建议：把"**本地 Laya 一键起服务（Docker/Python 两种）+ 免费渠道探测（额度/是否需绑卡/是否兼容 `/v1/systemone`）+ 兼容性自检**"做成一屏。特别要内置"**只认 `/v1/systemone`，不要走 `/v1/chat/completions`**"的校验，并给出一条真实的连通性测试请求。
3. **"用量统计"需要一套新的计量维度：决策调用 ≠ 对话调用**。
   - 关键计量事实：**input 按 $0.042/MTok 收费、output 完全免费**、`state`+`questions` 共用约 32K token 预算、问题本身也计费、合并 13 个问题比分开便宜 11.5×/快 9.6×、**Choice 上限 255**、Noul 无置信度、Doom 类高频闭环可达 $7/小时。
   - 落地建议：用量页把"决策类调用"单列，指标用 **决策次数 / 输入 token / 单次成本（应近似 0）/ 低置信度占比 / 阈值命中率**；并对"合并问题数"给出优化提示（合并越多越省）。
4. **"插件中心 / 内容中心"的真正差异化是"隐私边界"而不是"更多玩法"**。
   - 社区最强的负面共识：**水军推广**（HN 两条高赞指控 + Reddit 版主批量删帖）、**大量 README-only 的空壳 awesome 仓库**（同名目录 4 个以上，最高 2000★）、**中文聊天分析类工具把聊天记录发给云端决策模型**。
   - 落地建议：Switchelp 若做插件，**默认本地推理（Laya/von，Q4 约 272 MB 起）**、在 UI 上明确标注"这次请求会不会出网 / 出网到哪个 provider / 发了多少字符"；把"**什么场景不该用**"写进插件说明（深度推理、长文生成为、CJK 高精度要求、多选项分类）。
5. **风险提示（必须诚实呈现）**：
   - Jev 官方数字（193.6×/444.6×）是自评且口径在自家博客与创始人推文之间不一致（40–200× vs 20–200×）；**不要在产品内引用未经复现的倍数**。
   - Laya 在"选项多 + 中文"场景明显掉点（Banking77 42.5% vs Jev 87.0%；14 条 SQL 只对 6 条），**"开源平替"要按场景限定表述**，否则会伤害信任。

---

## ⑦ 来源清单

**官方**
- TypeSafe AI 发布公告（2026-09-15，含 RLCD/定价/延迟口径）— https://typesafe.ai/blog/introducing-system-one-models-and-jev
- TypeSafe 控制台 — https://console.typesafe.ai/
- TypeSafe Agent Skills 仓库（MIT，2458★）— https://github.com/typesafe-ai/skills
- OpenRouter 模型与端点 API — `https://openrouter.ai/api/v1/models/typesafe/jev-1.13/endpoints`（prompt 0.000000042 / completion 0 / ctx 32000 / maxout 28800）；`typesafe/jev-router`
- OpenRouter 模型页 — https://openrouter.ai/typesafe/jev-1.13/
- Vercel AI Gateway 定价 — https://vercel.com/docs/ai-gateway/pricing
- 创始人首发推文（oembed 核实原文）— https://x.com/CompleteSkeptic/status/2099925682726002904

**仓库 / 代码**
- Laya 主仓（Apache-2.0，28864★，2518 forks）— https://github.com/NandhaKishorM/laya ｜权重 https://huggingface.co/convaiinnovations/laya（4554 likes）｜多语言 https://huggingface.co/convaiinnovations/laya-multilingual ｜Space https://huggingface.co/spaces/convaiinnovations/laya-demo
- Laya Server（1Panel，Apache-2.0）— https://github.com/1Panel-dev/laya-server
- von（Apache-2.0，780★）— https://github.com/wfzyx/von
- kev（Apache-2.0，7974★）— https://github.com/jaredpalmer/kev
- SemIf-OpenJev（MIT，4594★）— https://github.com/TheoLeeCJ/SemIf-OpenJev
- browser-use/jev-ultrafast（MIT，21469★）— https://github.com/browser-use/jev-ultrafast
- jev-chat/jev-chat-jarvis（MIT，7155★）— https://github.com/jev-chat/jev-chat-jarvis
- FerryCorleone/crush-monitor（MIT，262★，含免费额度对照表）— https://github.com/FerryCorleone/crush-monitor
- Devin-AXIS/jev-dsh-decision（335★）— https://github.com/Devin-AXIS/jev-dsh-decision
- jigs10/laya-local-setup — https://github.com/jigs10/laya-local-setup
- Runtime-weekly/runtime-tutorials — https://github.com/Runtime-weekly/runtime-tutorials
- theaiautomators/jev-arena — https://github.com/theaiautomators/jev-arena
- GGUF 量化：https://huggingface.co/fr0stbit3/laya-gguf ｜ https://huggingface.co/mys/laya-GGUF ｜ https://huggingface.co/mys/laya-multilingual-GGUF
- 独立基准：JevBench https://github.com/fstandhartinger/jevbench ｜ DecisionBench https://huggingface.co/spaces/Hanno-Labs/decision-bench-leaderboard

**中文社区**
- B站（见 2.1 表，代表性：BV1wDhj6wEa8 / BV1ZLht69E7b / BV1Pfhf6dEkN / BV1CZe86yEst / BV1qfaw6pEUo / BV1pCeY6QEQT / BV17baK68Eay）
- 掘金（见 2.3 表，代表性：7688895674230833179 / 7688401277182066738 / 7687793891199418374 / 7686669083098775562 / 7688528701483515945 / 7688569614586789914）
- CSDN 搜索 API 结果（30 条）+ 对 `weixin_29062255/article/details/166762980` 的错误内容核查
- 微信公众号（经搜狗微信搜索取标题/摘要）：《一口气看完 5721 个 Jev 案例：6 类场景值得落地，5 个坑先别踩》《Laya 轻量决策模型部署指南：硬件配置、Agent 路由与应用场景》《Laya 开源决策模型，比 TypeSafe Jev 快 7.8 倍》
- 知乎（仅标题/摘要，正文未核实）— https://zhuanlan.zhihu.com/p/2085760548034167437
- 极道 jdon.com 多篇译文；一聚教程网 111cn.net；typesafe-jev.com；whatisjev.com/zh；jevai.dev/zh-Hant
- 台湾繁中 SEO 站群：bnext.com.tw/article/92319、klab.tw、aiposthub.com、grenade.tw、ai.com.tw、techhanlin.tw、yololab.net

**国际社区**
- HN Algolia API 检索（`Introducing System One Models and Jev` 1989 分：https://news.ycombinator.com/item?id=49717558；`Jev – A curation of Jev demos…` 95 分：https://news.ycombinator.com/item?id=49802160）
- Reddit 归档（pullpush.io）：r/LocalLLaMA `1wqcgfy` / `1wrn9zf` / `1wo6x7e` / `1wqag1i`；r/AI_Agents `1wtck6j` / `1wrur15`；r/LocalLLM `1wsi601`；r/SideProject `1wssv4o`；r/LLMObservability `1wt0ifb`；r/AIPractitionerGuides `1wtbwqq`；r/LLM `1wo4rnx`
- YouTube（见 3.1 表，代表性：`zBw5BMrlZLo` / `vj7hysh0mOI` / `J-Cn9UUJtdA` / `0ldz0pjDQB0` / `ty622HPl600` / `-rf3ZzJ4HpQ` / `X117w2Rark8`）
- 第三方长文：https://juejin.cn/post/7686669083098775562 （引用 backnotprop.com/blog/jev-poker/、seangoedecke.com/jev-means-structured-output-is-interesting-again/）；https://astgl.com/p/local-laya-vs-hosted-jev-typed-decisions ；https://www.privatemode.ai/blog/system-one-from-glm-flash

**未能访问 / 未能核实清单**
1. **小红书**：未能访问（页面 JS 渲染；子智能体环境浏览器不可用，报错 "Browser is not available in subagent"）。**无任何小红书条目**。
2. **知乎正文**：curl / WebFetch / r.jina.ai 全部 403 或验证码；仅有 1 条标题-摘要级证据，正文数字**未核实**。
3. **Reddit 直连**：403；评论数、投票数不可得；部分帖子正文被 `[removed]`。**未能取得 r/PiCodingAgent 的任何内容**（pullpush 该版无归档）。
4. **博客园**：搜索 0 篇 + 人机验证；**未找到任何 Jev 相关博客园文章**（同名 "Laya" 均为 LayaAir 游戏引擎，已排除）。
5. **X/Twitter**：仅能逐条读已知推文（oembed），**无法搜索、无法读取评论区**；中文圈转述的 X 内容（Vercel 生产案例、日本法令检索、换装演示）**未直接核实**。
6. **百度/搜狗网页搜索**：百度返回安全验证、搜狗在多次请求后限流；`site:` 与 `-` 运算符在 Bing 上失效，中文搜索覆盖度有限。
7. **"免费 Jev Key"渠道**（KnoxCore、JayCode 1.2 亿 Token、TaoToken）：**仅有标题/摘要，未核实**是否存在、是否有额度限制、是否会看到请求内容。
8. **Laya 4GB RAM 实测**：原帖为图片，**数字未独立验证**。
9. **Jev 服务条款"禁止蒸馏"条款原文**：来自 Reddit 作者转述，**未直接核实条款文本**。
10. **Cloudflare Workers AI / Netlify 是否支持 Laya**：**未找到**相关模型或免费额度。
