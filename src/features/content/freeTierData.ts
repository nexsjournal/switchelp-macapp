import type { FreeTierCatalog } from './freeTierPolicy';

/**
 * 随包基线清单（docs/design/09）。
 *
 * 收录规则：**只收厂商官方文档里写明的免费档**，每条必须带 docsUrl 与核实日期；
 * 二手聚合博客不作为收录依据（它们自己都声明限额随时会变）。数字是快照——
 * 卡片上永远显示「核实于」，过期只改这份清单，不改产品。
 *
 * 核实来源：2026-09-25 逐条抓官方文档（docs/research/06 §9.2），2026-09-30 补查
 * Groq / Cerebras / Mistral / 国内四家（SiliconFlow、火山方舟、混元、千帆）的公开政策；
 * Groq 与 Cerebras 的具体数字各家按模型浮动，条目里写的是档位结构并注明以官方页为准。
 */
export const FREE_TIER_CATALOG: FreeTierCatalog = {
  version: 1,
  verifiedAt: '2026-09-30',
  entries: [
    // ---- 模型免费档 ----
    {
      id: 'openrouter-free', provider: 'OpenRouter', icon: 'openrouter', category: 'model_free_tier',
      title: ':free 模型变体每日免费调用',
      quota: '未购过额度 50 次/天；累计充值 ≥$10 后 1000 次/天；20 次/分。官方额度接口可直接查询剩余量',
      docsUrl: 'https://openrouter.ai/docs/api-reference/limits', claimUrl: 'https://openrouter.ai/keys',
      presetId: 'openrouter', lastVerifiedAt: '2026-09-25',
    },
    {
      id: 'zhipu-glm-flash', provider: '智谱 BigModel', icon: 'zhipu', category: 'model_free_tier',
      title: '多个明确标注「免费」的 GLM 模型',
      quota: 'GLM-4.5-Flash / GLM-4V-Flash 等免费模型在模型广场直接标注，无需额度',
      docsUrl: 'https://docs.bigmodel.cn/cn/guide/start/model-overview', claimUrl: 'https://open.bigmodel.cn',
      presetId: 'zhipu', lastVerifiedAt: '2026-09-25',
    },
    {
      id: 'hunyuan-lite', provider: '腾讯混元', icon: 'hunyuan', category: 'model_free_tier',
      title: 'Hunyuan-Lite 永久免费',
      quota: 'Lite 档免费（QPS 有限）；首次使用另赠 10 万 token 体验额度（1 年有效）',
      docsUrl: 'https://cloud.tencent.com/document/product/1729', claimUrl: 'https://cloud.tencent.com/product/hunyuan',
      lastVerifiedAt: '2026-09-30',
    },
    {
      id: 'qianfan-free', provider: '百度千帆', icon: 'baidu', category: 'model_free_tier',
      title: 'ERNIE 轻量档永久免费 + 新客体验',
      quota: 'ERNIE-3.5-8K 永久免费（QPS 50）；ERNIE-4.0 新用户 100 万 token/月',
      docsUrl: 'https://cloud.baidu.com/doc/WENXINWORKSHOP/index.html', claimUrl: 'https://cloud.baidu.com/product/wenxinworkshop',
      lastVerifiedAt: '2026-09-30',
    },
    {
      id: 'groq-free', provider: 'Groq', icon: 'groq', category: 'model_free_tier',
      title: '免费档无需绑卡即可用全部模型',
      quota: '按模型分档限速（约 30 次/分、天级请求数与 token 上限随模型不同），以官方限速页的表格为准',
      docsUrl: 'https://console.groq.com/docs/rate-limits', claimUrl: 'https://console.groq.com/keys',
      lastVerifiedAt: '2026-09-30',
    },
    {
      id: 'cerebras-free', provider: 'Cerebras', icon: 'cerebras', category: 'model_free_tier',
      title: '注册即有每日免费 token 额度',
      quota: '约 100 万 token/天（Llama 系列等开源模型），以控制台为准',
      docsUrl: 'https://inference-docs.cerebras.ai/support/pricing', claimUrl: 'https://cloud.cerebras.ai',
      lastVerifiedAt: '2026-09-30',
    },
    {
      id: 'mistral-experiment', provider: 'Mistral', icon: 'mistral', category: 'model_free_tier',
      title: 'Experiment 免费档',
      quota: '每月约 10 亿 token；请求速率很低（约 1–2 次/分），适合批处理而非实时应用',
      docsUrl: 'https://docs.mistral.ai/deployment/labs/', claimUrl: 'https://console.mistral.ai',
      lastVerifiedAt: '2026-09-30',
    },
    {
      id: 'google-ai-studio', provider: 'Google AI Studio', icon: 'google', category: 'model_free_tier',
      title: 'Gemini API 免费档（无需绑卡）',
      quota: '官方已不再公布固定数字，具体限额在 AI Studio 内查看；每日请求窗太平洋时间午夜重置',
      docsUrl: 'https://ai.google.dev/gemini-api/docs/rate-limits', claimUrl: 'https://aistudio.google.com/apikey',
      lastVerifiedAt: '2026-09-25',
    },
    {
      id: 'deepseek-none', provider: 'DeepSeek', icon: 'deepseek', category: 'model_free_tier',
      title: '没有免费档',
      quota: '官方定价仅按 token 计费；网上流传的「DeepSeek 免费额度」均非官方',
      docsUrl: 'https://api-docs.deepseek.com/quick_start/pricing',
      lastVerifiedAt: '2026-09-25',
    },
    {
      id: 'kimi-none', provider: 'Moonshot Kimi', icon: 'moonshot', category: 'model_free_tier',
      title: '没有免费档',
      quota: '文档中未提供免费额度或免费模型',
      docsUrl: 'https://platform.kimi.com/docs/guide/start-using-kimi-api',
      lastVerifiedAt: '2026-09-25',
    },

    // ---- 试用金 / 新客额度 ----
    {
      id: 'dashscope-new', provider: '阿里云百炼（千问）', icon: 'qwen', category: 'trial_credit',
      title: '新用户按模型各送免费 token',
      quota: '每模型 100 万 token、90 天有效；仅北京地域，不可跨模型合并',
      docsUrl: 'https://help.aliyun.com/zh/model-studio/new-free-quota', claimUrl: 'https://bailian.console.aliyun.com',
      presetId: 'qwen', lastVerifiedAt: '2026-09-25',
    },
    {
      id: 'volc-ark', provider: '火山方舟（豆包）', icon: 'doubao', category: 'trial_credit',
      title: '每款豆包模型赠免费 tokens',
      quota: '每款豆包大模型 50 万 tokens；企业协作计划每日最高 500 万',
      docsUrl: 'https://www.volcengine.com/docs/82379', claimUrl: 'https://console.volcengine.com/ark',
      lastVerifiedAt: '2026-09-30',
    },
    {
      id: 'siliconflow-new', provider: '硅基流动', icon: 'siliconcloud', category: 'trial_credit',
      title: '新客体验金 + 长期免费的小模型',
      quota: '注册送 ¥14（约 2000 万 token，不可用于满血 R1）；小型/嵌入/重排模型长期免费（限速较低）',
      docsUrl: 'https://docs.siliconflow.cn/cn/userguide/introduction', claimUrl: 'https://cloud.siliconflow.cn',
      presetId: 'siliconflow', lastVerifiedAt: '2026-09-30',
    },
    {
      id: 'cohere-trial', provider: 'Cohere', icon: 'cohere', category: 'trial_credit',
      title: '试用 Key 免费调用',
      quota: '1,000 次调用/月；chat 模型 20 次/分',
      docsUrl: 'https://docs.cohere.com/docs/rate-limits', claimUrl: 'https://dashboard.cohere.com',
      lastVerifiedAt: '2026-09-25',
    },
    {
      id: 'hf-inference', provider: 'Hugging Face', icon: 'huggingface', category: 'trial_credit',
      title: 'Inference Providers 月度免费额度',
      quota: 'Free 账号 $0.10/月、PRO $2/月；OpenAI 兼容 router.huggingface.co/v1',
      docsUrl: 'https://huggingface.co/docs/inference-providers/pricing', claimUrl: 'https://huggingface.co/settings/tokens',
      lastVerifiedAt: '2026-09-25',
    },
    {
      id: 'cloudflare-workers-ai', provider: 'Cloudflare Workers AI', icon: 'cloudflare', category: 'trial_credit',
      title: '每日 10,000 Neurons 免费算力',
      quota: 'Free 与 Paid Workers 计划都是 10,000 Neurons/天；提供 OpenAI 兼容入口 /ai/v1',
      docsUrl: 'https://developers.cloudflare.com/workers-ai/platform/pricing/', claimUrl: 'https://dash.cloudflare.com',
      lastVerifiedAt: '2026-09-25',
    },
    {
      id: 'fireworks-credit', provider: 'Fireworks', icon: 'fireworks', category: 'trial_credit',
      title: '$1 免费额度',
      quota: '注册赠送 $1，用于开源模型推理',
      docsUrl: 'https://fireworks.ai/pricing', claimUrl: 'https://fireworks.ai/account',
      lastVerifiedAt: '2026-09-25',
    },
    {
      id: 'nvidia-nim', provider: 'NVIDIA NIM', icon: 'nvidia', category: 'trial_credit',
      title: '开发者计划免费试用',
      quota: '面向原型验证免费（限额以 NIM 文档为准）',
      docsUrl: 'https://www.nvidia.com/en-us/ai-data-science/nim/', claimUrl: 'https://build.nvidia.com',
      lastVerifiedAt: '2026-09-25',
    },

    // ---- 学生 / 开发者计划 ----
    {
      id: 'google-student', provider: 'Google AI Pro 学生版', icon: 'google', category: 'student_dev',
      title: '在校生免费 1 年（注意：是 Gemini 应用订阅，不是 API 额度）',
      quota: '美国 18 岁以上在校生；需资格验证与支付方式；兑换截止 2026-12-31；到期自动续费 $19.99/月',
      docsUrl: 'https://gemini.google/students/', claimUrl: 'https://gemini.google/students/',
      lastVerifiedAt: '2026-09-25',
    },
    {
      id: 'github-student', provider: 'GitHub Student Developer Pack', icon: 'github', category: 'student_dev',
      title: '学生开发者大礼包',
      quota: '含 Copilot Student、$100 Azure 额度、Codespaces 等',
      docsUrl: 'https://education.github.com/pack', claimUrl: 'https://education.github.com/pack',
      lastVerifiedAt: '2026-09-25',
    },

    // ---- 本机离线 ----
    {
      id: 'ollama-local', provider: 'Ollama', icon: 'ollama', category: 'local',
      title: '本机运行不限量（唯一真正「无限免费」的一档）',
      quota: 'OpenAI 兼容 http://localhost:11434/v1（Key 需要但被忽略）；吞吐受本机算力限制',
      docsUrl: 'https://docs.ollama.com/api/openai-compatibility', claimUrl: 'https://ollama.com/download',
      presetId: 'ollama', lastVerifiedAt: '2026-09-25',
    },

    // ---- 已退役（不删，防旧攻略） ----
    {
      id: 'github-models', provider: 'GitHub Models', icon: 'github', category: 'model_free_tier',
      title: '已完全退役——2024–2025 年「用 GitHub Models 白嫖」的说法全部失效',
      quota: '模型广场、推理 API、BYOK 已全部下线，官方文档引导迁移到 Azure AI Foundry',
      docsUrl: 'https://docs.github.com/en/github-models/about-github-models',
      retired: { at: '2026-07-30', note: '模型广场、推理 API、BYOK 全部下线' },
      lastVerifiedAt: '2026-09-25',
    },
  ],
};
