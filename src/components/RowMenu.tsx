import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { MoreHorizontal } from 'lucide-react';
import styles from './RowMenu.module.css';

/**
 * 行内「更多」菜单。
 *
 * 为什么不用第三个按钮：表格里并排三个按钮会把操作列撑到比数据列还宽，
 * 或者被迫竖排把行高拉成两块。设计规范也是主操作 + 次操作 + 菜单。
 *
 * 菜单渲染在 **body 上的 portal** 里，位置用 `position: fixed` 由触发按钮的矩形算出来。
 * 这不是洁癖：菜单以前是就地绝对定位的，于是任何一个 `overflow` 容器都会出事——
 * 模型表格的 `.tableWrap { overflow-x: auto }` 一打开菜单就被撑出上下滚动条，
 * 弹窗的 `overflow: hidden` 则会把它裁掉。挂到 body 上，容器怎么设都不再影响它。
 *
 * 键盘行为：`Enter`/`Space` 打开，方向键在项间移动，`Escape` 关闭并把焦点还给触发按钮，
 * 点击外部或滚动关闭。
 */
export function RowMenu({ label, items }: {
  label: string;
  items: { key: string; label: string; onSelect: () => void; danger?: boolean; disabled?: boolean; hint?: string }[];
}) {
  const [open, setOpen] = useState(false);
  /** 菜单的固定定位坐标：以触发按钮为锚点，算完再渲染，免得先画在错误位置再跳一下。 */
  const [anchor, setAnchor] = useState<{ top: number; right: number } | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!open) { setAnchor(null); return; }
    const button = trigger.current;
    if (!button) return;
    const rect = button.getBoundingClientRect();
    const height = items.length * 34 + 12;
    // 下面放不下就翻到上面：菜单从窗口底部弹出去，同样等于看不见。
    const below = rect.bottom + 6 + height <= window.innerHeight;
    setAnchor({ top: below ? rect.bottom + 6 : Math.max(8, rect.top - 6 - height), right: Math.max(8, window.innerWidth - rect.right) });
    // items 只用来估高度，内容变化由 key 变化覆盖；这里不重复触发。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, items.length]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (root.current?.contains(target) || menu.current?.contains(target)) return;
      setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        setOpen(false);
        trigger.current?.focus();
      }
    };
    // 滚动与缩放都会让锚点失效：关掉，比画在错位置强。
    const onMove = () => setOpen(false);
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    // 捕获阶段：任何一个祖先容器滚动都要关掉，不只是 window 自己。
    window.addEventListener('scroll', onMove, true);
    window.addEventListener('resize', onMove);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('scroll', onMove, true);
      window.removeEventListener('resize', onMove);
    };
  }, [open]);

  useEffect(() => {
    if (open && anchor) menu.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  }, [open, anchor]);

  function move(delta: number) {
    const buttons = [...(menu.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
    if (!buttons.length) return;
    const index = buttons.findIndex(button => button === document.activeElement);
    const next = (index + delta + buttons.length) % buttons.length;
    buttons[next]?.focus();
  }

  return <div className={styles.root} ref={root}>
    <button ref={trigger} className="icon-button" aria-label={label} aria-haspopup="menu" aria-expanded={open}
      onClick={() => setOpen(value => !value)}><MoreHorizontal size={18} /></button>
    {open && anchor && createPortal(<div className={styles.menu} role="menu" ref={menu}
      // `pointerEvents` 写在内联样式里（CSS 里也有一份）：模态弹窗打开时 Radix 会把
      // body 设成 pointer-events: none，只给它自己的内容开例外，而菜单挂在 body 上。
      // 内联样式同时让 jsdom 下的用例也走同一条路径。
      style={{ top: anchor.top, right: anchor.right, pointerEvents: 'auto' }}
      onKeyDown={event => {
        if (event.key === 'ArrowDown') { event.preventDefault(); move(1); }
        if (event.key === 'ArrowUp') { event.preventDefault(); move(-1); }
      }}>
      {items.map(item => <button key={item.key} role="menuitem" type="button"
        className={item.danger ? styles.danger : undefined}
        disabled={item.disabled} title={item.hint}
        onClick={() => { setOpen(false); item.onSelect(); }}>{item.label}</button>)}
    </div>, document.body)}
  </div>;
}

/** 供表格上方或详情页复用的行内提示条。 */
export function InlineHint({ children }: { children: ReactNode }) {
  return <p className={styles.hint}>{children}</p>;
}
