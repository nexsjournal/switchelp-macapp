import { useEffect, useState, useSyncExternalStore, type ReactElement } from 'react';
import { createPortal } from 'react-dom';
import { Check, Info, TriangleAlert, X } from 'lucide-react';
import styles from './Toast.module.css';

import { t } from '@/i18n';

export type ToastTone = 'success' | 'danger' | 'info';

type Toast = { id: number; tone: ToastTone; text: string };

/**
 * 全局提示队列。
 *
 * 为什么要有它：以前「成功 / 失败」这类一次性反馈散在各处——供应商弹窗里是弹窗底部的一条
 * 横幅、页面里是标题下面一条内联条。两者都不是「从别处冒出来的通知」该有的样子：
 * 横幅跟着布局走，用户填完一屏之后根本不会往那儿看。现在所有一次性反馈只有**一个**出口，
 * 位置由宿主统一决定，弹窗和应用常规界面共用同一个位置。
 *
 * 判别规则（写进文档，避免漂移）：
 * - **Toast**：某次动作的结果（已保存、已添加、连接成功/失败）与校验失败的原因——一次性、跟动作绑定。
 * - **页面内联条**：页面状态（加载失败、网关未启动、待应用数量）——需要常驻，直到状态本身改变。
 */
let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();

/** 同时最多显示三条：再多就把最早的一条挤掉（规范：最多 3 条）。 */
const MAX_VISIBLE = 3;
/** 停留时长：成功短、信息稍长、**失败不自动消失**（用户下一步要靠它决定怎么办）。 */
const LIFETIME: Record<ToastTone, number | null> = { success: 5000, info: 7000, danger: null };

function emit() {
  for (const listener of listeners) listener();
}

function schedule(id: number, tone: ToastTone) {
  const lifetime = LIFETIME[tone];
  if (lifetime === null) return;
  window.setTimeout(() => dismissToast(id), lifetime);
}

/** 推一条提示。任何组件都可以调用，不需要拿到宿主。 */
export function showToast(text: string, tone: ToastTone = 'success'): number {
  const id = nextId++;
  toasts = [...toasts, { id, tone, text }].slice(-MAX_VISIBLE);
  schedule(id, tone);
  emit();
  return id;
}

export function dismissToast(id: number) {
  const next = toasts.filter(item => item.id !== id);
  if (next.length === toasts.length) return;
  toasts = next;
  emit();
}

/**
 * 清掉某一档的全部提示。
 *
 * 用在「同一件事重试成功了」的时候：上一次失败的原因已经没有意义，让它继续挂在屏幕上
 * 会让人以为这次也没成。成功提示自己不需要这样清理，它有生命周期。
 */
export function dismissTone(tone: ToastTone) {
  const next = toasts.filter(item => item.tone !== tone);
  if (next.length === toasts.length) return;
  toasts = next;
  emit();
}

/** 测试用：把队列清空，避免用例之间互相看见对方的提示。 */
export function resetToasts() {
  toasts = [];
  emit();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

function snapshot(): Toast[] {
  return toasts;
}

const TONE_ICON: Record<ToastTone, ReactElement> = {
  success: <Check size={16} aria-hidden="true" />,
  danger: <TriangleAlert size={16} aria-hidden="true" />,
  info: <Info size={16} aria-hidden="true" />,
};

/**
 * 提示的宿主：悬浮在右下角，浮在弹窗之上（层级表里 toast 50 > dialog 40）。
 *
 * 用 portal 直接挂到 body 上，而不是挂在调用点所在的那棵子树里：挂在那里就会被容器的
 * `overflow` 裁掉或撑开（模型表格的「更多」菜单当初就是这么把表格撑出滚动条的）。
 *
 * 只渲染一份，放在应用壳的最外层。
 */
export function ToastHost() {
  const items = useSyncExternalStore(subscribe, snapshot, snapshot);
  // 服务端渲染/测试环境里没有 document 时不要炸。
  const [mounted, setMounted] = useState(false);
  useEffect(() => { setMounted(true); }, []);
  if (!mounted || typeof document === 'undefined') return null;

  return createPortal(
    // `aria-live` 放在容器上：新提示进来时读屏会念，但不会打断正在进行的朗读。
    <div className={styles.host} aria-live="polite" aria-label={t('common.notifications')}>
      {items.map(item => <div key={item.id} className={`${styles.toast} ${styles[item.tone]}`}
        role={item.tone === 'danger' ? 'alert' : 'status'}>
        {TONE_ICON[item.tone]}
        <span className={styles.text}>{item.text}</span>
        <button type="button" className="icon-button" aria-label={t('common.dismissNotification')}
          onClick={() => dismissToast(item.id)}><X size={15} /></button>
      </div>)}
    </div>,
    document.body,
  );
}
