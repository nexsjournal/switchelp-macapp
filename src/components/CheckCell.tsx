import { Lock } from 'lucide-react';
import styles from './CheckCell.module.css';

/**
 * 能力声明的勾选单元格（对齐参考界面的「输入类型 / 模型能力」）。
 *
 * 为什么是勾选而不是三态下拉：这里问的是「这个模型支不支持这一项」，
 * 勾上是支持、取消是不支持。**没点过的项不属于这两者**——调用方用状态记号
 * 保存「用户是否点过」，没点过的项在保存时原样写回，所以界面上不必多摆一个
 * 「未知」选项（那个选项既看不懂，也会让人以为必须逐项做决定）。
 *
 * `locked` 是「不可协商」的项（文本）：勾着、锁着、点不动。
 * `disabled` 是「当前链路发不出去」的项（PDF、视频）：可见、灰态、点不动，
 * 需求 R22 要求它们可见但不启用，而不是藏起来。
 *
 * 方框与对勾不在这里画：那是全局唯一的那一套（`global.css` 的 `input[type='checkbox']`，
 * 对勾用 `--checkbox-tick`）。这个组件只负责把「方框 + 文字 + 可选的小锁」装进一个
 * 带描边的单元格里——整页编辑器、添加模型弹窗、诊断包范围因此共用同一个形状与同一个对勾。
 */
export function CheckCell({ label, hint, checked, disabled = false, locked = false, onChange }: {
  label: string;
  /** 解释这一项的含义与后果：写在 label 的 title 上，也是读屏读到的说明。 */
  hint?: string;
  checked: boolean;
  disabled?: boolean;
  locked?: boolean;
  onChange?: (next: boolean) => void;
}) {
  return <label className={disabled ? `${styles.cell} ${styles.blocked}` : styles.cell} title={hint}>
    <input type="checkbox" checked={checked} disabled={disabled}
      onChange={event => onChange?.(event.target.checked)} />
    <span>{label}</span>
    {locked && <Lock size={12} aria-hidden="true" />}
  </label>;
}

/** 一行勾选单元格。gap 与换行由容器负责，调用方只管往里放。 */
export function CheckCells({ children }: { children: React.ReactNode }) {
  return <div className={styles.cells}>{children}</div>;
}
