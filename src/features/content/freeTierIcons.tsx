import type { ReactNode } from 'react';
import {
  Baidu, Cloudflare, Cerebras, Cohere, DeepSeek, Fireworks, Github, Google,
  Groq, HuggingFace, Hunyuan, Mistral, Moonshot, Nvidia, Ollama, OpenRouter, Qwen, SiliconCloud, Zhipu,
} from '@lobehub/icons';

/**
 * 免费额度条目的厂商图标（docs/design/09）。
 *
 * **品牌有官方彩色的用彩色版**（`.Color`：智谱蓝、HuggingFace 黄、Nvidia 绿、
 * DeepSeek 鲸鱼蓝、Google 四色等）；品牌标志本来就是黑白单色的
 * （OpenRouter / Groq / Moonshot / GitHub / Ollama，库里没有彩色变体）保持单色
 * 继承文字色——给它们编颜色才是失真。清单里没有图标的回退首字方块。
 * 图标只做识别（aria-hidden 的装饰位），厂商名是旁边的正文文字。
 */
type AnyIcon = (props: { size?: number }) => ReactNode;

const FREE_TIER_ICONS: Record<string, AnyIcon> = {
  openrouter: OpenRouter,
  zhipu: Zhipu.Color,
  qwen: Qwen.Color,
  siliconcloud: SiliconCloud.Color,
  cloudflare: Cloudflare.Color,
  cohere: Cohere.Color,
  huggingface: HuggingFace.Color,
  fireworks: Fireworks.Color,
  nvidia: Nvidia.Color,
  google: Google.Color,
  groq: Groq,
  mistral: Mistral.Color,
  cerebras: Cerebras.Color,
  hunyuan: Hunyuan.Color,
  baidu: Baidu.Color,
  deepseek: DeepSeek.Color,
  moonshot: Moonshot,
  github: Github,
  ollama: Ollama,
};

/** 有品牌图标返回元素（彩色的用官方色），没有返回 null 让调用方回退。 */
export function freeTierIcon(iconId: string | undefined): ReactNode {
  if (!iconId) return null;
  const Icon = FREE_TIER_ICONS[iconId];
  return Icon ? <Icon size={18} /> : null;
}
