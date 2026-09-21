import { useEffect, useState } from 'react';
import { Dialog } from '@/components/Dialog';
import { showToast } from '@/components/Toast';
import { type DesktopClient, type UpdateProgress, type UpdateReport, toCoreError } from '@/desktop/client';
import { renderReleaseNotes } from './releaseNotes';
import { t } from '@/i18n';

import styles from './UpdateDialog.module.css';

type Phase = 'idle' | 'downloading' | 'installing' | 'failed';

/** 字节数说成人话：进度旁边不该出现 5457549 这种数字。 */
function megabytes(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * 更新弹窗。
 *
 * 版面沿用项目里确认过的弹窗写法：正文走 `.form-fields`（24 内边距 + 16 网格间距），
 * 底栏走 Dialog 的 `footer` 槽（`.form-footer`），错误走全局 `.error-message`。
 * 第一版没走这些类，于是内容贴边、间距靠手写、底栏那句提示挤成三行——按规范重做的原因见
 * docs/architecture/06-updates.md §6.2。
 *
 * 两个刻意的取舍：
 * - 下载**没有**「取消」按钮。插件的下载不支持中断，摆一个按下去什么都不停的按钮就是假开关；
 *   所以这里明说「关闭窗口不会中断下载」，用户想走就走。
 * - 成功路径没有收尾界面：安装完成即重启，窗口随之消失。安装结果由启动流程读标记文件后
 *   用一条 Toast 告知（见 App.tsx）。
 */
export function UpdateDialog({ client, report, onClose }: {
  client: DesktopClient;
  report: UpdateReport;
  onClose: () => void;
}) {
  const [phase, setPhase] = useState<Phase>('idle');
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [failure, setFailure] = useState('');
  const [failureDetail, setFailureDetail] = useState('');

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let alive = true;
    void client.onUpdateProgress(next => setProgress(next))
      .then(stop => { if (alive) unsubscribe = stop; else stop(); })
      // 没有事件通道（浏览器夹具）就退化成不确定态：进度不是关键路径，不报错。
      .catch(() => undefined);
    return () => { alive = false; unsubscribe?.(); };
  }, [client]);

  /**
   * 下载并安装。**成功时这个 Promise 不会 resolve**：装完立刻重启，窗口随之消失。
   * 所以 catch 是失败路径，正常路径的收尾不在这里。
   */
  async function install() {
    setPhase('downloading'); setFailure(''); setFailureDetail('');
    try {
      await client.installUpdate();
      setPhase('installing');
    } catch (thrown) {
      const error = toCoreError(thrown);
      setFailure(t(error.messageKey));
      setFailureDetail(error.safeDetails.join(t('common.listSeparator')));
      setPhase('failed');
    }
  }

  const busy = phase === 'downloading' || phase === 'installing';
  const downloaded = progress?.downloaded ?? 0;
  const total = progress?.total ?? null;
  const ratio = total && total > 0 ? Math.min(1, downloaded / total) : null;

  return <Dialog width="normal" title={t('update.title')} onClose={onClose}
    description={t('update.versionChange', { current: report.current, latest: report.latest ?? '' })}
    footer={<footer className="form-footer">
      <span>{phase === 'failed' ? t('update.failedHint') : phase === 'idle' ? t('update.restartHint') : t('update.closeHint')}</span>
      <div className="actions">
        {phase === 'idle' && <>
          <button onClick={onClose}>{t('update.later')}</button>
          <button className="primary" autoFocus onClick={() => void install()}>{t('update.install')}</button>
        </>}
        {phase === 'downloading' && <button onClick={onClose}>{t('common.close')}</button>}
        {phase === 'installing' && <button disabled>{t('update.installing')}</button>}
        {phase === 'failed' && <>
          {report.releaseUrl && <button onClick={() => {
            void client.openReleasePage(report.releaseUrl!).catch(() => showToast(t('update.openFailed'), 'danger'));
          }}>{t('update.manual')}</button>}
          <button onClick={onClose}>{t('common.close')}</button>
          <button className="primary" autoFocus disabled={busy} onClick={() => void install()}>{t('update.retry')}</button>
        </>}
      </div>
    </footer>}>
    <div className="form-fields">
      {/* 我们在哪一版、要去哪一版：装错版本比装不上更麻烦，所以这几行一直在。
          标签在左、值在右，与设置页的状态行同一读法。 */}
      <dl className={styles.facts}>
        <dt>{t('update.currentVersion')}</dt><dd>{report.current}</dd>
        <dt>{t('update.latestVersion')}</dt><dd>{report.latest ?? '—'}</dd>
        {report.publishedAt && <><dt>{t('update.publishedAt')}</dt><dd>{report.publishedAt}</dd></>}
      </dl>

      {phase === 'failed' && <div role="alert" className="error-message">
        <strong>{failure}</strong>
        {failureDetail && <p>{failureDetail}</p>}
      </div>}

      {busy && <div className={styles.progress} role="status" aria-live="polite">
        <span className={styles.progressLabel}>
          {phase === 'downloading' ? t('update.downloading') : t('update.installing')}
        </span>
        <div className={styles.track}>
          <div className={styles.bar} data-indeterminate={ratio === null ? 'true' : undefined}
            style={ratio === null ? undefined : { width: `${ratio * 100}%` }} />
        </div>
        <span className="field-hint">
          {phase === 'installing'
            ? t('update.installingNote')
            : total
              ? t('update.progressKnown', { done: megabytes(downloaded), total: megabytes(total) })
              : t('update.progressUnknown', { done: megabytes(downloaded) })}
        </span>
      </div>}

      <section className={styles.notes}>
        <h3>{t('update.notesTitle')}</h3>
        {report.notes
          ? <div className={styles.notesBody}>{renderReleaseNotes(report.notes)}</div>
          : <p className="field-hint">{t('update.noNotes')}</p>}
      </section>
      {/* 共存模式的提醒只在正文里说一句：底栏那句必须短，长句会把底栏挤成三行（第一版的毛病）。 */}
      <p className="field-hint">{t('update.coexistHint')}</p>
      {/* 弹窗里只放摘要：完整说明（含表格、截图）在发布页上，给一个出口而不是把长文塞进来。 */}
      {report.releaseUrl && <div><button className="text-button" onClick={() => {
        void client.openReleasePage(report.releaseUrl!).catch(() => showToast(t('update.openFailed'), 'danger'));
      }}>{t('update.fullNotes')}</button></div>}
    </div>
  </Dialog>;
}
