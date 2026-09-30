import { Cpu, Info, Network, Palette, ScrollText, ShieldAlert, Trash2, Wrench } from 'lucide-react';
import { Dialog } from '@/components/Dialog';
import { showToast } from '@/components/Toast';
import { EmptyState } from '@/components/EmptyState';
import { useCallback, useEffect, useState } from 'react';
import type { CodexInstance } from '@/contracts/types';
import { type BackupEntry, type DesktopClient, type GatewayReport, type UpdateReport, toCoreError } from '@/desktop/client';
import { applyTheme, readThemePreference, setThemePreference, type ThemePreference } from '@/theme';
import {
  ACCENT_PRESETS, DEFAULT_ACCENT, DEFAULT_ACCENT_BASE, accentSwatch, normalizeHex,
  readAccentPreference, setAccentPreference, type AccentPreference,
} from '@/accent';
import styles from './SettingsPage.module.css';

import { readLocalePreference, setLocalePreference, t, type LocalePreference } from '@/i18n';

/** 十六进制框非法提示的 id，输入框用 aria-describedby 指过来。 */
const HEX_HINT_ID = 'accent-hex-hint';

/** 取色器与十六进制框里显示的「用户挑的色号」：默认档没有自己的色号，用默认青绿当代表。 */
function pickedHex(preference: AccentPreference): string {
  return preference === DEFAULT_ACCENT ? DEFAULT_ACCENT_BASE : preference;
}

/**
 * 设置页（设计 P09）。
 *
 * 原则：**能读到的真实状态就显示真值，做不到的就明说未实现**，
 * 不放一个看起来能点、实际不生效的开关。危险操作单独成组，不与日常项混在一起。
 */
export function SettingsPage({ client, gateway, onNavigate, onReopenOnboarding }: {
  client: DesktopClient;
  gateway: GatewayReport | null;
  onNavigate: (page: 'codexConfig' | 'logs' | 'diagnostics') => void;
  /** 重新打开首次接入向导（概览页）。向导被「稍后再说」关掉之后，这里是唯一的入口。 */
  onReopenOnboarding: () => void;
}) {
  const [instances, setInstances] = useState<CodexInstance[]>([]);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState('');
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [previewText, setPreviewText] = useState('');
  const [update, setUpdate] = useState<UpdateReport | null>(null);
  const [restore, setRestore] = useState<BackupEntry | null>(null);
  /** 暂停状态取自后端，不在前端自己翻转，避免与托盘菜单不一致。 */
  const [preference, setPreference] = useState<ThemePreference>(() => readThemePreference());
  const [resolved, setResolved] = useState(() => applyTheme(readThemePreference()));
  const applyPreference = (next: ThemePreference) => { setPreference(next); setResolved(setThemePreference(next)); };
  /**
   * 主题色。存的是用户挑的色号，界面上的色块与回显都要过 `accentSwatch` 换成**当前主题实际
   * 会用的那支**——同一支色在深浅两个主题下是不同的色号，所以两者都依赖 `resolved`。
   */
  const [accent, setAccent] = useState<AccentPreference>(() => readAccentPreference());
  /** 十六进制框要留得住「还没输完」的中间态，所以它有草稿，不直接绑定 accent。 */
  const [hexDraft, setHexDraft] = useState(() => pickedHex(accent));
  const [hexInvalid, setHexInvalid] = useState(false);

  /** 落定一支色：写偏好（立刻生效）+ 同步状态。归一化由 `setAccentPreference` 定，取它的返回值。 */
  const chooseAccent = (hex: string) => {
    const next = setAccentPreference(hex);
    setAccent(next);
    setHexDraft(pickedHex(next));
    setHexInvalid(false);
  };
  /** 十六进制框的「还原」：不留一个假值在框里，落回当前生效的那支色。 */
  const restoreHexDraft = () => { setHexDraft(pickedHex(accent)); setHexInvalid(false); };
  /**
   * 十六进制框的输入。
   *
   * 收得下就立刻生效（用户不必猜「要不要按回车」），**但不动草稿**：边输边把框里的字
   * 换成归一化后的色号，等于把光标拽到末尾，在小写化与改中间那几位时都会跟用户抢输入。
   * 收不下也不吭声——正在输的一半必然不合法，那时候报错只会一直闪红；
   * 提示留给「按了回车还是收不下」这一种真的失败的提交。
   */
  const editHexDraft = (value: string) => {
    setHexDraft(value);
    const hex = normalizeHex(value);
    if (hex) { setAccent(setAccentPreference(hex)); setHexInvalid(false); }
  };
  /** 语言与主题同构：值写进 localStorage，解析结果由 i18n 通知全树重渲染。 */
  const [language, setLanguage] = useState<LocalePreference>(() => readLocalePreference());
  const applyLanguage = (next: LocalePreference) => { setLanguage(next); setLocalePreference(next); };
  const [paused, setPaused] = useState(gateway?.paused ?? false);
  useEffect(() => { setPaused(gateway?.paused ?? false); }, [gateway?.paused]);

  const togglePaused = useCallback(async () => {
    setError('');
    try { setPaused(await client.setGatewayPaused(!paused)); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('settings.pauseFailed')); }
  }, [client, paused]);

  const loadBackups = useCallback(async () => {
    try { setBackups(await client.listBackups()); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('settings.backupsLoadFailed')); }
  }, [client]);

  useEffect(() => { void loadBackups(); }, [loadBackups]);

  async function run(label: string, work: () => Promise<void>) {
    setBusy(label); setError('');
    try { await work(); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('common.failed')); }
    finally { setBusy(''); }
  }

  const checkUpdate = () => run('update', async () => { setUpdate(await client.checkUpdate()); });
  /** 安装入口不在这里：它在侧栏左上角（见 docs/architecture/06-updates.md 的界面一节）。
      这里的「到发布页」是手动兜底，走系统浏览器打开。 */
  const openRelease = (url: string) => run('release', async () => { await client.openReleasePage(url); });

  const createBackup = () => run('backup', async () => {
    await client.createBackup(instances[0]!.id);
    showToast(t('settings.backupCreated'));
    await loadBackups();
  });

  const showPreview = (entry: BackupEntry) => run('preview', async () => { setPreviewText(await client.previewBackup(entry.id)); });

  const restoreBackup = (entry: BackupEntry) => run('restore', async () => {
    const target = await client.restoreBackup(entry.id);
    setRestore(null);
    showToast(t('settings.restored', { path: target }));
    await loadBackups();
  });

  useEffect(() => {
    let current = true;
    client.detectInstances()
      .then(found => { if (current) setInstances(found); })
      .catch(thrown => { if (current) setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('codex.detectFailed')); });
    return () => { current = false; };
  }, [client]);

  return <div className={styles.page}>
    {error && <div className="error-message" role="alert">{error}</div>}

    <section className={styles.card}>
      <h2><Palette size={18} />{t('settings.appearance')}</h2>
      <dl className={styles.rows}>
        {/* 控件的文字是垂直居中的（40px 高的下拉），标签默认与控件方框顶部对齐，
            看上去像高了一截。带上 controlLabel：标签下沉 (40-20)/2，与控件里的文字对齐。 */}
        <dt className={styles.controlLabel}>{t('settings.themeLabel')}</dt><dd>
          <select aria-label={t('settings.themeLabel')} value={preference} onChange={event => applyPreference(event.target.value as ThemePreference)}>
            <option value="dark">{t('settings.themeDark')}</option>
            <option value="light">{t('settings.themeLight')}</option>
            <option value="system">{t('common.followSystem')}</option>
          </select>
          <span className="text-muted">{t('settings.themeNote', { resolved: resolved === 'dark' ? t('settings.themeDark') : t('settings.themeLight') })}</span>
        </dd>
        {/*
         * 主题色。色块的底色是**算出来的**（`accentSwatch`：只取你挑的色相，亮度搬到本主题那一档），
         * 既不在任何 token 里、也不是某个 class 能表达的，所以这里是全项目唯一用内联 style 上色的地方。
         * 调色板、取色器与「实际应用」三处都走同一个函数：点之前看到的色和点之后生效的色必须是同一个。
         */}
        <dt className={styles.accentLabel}>{t('settings.accent')}</dt><dd className={styles.accentCell}>
          <div className={styles.accentRow} role="group" aria-label={t('settings.accent')}>
            <button type="button" className={styles.accentChip} aria-pressed={accent === DEFAULT_ACCENT}
              aria-label={t('settings.accentPresetDefault')} onClick={() => chooseAccent(DEFAULT_ACCENT_BASE)}>
              <span className={styles.swatch} style={{ background: accentSwatch(DEFAULT_ACCENT, resolved) }} />
            </button>
            {ACCENT_PRESETS.map(preset => <button key={preset.id} type="button" className={styles.accentChip}
              aria-pressed={accent === preset.hex} aria-label={t(preset.labelKey)} onClick={() => chooseAccent(preset.hex)}>
              <span className={styles.swatch} style={{ background: accentSwatch(preset.hex, resolved) }} />
            </button>)}
          </div>
          <div className={styles.customBlock}>
            <div className={styles.customRow}>
              <span className={styles.customLabel}>{t('settings.accentCustom')}</span>
              <input type="color" className={styles.colorPicker} aria-label={t('settings.accentPick')}
                value={pickedHex(accent)} onChange={event => chooseAccent(event.target.value)} />
              <input type="text" className={styles.hexInput} aria-label={t('settings.accentHexLabel')}
                spellCheck={false} value={hexDraft} aria-invalid={hexInvalid || undefined}
                aria-describedby={hexInvalid ? HEX_HINT_ID : undefined}
                onChange={event => editHexDraft(event.target.value)}
                onKeyDown={event => {
                  if (event.key === 'Enter') {
                    // 回车是「提交」：收不下就明说。草稿留着不动，用户改得动。
                    if (normalizeHex(hexDraft)) chooseAccent(hexDraft);
                    else setHexInvalid(true);
                  } else if (event.key === 'Escape') restoreHexDraft();
                }}
                onBlur={restoreHexDraft} />
            </div>
            {hexInvalid && <p className={styles.hexError} id={HEX_HINT_ID} role="alert">{t('settings.accentInvalid')}</p>}
          </div>
          {/* 预设同样会被改写明暗，所以这一行不只在自定义时显示。 */}
          <div className={styles.appliedRow}>
            <span className={styles.swatch} style={{ background: accentSwatch(accent, resolved) }} />
            <span>{t('settings.accentApplied')}</span>
            <code className="text-mono">{accentSwatch(accent, resolved)}</code>
          </div>
          <p className={styles.accentNote}>{t('settings.accentNote')}</p>
        </dd>
        <dt className={styles.controlLabel}>{t('settings.language')}</dt><dd>
          <select aria-label={t('settings.language')} value={language} onChange={event => applyLanguage(event.target.value as LocalePreference)}>
            <option value="system">{t('common.followSystem')}</option>
            <option value="zh-CN">{t('settings.languageZh')}</option>
            <option value="en">{t('settings.languageEn')}</option>
          </select>
          <span className="text-muted">{t('settings.languageNote')}</span>
        </dd>
        <dt>{t('settings.motion')}</dt><dd>{t('common.followSystem')}<span className="text-muted">{t('settings.motionNote')}</span></dd>
      </dl>
    </section>

    <section className={styles.card}>
      <h2><Network size={18} />{t('settings.gateway')}</h2>
      <dl className={styles.rows}>
        <dt>{t('settings.status')}</dt><dd className={gateway?.running ? styles.online : styles.offline}>
          {gateway?.running ? t('settings.running') : t('settings.notRunning')}
          {gateway?.error && <span className="text-muted">{gateway.error}</span>}
        </dd>
        <dt>{t('settings.publishedCatalog')}</dt><dd>{gateway?.revisions.length ? t('settings.publishedCount', { count: gateway.revisions.length }) : t('settings.publishedNone')}<span className="text-muted">{t('settings.publishedNote')}</span></dd>
        <dt>{t('settings.servedRequests')}</dt><dd className="text-mono">{gateway?.served ?? 0}<span className="text-muted">{t('settings.servedNote')}</span></dd>
        <dt>{t('settings.acceptRequests')}</dt><dd>{paused ? t('settings.acceptPaused') : t('settings.acceptNormal')}
          <span className="text-muted">{t('settings.pauseNote')}</span></dd>
      </dl>
      <div className={styles.actions}>
        <button onClick={() => void togglePaused()} disabled={!gateway?.running}>
          {paused ? t('settings.resume') : t('settings.pause')}
        </button>
      </div>
    </section>

    <section className={styles.card}>
      <h2><Cpu size={18} />{t('codex.instances')}</h2>
      {instances.length === 0
        ? <EmptyState icon={Cpu} title={t('settings.noInstance')} description={t('settings.noInstanceBody')} />
        : <ul className={styles.instances}>{instances.map(instance => <li key={instance.id}>
          <div><strong>{instance.appPath ?? t('settings.noAppPath')}</strong>
            <span className="text-mono text-muted break-anywhere">{instance.configFile}</span></div>
          <div className={styles.tags}>
            <span className="badge">{instance.compatibility === 'unverified' ? t('onboarding.compatUnverified') : instance.compatibility}</span>
            {instance.configExists ? <span className="badge">{t('onboarding.configExists')}</span> : <span className="badge warning">{t('onboarding.configMissing')}</span>}
            {instance.conflictingManagers.length > 0 && <span className="badge warning">{t('settings.otherTool', { names: instance.conflictingManagers.join(t('common.itemSeparator')) })}</span>}
          </div>
        </li>)}</ul>}
      <div className={styles.actions}>
        <button onClick={onReopenOnboarding}>{t('settings.reopenOnboarding')}</button>
      </div>
      <p className={styles.note}>{t('settings.reopenOnboardingNote')}</p>
    </section>

    <section className={styles.card}>
      <h2><ScrollText size={18} />{t('settings.logsAndDiagnostics')}</h2>
      <dl className={styles.rows}>
        <dt>{t('settings.backupRetention')}</dt><dd>{t('settings.retentionValue')}<span className="text-muted">{t('settings.retentionNote')}</span></dd>
        <dt>{t('settings.scope')}</dt><dd>{t('settings.scopeValue')}<span className="text-muted">{t('settings.scopeNote')}</span></dd>
      </dl>
      <div className={styles.actions}>
        <button onClick={() => onNavigate('diagnostics')}>{t('nav.diagnostics')}</button>
        <button onClick={() => onNavigate('logs')}>{t('settings.viewLogs')}</button>
      </div>
    </section>

    <section className={styles.card}>
      <h2><Wrench size={18} />{t('settings.backupAndUpdate')}</h2>
      <dl className={styles.rows}>
        <dt>{t('settings.autoBackup')}</dt><dd>{t('settings.autoBackupValue')}<span className="text-muted">{t('settings.autoBackupNote')}</span></dd>
        <dt>{t('settings.backupRetention')}</dt><dd>{t('settings.backupRetentionValue')}<span className="text-muted">{t('settings.backupRetentionNote')}</span></dd>
        <dt>{t('settings.update')}</dt><dd>
          {update?.error ? <span className={styles.offline}>{t('settings.updateFailed', { error: update.error })}<small>{t('settings.updateFailedNote')}</small></span>
            : update?.hasUpdate ? <span className={styles.online}>{t('settings.updateAvailable', { version: update.latest ?? '' })}<small>{t('settings.updateAvailableNote', { current: update.current ?? '' })}</small></span>
            : update ? <span>{t('settings.upToDate', { version: update.current ?? '' })}</span>
            : <span className="text-muted">{t('settings.updateNotChecked')}</span>}
        </dd>
      </dl>
      <div className={styles.actions}>
        <button onClick={() => void checkUpdate()} disabled={busy === 'update'}>{busy === 'update' ? t('diag.checking') : t('settings.checkUpdate')}</button>
        {update?.hasUpdate && update.releaseUrl && <button onClick={() => openRelease(update.releaseUrl!)} disabled={busy === 'release'}>{t('settings.openRelease')}</button>}
        <button onClick={() => void createBackup()} disabled={busy === 'backup' || !instances.length}>{t('settings.backupNow')}</button>
      </div>

      <h3 className={styles.subHeading}>{t('settings.recentBackups')}</h3>
      {backups.length === 0
        ? <p className="text-muted">{t('settings.noBackups')}</p>
        : <ul className={styles.backups}>{backups.slice(0, 8).map(item => <li key={item.id}>
          <div>
            <strong className="text-mono">{item.createdAt}</strong>
            <span className="text-muted text-mono break-anywhere">{t('settings.backupMeta', { hash: item.contentHash.slice(0, 12), bytes: item.bytes })}</span>
          </div>
          {item.mayContainSecrets && <span className="badge warning">{t('settings.mayContainSecrets')}</span>}
          <div className="actions">
            <button onClick={() => void showPreview(item)}>{t('settings.maskedPreview')}</button>
            <button className="danger" onClick={() => setRestore(item)}>{t('settings.restore')}</button>
          </div>
        </li>)}</ul>}
      {previewText && <textarea className={styles.summary} readOnly aria-label={t('settings.backupPreview')} rows={6} value={previewText} />}
    </section>

    <section className={styles.card}>
      <h2><Info size={18} />{t('settings.about')}</h2>
      <dl className={styles.rows}>
        <dt>{t('settings.version')}</dt><dd className="text-mono">{__APP_VERSION__}</dd>
        <dt>{t('settings.repository')}</dt><dd><a href="https://github.com/nexsjournal/switchelp-macapp" rel="noreferrer noopener" target="_blank" className="text-mono">github.com/nexsjournal/switchelp-macapp</a></dd>
        <dt>{t('settings.license')}</dt><dd>{t('settings.licenseValue')}<span className="text-muted">{t('settings.licenseNote')}</span></dd>
      </dl>
    </section>

    <section className={`${styles.card} ${styles.dangerZone}`}>
      <h2><ShieldAlert size={18} />{t('settings.dangerZone')}</h2>
      <p className="text-muted">{t('settings.dangerZoneNote')}</p>
      <div className={styles.actions}>
        <button onClick={() => onNavigate('codexConfig')}>{t('settings.restoreCodex')}</button>
        <button onClick={() => onNavigate('logs')}><Trash2 size={16} />{t('settings.clearLogs')}</button>
      </div>
      <p className={styles.note}>{t('settings.dangerNote')}</p>
    </section>

    {restore && <Dialog width="narrow" title={t('settings.restoreAction')} busy={busy === 'restore'}
      description={t('settings.restoreBody', { path: restore.sourcePath, time: restore.createdAt })}
      onClose={() => setRestore(null)} footer={<footer className="form-footer">
        <span>{t('settings.restoreNote')}</span>
        <div className="actions">
          <button onClick={() => setRestore(null)} disabled={busy === 'restore'}>{t('action.cancel')}</button>
          <button className="danger" autoFocus disabled={busy === 'restore'} onClick={() => void restoreBackup(restore)}>
            {busy === 'restore' ? t('settings.restoring') : t('settings.restoreAction')}
          </button>
        </div>
      </footer>}
      />}
  </div>;
}
