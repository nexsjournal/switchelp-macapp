import * as Primitive from '@radix-ui/react-dialog';
import { useRef, useState, type ReactNode } from 'react';
import { X } from 'lucide-react';
import styles from './Dialog.module.css';

import { t } from '@/i18n';
/**
 * 弹窗骨架：标题固定、正文滚动、页脚固定。
 *
 * `footer` 是滚动区**之外**的底栏。表单型弹窗把保存按钮放这里，而不是写在 `<form>` 末尾：
 * 写在表单末尾时页脚属于滚动内容，长表单一打开就会盖住最后几个字段。
 */
export function Dialog({ title, description, onClose, dirty = false, busy = false, leadingIcon, headerActions, width = 'normal', children, footer }: {
  /**
   * 标题。可以是元素：供应商弹窗把**名称输入框**放在这里（标题本身就是这个名字），
   * 那种情况下调用方要在标题里额外放一段 visually-hidden 的文字，
   * 否则标题元素里没有文本，弹窗就没有 accessible name 了。
   */
  title: ReactNode;
  /**
   * 说明写在标题下面。可选：像供应商弹窗那样「标题自己就是输入框」的表单，
   * 再挂一段解释只会把首屏推下去，关键提示留给字段自己的 hint。
   */
  description?: string;
  onClose: () => void; dirty?: boolean; busy?: boolean;
  /** 标题左侧的图形标识（供应商弹窗用它标出这是哪一种条目）。 */
  leadingIcon?: ReactNode;
  /** 标题右侧、关闭按钮左侧的控件（启用开关、更多菜单）。 */
  headerActions?: ReactNode;
  width?: 'narrow' | 'normal' | 'wide';
  children?: ReactNode; footer?: ReactNode;
}) {
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const previousFocus = useRef(document.activeElement as HTMLElement | null);
  const close = () => { if (!busy) { if (dirty) setConfirmDiscard(true); else onClose(); } };
  return <Primitive.Root open onOpenChange={open => { if (!open) close(); }}>
    <Primitive.Portal>
      <Primitive.Overlay className={styles.overlay} />
      <Primitive.Content className={`${styles.dialog} ${styles[width]}`} onCloseAutoFocus={event => {
        event.preventDefault(); previousFocus.current?.focus();
      }} onInteractOutside={event => event.preventDefault()}>
        <header className={styles.header}>
          <div className={styles.heading}>
            {leadingIcon && <span className={styles.leadingIcon} aria-hidden="true">{leadingIcon}</span>}
            <Primitive.Title className="text-section-title">{title}</Primitive.Title>
            {description && <Primitive.Description className={styles.description}>{description}</Primitive.Description>}
          </div>
          <div className={styles.headerActions}>{headerActions}
            <button className="icon-button" aria-label={t('common.close')} onClick={close} disabled={busy}><X size={18} /></button>
          </div>
        </header>
        {confirmDiscard && <div className={styles.discard}>
          <p>{t('editor.discardBody')}</p>
          <div className="actions"><button onClick={() => setConfirmDiscard(false)} autoFocus>{t('editor.keepEditing')}</button>
            <button className="danger" onClick={onClose}>{t('editor.discardTitle')}</button></div>
        </div>}
        <div className={styles.body} hidden={confirmDiscard}>{children}</div>
        {footer && <div className={styles.footer} hidden={confirmDiscard}>{footer}</div>}
      </Primitive.Content>
    </Primitive.Portal>
  </Primitive.Root>;
}
