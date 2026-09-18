import type { ComponentType, ReactNode } from 'react';
import styles from './EmptyState.module.css';

/**
 * 统一的空状态。
 *
 * 之前每个页面各写一份，图标大小、留白、按钮位置都不一致，看起来像不同人做的。
 * 这里定成一种形态：淡底圆角图标 → 标题 → 一句说明 → 可选动作。
 *
 * 两条约束：
 * - 图标只用 20px 并放在柔和底上，不用 28px 大图标占掉半屏；
 * - 动作按钮是可选的，没有可执行的下一步时就不放按钮，不放一个点了没反应的占位。
 */
export function EmptyState({ icon: Icon, title, description, action, tone = 'neutral' }: {
  icon?: ComponentType<{ size?: number | string }>;
  title: string;
  description?: string;
  action?: ReactNode;
  /** `warning` 用于“有问题需要处理”的空态，其余用中性。 */
  tone?: 'neutral' | 'warning';
}) {
  return <div className={`${styles.empty} ${tone === 'warning' ? styles.warning : ''}`}>
    {Icon && <span className={styles.icon}><Icon size={20} /></span>}
    <h3 className={styles.title}>{title}</h3>
    {description && <p className={styles.description}>{description}</p>}
    {action && <div className={styles.action}>{action}</div>}
  </div>;
}
