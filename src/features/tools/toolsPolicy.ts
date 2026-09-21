import type { ResolvedLocale } from '@/i18n';
import type { ToolCategory, ToolStatus } from '@/contracts/types';

/**
 * 界面语言 → 清单里的语言键。清单用 BCP-47 的 `zh-Hans`，
 * 界面用 `zh-CN`；这一处转换必须只有一份，否则两处不一致时会出现
 * 「中文界面里工具显示英文名」这种很难定位的问题。
 */
export function catalogLocale(locale: ResolvedLocale): string {
  return locale === 'zh-CN' ? 'zh-Hans' : 'en';
}

/**
 * 工具状态到徽章的映射。**只在这里写一次**：状态色散在各处时，
 * 同一个状态会在不同页面上显示成不同颜色，而那正是最难被发现的错误。
 */
export type Tone = 'success' | 'warning' | 'muted' | 'danger';

export interface Badge {
  tone: Tone;
  /** 文案键；走 t() 取。 */
  labelKey: string;
}

const STATUS_BADGES: Record<ToolStatus, Badge> = {
  // 已就绪：探针通过且清单声明的本地文件都在。
  ready: { tone: 'success', labelKey: 'tools.status.ready' },
  // 未授权：探针明确说了「没登录」。这是可操作的琥珀，不是错误。
  needsLogin: { tone: 'warning', labelKey: 'tools.status.needsLogin' },
  installed: { tone: 'muted', labelKey: 'tools.status.installed' },
  // 未验证：找到了文件但探针没通过。它**不是**已就绪，必须与绿色区分开。
  unverified: { tone: 'warning', labelKey: 'tools.status.unverified' },
  notInstalled: { tone: 'muted', labelKey: 'tools.status.notInstalled' },
  unsupportedPlatform: { tone: 'muted', labelKey: 'tools.status.unsupportedPlatform' },
};

/** 取状态徽章。未知取值按「未验证」处理——绝不默认成绿色。 */
export function statusBadge(status: ToolStatus): Badge {
  return STATUS_BADGES[status] ?? { tone: 'warning', labelKey: 'tools.status.unverified' };
}

const CATEGORY_LABELS: Record<ToolCategory, string> = {
  cliCode: 'tools.category.cliCode',
  utility: 'tools.category.utility',
  runtime: 'tools.category.runtime',
};

export function categoryLabel(category: ToolCategory): string {
  return CATEGORY_LABELS[category] ?? category;
}

/**
 * 工具名的图标底纹。
 *
 * **不发厂商 logo**：那是第三方商标，跟着版本走还得随包分发，许可与更新都不是我们能负责的；
 * 运行时抓 favicon 又会把「你装了哪些工具」泄露给外部站点，还让离线时整页没有图形。
 * 所以沿用供应商列表已经在用的做法——取名字首字符做单字底纹，同一套视觉语言，零外部依赖。
 */
export function monogram(displayName: string): string {
  const trimmed = displayName.trim();
  if (!trimmed) return '?';
  // 中文取第一个字，西文取首字母（保持大写），两类都不会因为截断而变怪。
  return trimmed.slice(0, 1).toUpperCase();
}

/**
 * 探测时间的人话。界面必须说清「这个结论是什么时候得出的」——
 * 没有时间的「已就绪」在用户刚装完工具之后就是错的信息。
 */
export function describeProbedAt(probedAt: number, now: number, locale: string): string {
  const seconds = Math.max(0, now - probedAt);
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  if (seconds < 60) return formatter.format(-seconds, 'second');
  if (seconds < 3600) return formatter.format(-Math.round(seconds / 60), 'minute');
  if (seconds < 86_400) return formatter.format(-Math.round(seconds / 3600), 'hour');
  return formatter.format(-Math.round(seconds / 86_400), 'day');
}
