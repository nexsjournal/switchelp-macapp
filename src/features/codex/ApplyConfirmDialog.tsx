import { useMemo } from 'react';
import { AlertTriangle } from 'lucide-react';
import type { ApplyPlan, FieldChange } from '@/contracts/types';
import { Dialog } from '@/components/Dialog';
import styles from './ApplyConfirmDialog.module.css';

import { t } from '@/i18n';

/** 差异分组顺序：先路由，再目录，最后策略。与核心的 reasonKey 前缀对应。 */
const groupOrder = ['route', 'catalog', 'policy', 'restore', 'other'] as const;

function groupOf(reasonKey: string): typeof groupOrder[number] {
  switch (reasonKey) {
    case 'reason.defaultModel':
    case 'reason.providerRoute':
      return 'route';
    // provider 子表跟着目录一起展示：它是「这次发布要带上的网关入口」，口径沿用原实现。
    case 'reason.catalog':
    case 'reason.gatewayProvider':
      return 'catalog';
    case 'reason.contextOverride':
    case 'reason.reasoningDefault':
      return 'policy';
    case 'reason.restore':
      return 'restore';
    default:
      return 'other';
  }
}

/** 界面只显示人话：reasonKey 是内部标识，不往界面上抛。 */
function reasonLabel(reasonKey: string): string {
  const short = reasonKey.startsWith('reason.') ? reasonKey.slice('reason.'.length) : reasonKey;
  const label = t(`reason.${short}`);
  return label === `reason.${short}` ? t('reason.other') : label;
}

function groups(changes: FieldChange[]) {
  const map = new Map<string, FieldChange[]>();
  for (const change of changes) {
    const key = groupOf(change.reasonKey);
    map.set(key, [...(map.get(key) ?? []), change]);
  }
  return groupOrder.filter(key => map.has(key)).map(key => ({ key, changes: map.get(key)! }));
}

/**
 * 编译警告是 `键：细节` 的字符串。前半段可能是核心给的 messageKey，也可能是编译期拼的自由文本：
 * messageKey 必须翻成人话（翻不出来就退到通用标签，绝不把 `warning.xxx` 摆到界面上），
 * 自由文本原样显示。
 */
function warningOf(raw: string): { label: string; detail: string } {
  const separator = raw.indexOf('：');
  // 没有分隔符：整条就是自由文本，配一个通用标签让它看起来仍是一条「警告」。
  if (separator < 0) return { label: t('warning.other'), detail: raw };
  const head = raw.slice(0, separator);
  const detail = raw.slice(separator + 1);
  // 自由文本（不是 messageKey 的）原样当标签；messageKey 翻不出来就退到通用标签，
  // 绝不把 `warning.xxx` 摆到用户面前。
  if (!/^(warning|error|reason|probe|stage|advice)\./.test(head)) return { label: head, detail };
  const translated = t(head);
  return { label: translated === head ? t('warning.other') : translated, detail };
}

/**
 * 写入前的差异确认弹窗。
 *
 * 应用与还原共用：它只负责「让人看清要改什么，然后确认或取消」，
 * 提交逻辑（写入、重启宿主、结论）留在各自的调用方——两条路要报告的结果不一样。
 *
 * 为什么是模态：以前这块渲染在页面操作行**之下**，用户点完按钮不往下滚就看不到确认入口，
 * 于是以为操作没生效（真机上就发生过：点完「还原」又去点了「重启 Codex」）。
 */
export function ApplyConfirmDialog({ plan, kind, busy, error, commitLabel, onConfirm, onClose }: {
  plan: ApplyPlan;
  kind: 'apply' | 'restore';
  busy: boolean;
  error?: string;
  commitLabel: string;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const diff = useMemo(() => groups(plan.changes), [plan]);
  const conflicts = plan.warnings.filter(raw => raw.includes('已被外部修改'));

  return <Dialog width="wide" busy={busy}
    title={kind === 'apply' ? t('codex.diffTitleApply') : t('codex.diffTitleRestore')}
    description={t('codex.changeCount', { count: plan.changes.length })}
    onClose={onClose}
    footer={<footer className="form-footer">
      <span>{busy ? t('codex.commitNotCancellable') : t('codex.casNote')}</span>
      <div className="actions">
        <button onClick={onClose} disabled={busy}>{t('action.cancel')}</button>
        <button className="primary" autoFocus onClick={onConfirm} disabled={busy}>
          {busy ? t('codex.committing') : commitLabel}
        </button>
      </div>
    </footer>}>
    <div className="form-fields">
      <p className="field-hint">{t('codex.targetFile')}<span className="text-mono break-anywhere">{plan.configPath}</span></p>
      {plan.changes.length === 0
        ? <p className="field-hint">{t('codex.noFieldDiff')}</p>
        : diff.map(group => <div key={group.key} className={styles.group}>
          <h3>{t(`group.${group.key}`)}<span className="badge">{group.changes.length}</span></h3>
          <table className={styles.changes}>
            <thead><tr>
              <th scope="col">{t('codex.changeField')}</th>
              <th scope="col">{t('codex.changeBefore')}</th>
              <th scope="col">{t('codex.changeAfter')}</th>
              <th scope="col">{t('codex.changeReason')}</th>
            </tr></thead>
            <tbody>{group.changes.map(change => <tr key={change.keyPath}>
              <td className="text-mono">{change.keyPath}</td>
              <td><code className="text-muted break-anywhere">{change.before ?? t('codex.notSet')}</code></td>
              <td><code className="break-anywhere">{change.after ?? t('codex.willBeDeleted')}</code></td>
              <td className="text-muted">{reasonLabel(change.reasonKey)}</td>
            </tr>)}</tbody>
          </table>
        </div>)}
      {plan.warnings.length > 0 && <div className={styles.warnings}>
        <AlertTriangle size={15} aria-hidden="true" />
        <div>
          <strong>{conflicts.length ? t('codex.conflictWarnings') : t('codex.compileWarnings')}</strong>
          <ul>{plan.warnings.map(raw => {
            const { label, detail } = warningOf(raw);
            // 标签单独成元素：正文里再出现同一个词也不影响「标签可读」这条断言，
            // 读屏也知道哪一段是分类、哪一段是细节。
            return <li key={raw}>{label && <><strong>{label}</strong>{t('common.labelSeparator')}</>}{detail}</li>;
          })}</ul>
        </div>
      </div>}
      <p className="field-hint">{t('codex.applyRestartsHost')}</p>
      {error && <div role="alert" className="error-message">{error}</div>}
    </div>
  </Dialog>;
}
