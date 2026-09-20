import { Check, Lock } from 'lucide-react';
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
 * 勾选框是自绘的：全局那套 `accent-color` + 透明边框撑点击区的写法，在本项目里
 * 直接把方框画成了实心块（浅色主题下看着像已经勾上）。自绘之后两种主题都是
 * 同样的「空框 / 实心框 + 勾」，不依赖各引擎对原生控件的各自实现。
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
    <span className={styles.box}>
      <input type="checkbox" checked={checked} disabled={disabled}
        onChange={event => onChange?.(event.target.checked)} />
      <Check size={12} className={styles.tick} aria-hidden="true" />
    </span>
    <span>{label}</span>
    {locked && <Lock size={12} aria-hidden="true" />}
  </label>;
}

/** 一行勾选单元格。gap 与换行由容器负责，调用方只管往里放。 */
export function CheckCells({ children }: { children: React.ReactNode }) {
  return <div className={styles.cells}>{children}</div>;
}
