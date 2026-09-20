import { useState } from 'react';
import { CloudUpload } from 'lucide-react';
import type { ApplyPlan, Model, Provider } from '@/contracts/types';
import { type AppliedSummary, type DesktopClient, isCoreError, toCoreError } from '@/desktop/client';
import { showToast } from '@/components/Toast';
import { ApplyConfirmDialog } from '@/features/codex/ApplyConfirmDialog';
import styles from './PendingApplyBar.module.css';

import { t } from '@/i18n';

/**
 * 待应用条（设计 P06 的 PendingApplyBar）。
 *
 * 为什么需要它：模型「已保存」和「已进 Codex 菜单」是两件事——中间必须有一次
 * 「应用」。以前这个入口只在 Codex 配置页里，用户在自己的配置页（供应商与模型）配完
 * 供应商和模型之后，盯着「编辑 / 测试 / 更多」找不到任何能生效的按钮，只能靠猜。
 *
 * 所以这条常驻在页面底部，只要还有待应用的模型就出现：
 * - 左边说清「几个模型待应用、涉及几家供应商、为什么必须重启一次」；
 * - 右边两个动作：「查看差异」去 Codex 配置页看完整事务状态，「应用并重启 Codex」就地走
 *   「生成计划 → 差异确认 → 写入 → 重启」——确认那一步不省：它要写的是 Codex 自己的配置文件，
 *   而且会重启用户的 Codex。
 */
export function PendingApplyBar({ client, providers, models, summary, onApplied, onOpenDiff }: {
  client: DesktopClient;
  providers: Provider[];
  /** 全部模型；这里自己筛「已纳入目录但还没生效」的那些。 */
  models: Model[];
  /** 已发布的配置摘要：手动交回执时要用它的 operationId。 */
  summary: AppliedSummary | null;
  /** 应用成功后重读数据。 */
  onApplied: () => Promise<void> | void;
  /** 「查看差异」：交给宿主切到 Codex 配置页。 */
  onOpenDiff: () => void;
}) {
  const [plan, setPlan] = useState<ApplyPlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const pending = models.filter(model => model.inCatalog && model.hostState !== 'loaded');
  if (!pending.length) return null;

  const scope = new Set(pending.map(model => model.providerId)).size;
  const providerName = (id: string) => providers.find(provider => provider.id === id)?.name ?? t('common.unknownProvider');
  /**
   * 全部待办都只是「等宿主回执」时，这条不该再说「待应用」——配置已经写完了，
   * 再点一次「应用并重启」只会重做一遍同样的事。这时它改说事实，并把回执交回用户。
   */
  const awaitingHost = pending.every(model => model.hostState === 'awaiting_reload') && !!summary;

  /** 生成计划 → 弹差异确认。提交在 confirmApply 里。 */
  async function planApply() {
    setBusy(true); setError('');
    try {
      const instances = await client.detectInstances();
      const instance = instances[0];
      if (!instance) { showToast(t('codex.noInstance'), 'danger'); return; }
      setPlan(await client.planApply({ instanceId: instance.id, draftRevision: '' }));
    } catch (thrown) {
      showToast(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('common.failed'), 'danger');
    } finally { setBusy(false); }
  }

  /**
   * 提交。计划带的是生成时那份配置的哈希，而 Codex 自己会写 `config.toml`
   * （启动时补 `[projects.*]`），所以过期是常态：自动重生成一次再提交，重试仍失败才报错。
   */
  async function confirmApply() {
    if (!plan) return;
    setBusy(true); setError('');
    try {
      const result = await client.executeApply({ planId: plan.id, planHash: plan.planHash, idempotencyKey: newIdempotencyKey() })
        .catch(async (thrown: unknown) => {
          if (!isCoreError(thrown) || thrown.code !== 'CONFIG_CHANGED') throw thrown;
          const instances = await client.detectInstances();
          const instance = instances[0];
          if (!instance) throw thrown;
          const fresh = await client.planApply({ instanceId: instance.id, draftRevision: '' });
          setPlan(fresh);
          showToast(t('codex.replanned'), 'info');
          return client.executeApply({ planId: fresh.id, planHash: fresh.planHash, idempotencyKey: newIdempotencyKey() });
        });
      await client.applyStatus(result.operationId);
      setPlan(null);
      await onApplied();
      // 配置写完了，但 Codex 只在启动时读它：直接重启，省掉「再点一次重启」。
      const report = await client.restartHost(await instanceIdOf(client)).catch(() => null);
      if (!report || !report.quitConfirmed || !report.launchedConfirmed) {
        showToast(t('codex.restartHostStillRunning'), 'danger');
        return;
      }
      // 重启是我们自己做的，而且退出与启动都**观察到**了——这就是宿主回执，直接记账。
      // 以前不做这一步，于是配置早已生效、Codex 也真的重新读过了，事务却永远停在
      // 「等待重载」：界面一直显示待应用，用户以为没生效。
      await client.confirmReload(result.operationId, true).catch(() => null);
      await onApplied();
      showToast(t('codex.appliedAndRestarted'));
    } catch (thrown) {
      setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('common.failed'));
    } finally { setBusy(false); }
  }

  /** 手动交回执：用于自动补记没能成立的情况（宿主没重启、平台查不到启动时间）。 */
  async function confirmHostReloaded() {
    if (!summary) return;
    setBusy(true); setError('');
    try {
      await client.confirmReload(summary.operationId, true);
      await onApplied();
      showToast(t('codex.hostReloadConfirmed'), 'info');
    } catch (thrown) {
      setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('common.failed'));
    } finally { setBusy(false); }
  }

  return <>
    <div className={styles.bar} role="region" aria-label={t('codex.pendingBarLabel')}>
      <CloudUpload size={16} aria-hidden="true" />
      <span className={styles.text}>
        {awaitingHost
          ? <><strong>{t('codex.awaitingHostTitle')}</strong>
            <span className="text-muted">{t('codex.awaitingHostBody')}</span></>
          : <><strong>{t('codex.pendingCount', { count: pending.length })}</strong>
            <span className="text-muted">{t('codex.pendingScope', { providers: scope, names: [...new Set(pending.map(model => providerName(model.providerId)))].join(t('common.itemSeparator')) })}</span></>}
      </span>
      <div className="actions">
        <button type="button" onClick={onOpenDiff}>{t('action.viewDiff')}</button>
        {awaitingHost && summary
          ? <button type="button" className="primary" disabled={busy} onClick={() => void confirmHostReloaded()}>
            {t('action.confirmHostReloaded')}
          </button>
          : <button type="button" className="primary" disabled={busy} onClick={() => void planApply()}>
            {busy && !plan ? t('codex.committing') : t('codex.applyAndRestart')}
          </button>}
      </div>
    </div>

    {plan && <ApplyConfirmDialog plan={plan} kind="apply" busy={busy} error={error} models={models}
      commitLabel={t('codex.applyAndRestart')}
      onConfirm={() => void confirmApply()} onClose={() => { setPlan(null); setError(''); }} />}
  </>;
}

/** 取当前要操作的实例：探测结果里的第一个（与 Codex 配置页的默认选中一致）。 */
async function instanceIdOf(client: DesktopClient): Promise<string> {
  const instances = await client.detectInstances();
  return instances[0]?.id ?? '';
}

function newIdempotencyKey(): string {
  return globalThis.crypto?.randomUUID?.() ?? `idem-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
