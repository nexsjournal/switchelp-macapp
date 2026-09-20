import styles from './Switch.module.css';

/**
 * 二态开关（规范 36×20，docs/design/03-components.md）。
 *
 * 用 `role="switch"` 而不是 checkbox：读屏会念「开关」并直接给出开/关，
 * 与「勾选一个选项」是两件事。
 *
 * 只承载**立即生效、无需确认**的二态（启用供应商、纳入目录、智能配置）。
 * 会改外部配置或需要重启的动作不能藏在一个开关里。
 */
export function Switch({ checked, onChange, label, disabled, size = 'normal' }: {
  checked: boolean;
  onChange: (next: boolean) => void;
  /** accessible name：开关本身没有文字，名字必须由调用方给出。 */
  label: string;
  disabled?: boolean;
  /** `small` 用在列表行里，与行高对齐。 */
  size?: 'normal' | 'small';
}) {
  return <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled}
    className={`${styles.switch} ${styles[size]} ${checked ? styles.on : ''}`}
    onClick={() => onChange(!checked)}>
    <span className={styles.knob} aria-hidden="true" />
  </button>;
}
