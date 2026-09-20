import { CircleHelp } from 'lucide-react';
import styles from './FieldHelp.module.css';

/**
 * 字段标签旁的「?」。
 *
 * 它承载的是「这个值最终落在哪里 / 会产生什么后果」，不是必填说明——
 * 必填靠 `required` 与提交时报错表达。鼠标悬停有原生 tooltip，
 * 读屏会把它当成一张带说明的图片念出来，所以这段文字必须能独立读懂。
 */
export function FieldHelp({ text }: { text: string }) {
  return <span className={styles.help} role="img" aria-label={text} title={text}><CircleHelp size={14} /></span>;
}
