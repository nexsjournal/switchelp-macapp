import type { ReactNode } from 'react';
import { Claude, Ollama, OpenAI, Github } from '@lobehub/icons';

/**
 * 工具清单里认得出品牌的那几家，用各家的真实图标（@lobehub/icons，MIT）。
 *
 * **品牌有官方彩色的用彩色版**（`Claude.Color`——珊瑚橙星芒是官方品牌色）；
 * 品牌标志本来就是黑白单色的（OpenAI / GitHub / Ollama 没有彩色变体，库里有色版
 * 会渲染不出来）保持单色继承文字色——给它们编颜色才是失真。
 * 清单里的其余工具（aider、ffmpeg、hermes 这类没有公开品牌标识或不在库里的）
 * 返回 null，列表回退到首字方块——不给工具编造 logo。
 */
type AnyIcon = (props: { size?: number }) => ReactNode;

const TOOL_ICONS: Record<string, AnyIcon> = {
  'claude-code': Claude.Color,
  codex: OpenAI,
  'github-cli': Github,
  ollama: Ollama,
};

/** 有品牌图标返回元素，没有返回 null 让调用方回退到首字方块。 */
export function toolIcon(toolId: string): ReactNode {
  const Icon = TOOL_ICONS[toolId];
  return Icon ? <Icon size={18} /> : null;
}
