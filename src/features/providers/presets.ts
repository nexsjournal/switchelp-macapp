import type { Provider, ProviderPreset } from '@/contracts/types';

/**
 * 一条常用供应商预设 = 契约里的 ProviderPreset，外加两件前端自己的事：
 * 显示名走 locale（同一家的中英叫法不同：智谱 GLM / Zhipu GLM），
 * 以及一个可选的认证方式——绝大多数公开端点用 API Key，只有本机服务（Ollama）天生不需要。
 * `name` 保留契约要求的规范名（拉丁写法），界面上显示的是 `nameKey` 那份文案。
 */
export type WellKnownPreset = ProviderPreset & { nameKey: string; authKind?: Provider['authKind'] };

/**
 * 常用供应商的公开 OpenAI 兼容入口。
 *
 * 与核心的约定一致：预设只填公开 Endpoint 与协议建议，**不预填真实 Key**——
 * Key 永远由用户自己填。地址是各家文档里的原始值，填错用户就连不上，
 * 所以每条都要有出处：DeepSeek / Kimi / 智谱 / 千问 / OpenRouter / 硅基流动
 * 是各家文档写明的主入口，MiniMax 与 MiMo 的地址在 2026-09 对照过官方接入说明。
 */
export const PROVIDER_PRESETS: WellKnownPreset[] = [
  { id: 'deepseek', name: 'DeepSeek', nameKey: 'providers.preset.deepseek', baseUrl: 'https://api.deepseek.com/v1', protocol: 'chat_completions' },
  { id: 'kimi', name: 'Kimi', nameKey: 'providers.preset.kimi', baseUrl: 'https://api.moonshot.cn/v1', protocol: 'chat_completions' },
  { id: 'zhipu', name: 'Zhipu GLM', nameKey: 'providers.preset.zhipu', baseUrl: 'https://open.bigmodel.cn/api/paas/v4', protocol: 'chat_completions' },
  { id: 'qwen', name: 'Qwen', nameKey: 'providers.preset.qwen', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', protocol: 'chat_completions' },
  { id: 'minimax', name: 'MiniMax', nameKey: 'providers.preset.minimax', baseUrl: 'https://api.minimaxi.com/v1', protocol: 'chat_completions' },
  { id: 'mimo', name: 'MiMo', nameKey: 'providers.preset.mimo', baseUrl: 'https://api.xiaomimimo.com/v1', protocol: 'chat_completions' },
  { id: 'openrouter', name: 'OpenRouter', nameKey: 'providers.preset.openrouter', baseUrl: 'https://openrouter.ai/api/v1', protocol: 'chat_completions' },
  { id: 'siliconflow', name: 'SiliconFlow', nameKey: 'providers.preset.siliconflow', baseUrl: 'https://api.siliconflow.cn/v1', protocol: 'chat_completions' },
  // 本机服务没有密钥可填：预设直接把认证方式切成无认证（无认证只对 loopback 开放，localhost 正好满足）。
  { id: 'ollama', name: 'Ollama', nameKey: 'providers.preset.ollama', baseUrl: 'http://localhost:11434/v1', protocol: 'chat_completions', authKind: 'none' },
];
