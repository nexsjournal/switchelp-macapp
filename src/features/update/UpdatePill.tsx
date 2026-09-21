import { t } from '@/i18n';

import styles from './UpdatePill.module.css';

/**
 * 侧栏左上角的更新入口。**只在有更新时出现**，占的是副标题那一行：
 * 按钮和副标题共用同一个位置，所以出现按钮不会把侧栏内容整体推下去
 * （见 docs/architecture/06-updates.md 的界面一节）。
 *
 * 尺寸是刻意的小：它顶掉的是一行 12px 的小字，视觉上就该是同一档（胶囊在内层 span 上），
 * 点击区由外层按钮撑到 32px，符合「点击目标 ≥32×32」的版面要求。
 */
export function UpdatePill({ version, onClick }: { version: string; onClick: () => void }) {
  return <button type="button" className={styles.pill} onClick={onClick}>
    <span className={styles.label}>{t('update.pill', { version })}</span>
  </button>;
}
