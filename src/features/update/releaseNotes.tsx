import type { ReactNode } from 'react';

/**
 * 发布说明的极简渲染。
 *
 * 为什么要自己写：说明来自 Release 正文（Markdown），原样显示会把 `##`、`-`、`**` 直接摆到用户
 * 面前（第一版就是这么干的，看起来像没做完）。引一个 Markdown 库又会把整棵渲染树带进包里，
 * 而发布说明真正用到的只有四五种语法。
 *
 * 认得的：`#`~`######` 标题、`-`/`*` 无序列表、`**加粗**`、`` `行内代码` ``，空行分段。
 * **不认的按普通文字逐行显示**——清单里的说明请只用上面这个子集，表格与代码块会退化成一行行原文
 * （见 docs/architecture/06-updates.md §5）。宁可显示得朴素，也不悄悄吞掉作者写的内容。
 */

/** 行内语法：加粗与行内代码。其余原样。 */
function inline(text: string, keyPrefix: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  // 一次扫完两种标记，避免「先替换加粗再替换代码」那种顺序依赖。
  const pattern = /\*\*([^*]+)\*\*|`([^`]+)`/g;
  let cursor = 0;
  let match: RegExpExecArray | null;
  let index = 0;
  while ((match = pattern.exec(text)) !== null) {
    if (match.index > cursor) nodes.push(text.slice(cursor, match.index));
    if (match[1] !== undefined) nodes.push(<strong key={`${keyPrefix}-b${index}`}>{match[1]}</strong>);
    else nodes.push(<code key={`${keyPrefix}-c${index}`}>{match[2]}</code>);
    cursor = match.index + match[0].length;
    index += 1;
  }
  if (cursor < text.length) nodes.push(text.slice(cursor));
  return nodes;
}

export function renderReleaseNotes(text: string): ReactNode {
  const blocks: ReactNode[] = [];
  /** 累积中的列表项；遇到非列表行就收成一段 <ul>。 */
  let bullets: string[] = [];
  let key = 0;

  const flushBullets = () => {
    if (!bullets.length) return;
    blocks.push(<ul key={`ul-${key++}`}>{bullets.map((item, position) => <li key={position}>{inline(item, `li-${key}-${position}`)}</li>)}</ul>);
    bullets = [];
  };

  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trimEnd();
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    const bullet = /^[-*]\s+(.*)$/.exec(line);
    if (heading) {
      flushBullets();
      // 一律渲染成 h4：弹窗里已经有一级小标题（「这一版改了什么」），
      // 让说明里的 `#` 与 `##` 在视觉上同级，不会因为源文件层级不同而忽大忽小。
      blocks.push(<h4 key={`h-${key++}`}>{inline(heading[2]!, `h-${key}`)}</h4>);
      continue;
    }
    if (bullet) {
      bullets.push(bullet[1]!);
      continue;
    }
    flushBullets();
    if (line.trim() === '') continue;
    blocks.push(<p key={`p-${key++}`}>{inline(line, `p-${key}`)}</p>);
  }
  flushBullets();
  return blocks;
}
