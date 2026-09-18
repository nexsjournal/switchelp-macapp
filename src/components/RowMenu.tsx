import { useEffect, useRef, useState, type ReactNode } from 'react';
import { MoreHorizontal } from 'lucide-react';
import styles from './RowMenu.module.css';

/**
 * 行内「更多」菜单。
 *
 * 为什么不用第三个按钮：表格里并排三个按钮会把操作列撑到比数据列还宽，
 * 或者被迫竖排把行高拉成两块。设计规范也是主操作 + 次操作 + 菜单。
 *
 * 键盘行为：`Enter`/`Space` 打开，方向键在项间移动，`Escape` 关闭并把焦点还给触发按钮，
 * 点击外部关闭。
 */
export function RowMenu({ label, items }: {
  label: string;
  items: { key: string; label: string; onSelect: () => void; danger?: boolean; disabled?: boolean; hint?: string }[];
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        setOpen(false);
        trigger.current?.focus();
      }
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [open]);

  useEffect(() => {
    if (open) menu.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  }, [open]);

  function move(delta: number) {
    const buttons = [...(menu.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
    if (!buttons.length) return;
    const index = buttons.findIndex(button => button === document.activeElement);
    const next = (index + delta + buttons.length) % buttons.length;
    buttons[next]?.focus();
  }

  return <div className={styles.root} ref={root}>
    <button ref={trigger} className="icon-button" aria-label={label} aria-haspopup="menu" aria-expanded={open}
      onClick={() => setOpen(value => !value)}><MoreHorizontal size={17} /></button>
    {open && <div className={styles.menu} role="menu" ref={menu}
      onKeyDown={event => {
        if (event.key === 'ArrowDown') { event.preventDefault(); move(1); }
        if (event.key === 'ArrowUp') { event.preventDefault(); move(-1); }
      }}>
      {items.map(item => <button key={item.key} role="menuitem" type="button"
        className={item.danger ? styles.danger : undefined}
        disabled={item.disabled} title={item.hint}
        onClick={() => { setOpen(false); item.onSelect(); }}>{item.label}</button>)}
    </div>}
  </div>;
}

/** 供表格上方或详情页复用的行内提示条。 */
export function InlineHint({ children }: { children: ReactNode }) {
  return <p className={styles.hint}>{children}</p>;
}
