import { useState, type ReactNode } from 'react';
import { Info, TriangleAlert } from 'lucide-react';
import { t } from '@/i18n';
import styles from './Notice.module.css';

/**
 * 小提醒：一行摘要常驻，详情点开才看。
 *
 * 为什么要有它：抓取失败、接口限额这类**背景状态**以前整块铺在页面上——一屏里最显眼的是
 * 用户此刻做不了什么的报错，而且每次进这一页都再铺一遍。它既不该走 Toast（Toast 是一次性
 * 动作的结果，会自己消失），也不该一直占着版面。所以：
 *
 * - 折叠时只有一行：图标 + 摘要 + 「详情」；
 * - 点开才给完整原因与可做的事（重试、填令牌这类 `actions`）。
 *
 * 摘要要能独立成立（「1 个源抓取失败」），详情才放逐条原因——这样折叠状态下也是可读的事实，
 * 而不是一个「出错了，点开看看」的黑箱。
 */
export function Notice({ tone = 'warning', summary, details, actions }: {
  /** `warning` 用于需要留意但不必立刻处理的状态；`info` 用于纯告知。 */
  tone?: 'warning' | 'info';
  /** 常驻的一行摘要。 */
  summary: ReactNode;
  /** 点开后显示的详情；与 `actions` 都没有时不出「详情」按钮，只留一行。 */
  details?: ReactNode;
  /** 详情里的动作按钮（重试、设置令牌这类）。 */
  actions?: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const expandable = Boolean(details || actions);

  return (
    <section className={`${styles.notice} ${tone === 'warning' ? styles.warning : styles.info}`}>
      <div className={styles.line}>
        {tone === 'warning'
          ? <TriangleAlert size={14} aria-hidden="true" />
          : <Info size={14} aria-hidden="true" />}
        <span className={styles.summary}>{summary}</span>
        {expandable && (
          <button type="button" className={`${styles.toggle} text-button`} aria-expanded={open}
            onClick={() => setOpen(current => !current)}>
            {open ? t('notice.hide') : t('notice.details')}
          </button>
        )}
      </div>
      {open && (
        <div className={styles.details}>
          {details}
          {actions && <div className="actions">{actions}</div>}
        </div>
      )}
    </section>
  );
}
