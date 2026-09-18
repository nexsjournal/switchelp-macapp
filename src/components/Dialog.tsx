import * as Primitive from '@radix-ui/react-dialog';
import { useRef, useState, type ReactNode } from 'react';
import { X } from 'lucide-react';
import styles from './Dialog.module.css';

import { t } from '@/i18n';
export function Dialog({ title, description, onClose, dirty = false, busy = false, children }: {
  title: string; description: string; onClose: () => void; dirty?: boolean; busy?: boolean; children: ReactNode;
}) {
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const previousFocus = useRef(document.activeElement as HTMLElement | null);
  const close = () => { if (!busy) { if (dirty) setConfirmDiscard(true); else onClose(); } };
  return <Primitive.Root open onOpenChange={open => { if (!open) close(); }}>
    <Primitive.Portal>
      <Primitive.Overlay className={styles.overlay} />
      <Primitive.Content className={styles.dialog} onCloseAutoFocus={event => {
        event.preventDefault(); previousFocus.current?.focus();
      }} onInteractOutside={event => event.preventDefault()}>
        <header className={styles.header}>
          <div><Primitive.Title className="text-section-title">{title}</Primitive.Title>
            <Primitive.Description className={styles.description}>{description}</Primitive.Description></div>
          <button className="icon-button" aria-label={t('common.close')} onClick={close} disabled={busy}><X size={18} /></button>
        </header>
        {confirmDiscard && <div className={styles.discard}>
          <p>{t('editor.discardBody')}</p>
          <div className="actions"><button onClick={() => setConfirmDiscard(false)} autoFocus>{t('editor.keepEditing')}</button>
            <button className="danger" onClick={onClose}>{t('editor.discardTitle')}</button></div>
        </div>}
        <div hidden={confirmDiscard}>{children}</div>
      </Primitive.Content>
    </Primitive.Portal>
  </Primitive.Root>;
}
