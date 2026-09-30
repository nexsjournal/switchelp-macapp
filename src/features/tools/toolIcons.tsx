import type { ReactNode } from 'react';
import { Claude, Gemini, Goose, Ollama, OpenAI, Qwen, Github } from '@lobehub/icons';
import { OpenClawIcon, OpenCodeIcon } from './brandIcons';

/**
 * 工具清单里认得出品牌的那几家，用各家的真实图标（@lobehub/icons，MIT）。
 *
 * **品牌有官方彩色的用彩色版**（`Claude.Color`——珊瑚橙星芒是官方品牌色；
 * Gemini / Qwen 同理）；品牌标志本来就是黑白单色的
 * （OpenAI / GitHub / Ollama 没有彩色变体，库里有色版会渲染不出来）保持单色继承文字色
 * ——给它们编颜色才是失真。Goose 属于后者：v1 里这个品牌**只有单色版**
 * （没有 `.Color` / `.BrandColor`），所以和上面几家一样继承文字色。
 *
 * opencode / openclaw 不在 v1 里，路径本地内联在 brandIcons.tsx（取自 MIT 的 v5，
 * 依赖不升版，来源与许可写在那份文件的头部）。
 *
 * **hermes 有官方图形但不在这儿用**：v5 里那个 `HermesAgent` 是一幅人物肖像插画
 * （不是几何标记），实测在 32px 方块里的 18px 尺寸下只剩一团灰糊——
 * 认不出来就不算图标。aider / crush 与 ffmpeg 这类则根本没有公开可用的符号级标志。
 * 这些一律返回 null，列表回退到首字方块——不给工具编造 logo，也不用认不出的图形凑数。
 */
type AnyIcon = (props: { size?: number }) => ReactNode;

const TOOL_ICONS: Record<string, AnyIcon> = {
  'claude-code': Claude.Color,
  codex: OpenAI,
  'gemini-cli': Gemini.Color,
  'github-cli': Github,
  goose: Goose,
  ollama: Ollama,
  openclaw: OpenClawIcon,
  opencode: OpenCodeIcon,
  'qwen-code': Qwen.Color,
};

/** 有品牌图标返回元素，没有返回 null 让调用方回退到首字方块。 */
export function toolIcon(toolId: string): ReactNode {
  const Icon = TOOL_ICONS[toolId];
  return Icon ? <Icon size={18} /> : null;
}
