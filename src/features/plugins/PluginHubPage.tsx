import { useCallback, useEffect, useMemo, useState } from 'react';
import { Check, ExternalLink, PackageOpen, Plus, RefreshCw, Trash2, TriangleAlert } from 'lucide-react';
import type {
  ConflictChoice, InstallPreview, InstallReport, PluginSource, RepoCatalog, RepoSkill, SkillRecord, SkillTarget, UpdateInfo,
} from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { SegmentedTabs } from '@/components/SegmentedTabs';
import { EmptyState } from '@/components/EmptyState';
import { showToast } from '@/components/Toast';
import { t } from '@/i18n';
import styles from './PluginHubPage.module.css';

/**
 * 插件中心（设计 P-T2）。
 *
 * 两件事必须一直看得见，因为它们是这个板块最容易伤人 / 骗人的地方：
 *
 * 1. **技能是给 AI 的执行说明**，不是给人点的按钮。详情页默认展开它会要求 AI 做什么。
 * 2. **目标目录不是我们装的时候绝不覆盖**——只提供「跳过」和「装到新目录」两个选择，
 *    因为覆盖就意味着删除别人的文件。
 *
 * 安装只写文件（`SKILL.md` 及其同目录文件），带归属清单，可精确回滚。不执行仓库里的任何东西。
 */
export function PluginHubPage({ client }: { client: DesktopClient }) {
  const [tab, setTab] = useState<'market' | 'installed'>('market');

  const [sources, setSources] = useState<PluginSource[]>([]);
  const [repo, setRepo] = useState('');
  const [newSource, setNewSource] = useState('');
  const [addingSource, setAddingSource] = useState(false);
  const [catalog, setCatalog] = useState<RepoCatalog | null>(null);
  const [loadingCatalog, setLoadingCatalog] = useState(false);
  const [catalogError, setCatalogError] = useState('');
  const [selected, setSelected] = useState<string>('');
  const [query, setQuery] = useState('');
  const [markdownOpen, setMarkdownOpen] = useState(false);

  const [targets, setTargets] = useState<SkillTarget[]>([]);
  const [chosen, setChosen] = useState<string[]>([]);

  const [preview, setPreview] = useState<InstallPreview | null>(null);
  const [conflictChoices, setConflictChoices] = useState<Record<string, ConflictChoice>>({});
  const [installing, setInstalling] = useState(false);
  const [report, setReport] = useState<InstallReport | null>(null);
  const [runError, setRunError] = useState('');

  const [installed, setInstalled] = useState<SkillRecord[]>([]);
  const [updates, setUpdates] = useState<UpdateInfo[]>([]);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [pendingUninstall, setPendingUninstall] = useState<SkillRecord | null>(null);

  const loadSources = useCallback(async () => {
    try {
      const list = await client.listPluginSources();
      setSources(list);
      setRepo(current => current || list[0]?.repo || '');
    } catch (cause) {
      const core = toCoreError(cause);
      setCatalogError(core.safeDetails[0] ?? t(core.messageKey));
    }
  }, [client]);

  const loadInstalled = useCallback(async () => {
    try {
      setInstalled(await client.listInstalledSkills());
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    }
  }, [client]);

  useEffect(() => { void loadSources(); void loadInstalled(); }, [loadSources, loadInstalled]);
  useEffect(() => {
    void client.listSkillTargets().then(list => {
      setTargets(list);
      setChosen(list.map(target => target.toolId));
    }).catch(() => setTargets([]));
  }, [client]);

  const browse = useCallback(async (target: string) => {
    if (!target) return;
    setLoadingCatalog(true);
    setCatalogError('');
    setReport(null);
    try {
      const result = await client.browsePluginRepo(target);
      setCatalog(result);
      setSelected(result.skills[0]?.dirName ?? '');
      setMarkdownOpen(false);
    } catch (cause) {
      const core = toCoreError(cause);
      setCatalog(null);
      setCatalogError(core.safeDetails[0] ?? t(core.messageKey));
    } finally {
      setLoadingCatalog(false);
    }
  }, [client]);

  useEffect(() => { if (repo) void browse(repo); }, [repo, browse]);

  const skills = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!catalog) return [];
    if (!needle) return catalog.skills;
    return catalog.skills.filter(skill =>
      `${skill.dirName} ${skill.document.id} ${skill.document.description ?? ''}`
        .toLocaleLowerCase()
        .includes(needle));
  }, [catalog, query]);

  const current: RepoSkill | undefined = useMemo(
    () => catalog?.skills.find(skill => skill.dirName === selected),
    [catalog, selected],
  );

  /** 已经装过的（技能 + 工具）组合，用于卡片上的「已装」标记。 */
  const installedKeys = useMemo(
    () => new Set(installed.map(record => `${record.skillId}::${record.targetTool}`)),
    [installed],
  );

  const startInstall = async (skill: RepoSkill) => {
    setRunError('');
    setReport(null);
    try {
      const plan = await client.previewPluginInstall({
        repo: catalog!.repo,
        gitRef: null,
        skillDirs: [skill.dirName],
        targets: chosen,
        conflictChoices: {},
      });
      setConflictChoices({});
      setPreview(plan);
    } catch (cause) {
      const core = toCoreError(cause);
      setRunError(core.safeDetails[0] ?? t(core.messageKey));
    }
  };

  const confirmInstall = async () => {
    if (!preview) return;
    setInstalling(true);
    setRunError('');
    try {
      const result = await client.installPlugin({
        repo: preview.repo,
        gitRef: null,
        skillDirs: preview.skills.map(skill => skill.dirName),
        targets: chosen,
        conflictChoices,
      });
      setReport(result);
      setPreview(null);
      await loadInstalled();
      // 部分完成必须写清楚，不能只报一句「安装完成」。
      if (result.failed.length || result.skipped.length) {
        showToast(t('plugins.report.partial', {
          installed: result.installed.length,
          total: result.installed.length + result.failed.length + result.skipped.length,
        }));
      } else {
        showToast(t('plugins.report.installed', { count: result.installed.length }));
      }
    } catch (cause) {
      const core = toCoreError(cause);
      setRunError(core.safeDetails[0] ?? t(core.messageKey));
    } finally {
      setInstalling(false);
    }
  };

  const toggleTarget = (toolId: string) => {
    setChosen(current => current.includes(toolId) ? current.filter(id => id !== toolId) : [...current, toolId]);
  };

  const addSource = async () => {
    if (!newSource.trim()) return;
    setAddingSource(true);
    try {
      const list = await client.addPluginSource(newSource.trim());
      setSources(list);
      setRepo(newSource.trim().replace(/^https?:\/\/github\.com\//, '').replace(/\.git$/, ''));
      setNewSource('');
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    } finally {
      setAddingSource(false);
    }
  };

  const removeSource = async (target: string) => {
    try {
      setSources(await client.removePluginSource(target));
      if (repo === target) setRepo(sources.find(source => source.repo !== target)?.repo ?? '');
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    }
  };

  const checkUpdates = async () => {
    setCheckingUpdates(true);
    try {
      setUpdates(await client.checkSkillUpdates());
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    } finally {
      setCheckingUpdates(false);
    }
  };

  const toggleEnabled = async (record: SkillRecord) => {
    try {
      const updated = await client.setSkillEnabled(record.skillId, record.targetTool, !record.enabled);
      setInstalled(list => list.map(item =>
        item.skillId === updated.skillId && item.targetTool === updated.targetTool ? updated : item));
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    }
  };

  const confirmUninstall = async () => {
    if (!pendingUninstall) return;
    const record = pendingUninstall;
    setPendingUninstall(null);
    try {
      const outcomes = await client.uninstallSkill(record.skillId, [record.targetTool]);
      const outcome = outcomes[0];
      // 有三种「没有全删干净」的情形，都要如实说出来。
      if (outcome && (outcome.keptModified.length || outcome.foreignFiles.length)) {
        showToast(t('plugins.uninstall.partial', {
          kept: outcome.keptModified.length + outcome.foreignFiles.length,
        }));
      } else {
        showToast(t('plugins.uninstall.done', { name: record.skillId }));
      }
      await loadInstalled();
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    }
  };

  return (
    <div className={styles.page}>
      <SegmentedTabs
        ariaLabel={t('plugins.title')}
        active={tab}
        onChange={id => setTab(id as 'market' | 'installed')}
        tabs={[
          { id: 'market', label: t('plugins.tab.market') },
          { id: 'installed', label: t('plugins.tab.installed'), count: installed.length },
        ]} />

      {runError && <div className="error-message" role="alert">{runError}</div>}

      {report && (
        <section className={styles.report} role="status" aria-live="polite">
          <strong>{t('plugins.report.title')}</strong>
          <ul>
            {report.installed.map(record => (
              <li key={`${record.skillId}:${record.targetTool}`} className={styles.reportOk}>
                <Check size={13} />{t('plugins.report.line', { name: record.skillId, tool: record.targetDisplayName })}
              </li>
            ))}
            {report.skipped.map(item => (
              <li key={`s:${item.toolId}:${item.dirName}`} className={styles.reportWarn}>
                <TriangleAlert size={13} />{t('plugins.report.skipped', { tool: item.toolId, name: item.dirName })}：{item.reason}
              </li>
            ))}
            {report.failed.map(item => (
              <li key={`f:${item.toolId}:${item.dirName}`} className={styles.reportBad}>
                <TriangleAlert size={13} />{t('plugins.report.failed', { tool: item.toolId, name: item.dirName })}：{item.message}
              </li>
            ))}
          </ul>
          <button type="button" onClick={() => setReport(null)}>{t('common.close')}</button>
        </section>
      )}

      {tab === 'market' ? (
        <>
          <section className={styles.card}>
            <div className={styles.sourceRow}>
              <label className={styles.sourcePicker}>
                <span>{t('plugins.source.label')}</span>
                <select value={repo} onChange={event => setRepo(event.target.value)} disabled={loadingCatalog}>
                  {sources.map(source => <option key={source.repo} value={source.repo}>{source.label} · {source.repo}</option>)}
                </select>
              </label>
              <div className={styles.addSource}>
                <input value={newSource} onChange={event => setNewSource(event.target.value)}
                  placeholder={t('plugins.source.addPlaceholder')} aria-label={t('plugins.source.addPlaceholder')}
                  onKeyDown={event => { if (event.key === 'Enter') void addSource(); }} />
                <button type="button" onClick={() => void addSource()} disabled={addingSource || !newSource.trim()}>
                  <Plus size={15} />{t('plugins.source.add')}
                </button>
              </div>
              {repo && !sources.find(source => source.repo === repo)?.builtin && (
                <button type="button" className={styles.removeSource} onClick={() => void removeSource(repo)}>
                  <Trash2 size={14} />{t('plugins.source.remove')}
                </button>
              )}
            </div>
            {catalog && (
              <p className={styles.commit}>{t('plugins.source.commit', { commit: catalog.commit.slice(0, 8), count: catalog.skills.length })}</p>
            )}
            {catalogError && <div className="error-message" role="alert">{catalogError}</div>}
          </section>

          {loadingCatalog ? (
            <div className={styles.empty} role="status" aria-live="polite">{t('plugins.loadingCatalog')}</div>
          ) : !catalog ? (
            <EmptyState icon={PackageOpen} title={t('plugins.market.empty.title')} description={t('plugins.market.empty.body')} />
          ) : (
            <div className={styles.marketLayout}>
              <section className={styles.listColumn} aria-label={t('plugins.market.listLabel')}>
                <input className={styles.search} value={query} onChange={event => setQuery(event.target.value)}
                  placeholder={t('plugins.searchPlaceholder')} aria-label={t('plugins.search')} />
                <ul className={styles.skillList}>
                  {skills.map(skill => {
                    const installedHere = chosen.some(toolId => installedKeys.has(`${skill.document.id}::${toolId}`))
                      || installed.some(record => record.skillId === skill.document.id);
                    return (
                      <li key={skill.dirName}>
                        <button type="button" className={skill.dirName === selected ? styles.selected : ''}
                          onClick={() => { setSelected(skill.dirName); setMarkdownOpen(false); }}>
                          <span className={styles.skillName}>{skill.document.id}</span>
                          {installedHere && <span className={styles.installedTag}>{t('plugins.installedTag')}</span>}
                          <span className={styles.skillDescription}>
                            {skill.document.description ?? t('plugins.noDescription')}
                          </span>
                        </button>
                      </li>
                    );
                  })}
                  {!skills.length && <li className={styles.empty}>{t('plugins.noMatch')}</li>}
                </ul>
              </section>

              <section className={styles.detailColumn} aria-label={t('plugins.detail.label')}>
                {current ? (
                  <>
                    <h2 className={styles.detailTitle}>{current.document.id}</h2>
                    <p className={styles.detailPath}>
                      {current.sourcePath}
                      {catalog && <> · <span className="text-mono">{catalog.repo}@{catalog.commit.slice(0, 8)}</span></>}
                    </p>
                    <p className={styles.detailDescription}>
                      {current.document.description ?? t('plugins.noDescription')}
                    </p>
                    {!current.document.frontMatterParsed && (
                      <p className={styles.warnNote}>{t('plugins.unparsedFrontMatter')}</p>
                    )}
                    {current.document.requiresBins.length > 0 && (
                      <p className={styles.requires}>
                        {t('plugins.requiresBins', { bins: current.document.requiresBins.join('、') })}
                      </p>
                    )}

                    {/* 技能是写给 AI 的指令这件事，必须在安装之前说清楚。 */}
                    <p className={styles.caution}>{t('plugins.caution')}</p>

                    <button type="button" className={styles.disclosure} aria-expanded={markdownOpen}
                      onClick={() => setMarkdownOpen(open => !open)}>
                      {markdownOpen ? t('plugins.hideDocument') : t('plugins.showDocument')}
                    </button>
                    {markdownOpen && <pre className={styles.markdown}>{current.files[0]?.text}</pre>}
                    {current.files.length > 1 && (
                      <p className={styles.fileList}>
                        {t('plugins.siblingFiles', { count: current.files.length - 1 })}
                        {' '}
                        {current.files.slice(1).map(file => file.path).join('、')}
                      </p>
                    )}

                    <fieldset className={styles.targets}>
                      <legend>{t('plugins.targets.label')}</legend>
                      {targets.map(target => (
                        <label key={target.toolId} className="check-label">
                          <input type="checkbox" checked={chosen.includes(target.toolId)}
                            onChange={() => toggleTarget(target.toolId)} />
                          <span>{target.displayName}</span>
                          <span className={styles.targetRoot}>{target.root}</span>
                        </label>
                      ))}
                      {!targets.length && <p className={styles.warnNote}>{t('plugins.targets.none')}</p>}
                    </fieldset>

                    <div className="actions">
                      <button className="primary" disabled={!chosen.length} onClick={() => void startInstall(current)}>
                        {t('plugins.install')}
                      </button>
                      <button type="button" onClick={() => window.open(`https://github.com/${catalog?.repo}`, '_blank', 'noreferrer')}>
                        <ExternalLink size={15} />{t('plugins.openRepo')}
                      </button>
                    </div>
                  </>
                ) : (
                  <p className={styles.empty}>{t('plugins.pickSkill')}</p>
                )}
              </section>
            </div>
          )}
        </>
      ) : (
        <section className={styles.card}>
          <div className={styles.installedHeader}>
            <h2>{t('plugins.tab.installed')}</h2>
            <button type="button" onClick={() => void checkUpdates()} disabled={checkingUpdates || !installed.length}>
              <RefreshCw size={15} className={checkingUpdates ? styles.spin : ''} />{t('plugins.checkUpdates')}
            </button>
          </div>
          {!installed.length ? (
            <EmptyState icon={PackageOpen} title={t('plugins.installed.empty.title')} description={t('plugins.installed.empty.body')} />
          ) : (
            <ul className={styles.installedList}>
              {installed.map(record => {
                const update = updates.find(item => item.skillId === record.skillId && item.targetTool === record.targetTool);
                return (
                  <li key={`${record.skillId}:${record.targetTool}`}>
                    <div className={styles.installedRow}>
                      <div className={styles.installedMain}>
                        <span className={styles.skillName}>{record.skillId}</span>
                        <span className={styles.installedMeta}>
                          {record.targetDisplayName} · <span className="text-mono">{record.sourceRepo}@{record.sourceCommit.slice(0, 8)}</span>
                        </span>
                        <span className={`${styles.installedMeta} break-anywhere`}>{record.installedPath}</span>
                      </div>
                      <div className={styles.installedActions}>
                        {update && <span className="badge warning">{t('plugins.updateAvailable')}</span>}
                        {!record.enabled && <span className="badge">{t('plugins.disabledTag')}</span>}
                        <label className="check-label">
                          <input type="checkbox" checked={record.enabled} onChange={() => void toggleEnabled(record)}
                            aria-label={t('plugins.enableToggle', { name: record.skillId })} />
                          <span>{record.enabled ? t('plugins.enabled') : t('plugins.disabled')}</span>
                        </label>
                        <button type="button" onClick={() => setPendingUninstall(record)}>
                          <Trash2 size={15} />{t('plugins.uninstall')}
                        </button>
                      </div>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
          <p className={styles.note}>{t('plugins.disableNote')}</p>
        </section>
      )}

      {preview && (
        <Dialog title={t('plugins.confirm.title')} width="wide" busy={installing}
          description={t('plugins.confirm.description', { repo: preview.repo, commit: preview.commit.slice(0, 8) })}
          onClose={() => setPreview(null)}
          footer={<>
            <span>{t('plugins.confirm.footer')}</span>
            <div className="actions">
              <button type="button" onClick={() => setPreview(null)} disabled={installing}>{t('action.cancel')}</button>
              <button className="primary" onClick={() => void confirmInstall()} disabled={installing}>
                {installing ? t('plugins.confirm.installing') : t('plugins.confirm.action')}
              </button>
            </div>
          </>}>
          {preview.skills.flatMap(skill => skill.targets.map(target => {
            const key = `${target.toolId}::${skill.dirName}`;
            const choice = conflictChoices[key];
            return (
              <div key={key} className={styles.plan}>
                <p className={styles.planHead}>
                  <strong>{skill.skillId}</strong> → {target.displayName}
                  <span className={styles.planAction}>
                    {t(target.action === 'create' ? 'plugins.plan.create'
                      : target.action === 'update' ? 'plugins.plan.update' : 'plugins.plan.conflict')}
                  </span>
                </p>
                <p className={`${styles.planDir} break-anywhere`}>{target.dir}</p>
                <p className={styles.planFiles}>
                  {t('plugins.plan.files', { count: target.files.length })}
                  {target.foreignFiles.length > 0 && <> · {t('plugins.plan.foreign', { count: target.foreignFiles.length })}</>}
                </p>
                {target.action === 'conflict' && (
                  <>
                    <p className={styles.warnNote}>{target.conflictDetail}</p>
                    <div className={styles.conflictChoices}>
                      <label className="check-label">
                        <input type="radio" name={`c-${key}`} checked={choice !== 'keepBoth'}
                          onChange={() => setConflictChoices(current => ({ ...current, [key]: 'skip' }))} />
                        <span>{t('plugins.conflict.skip')}</span>
                      </label>
                      <label className="check-label">
                        <input type="radio" name={`c-${key}`} checked={choice === 'keepBoth'}
                          onChange={() => setConflictChoices(current => ({ ...current, [key]: 'keepBoth' }))} />
                        <span>{t('plugins.conflict.keepBoth')}</span>
                      </label>
                    </div>
                  </>
                )}
              </div>
            );
          }))}
        </Dialog>
      )}

      {pendingUninstall && (
        <Dialog title={t('plugins.uninstallConfirm.title')} busy={false}
          description={t('plugins.uninstallConfirm.description', { name: pendingUninstall.skillId, tool: pendingUninstall.targetDisplayName })}
          onClose={() => setPendingUninstall(null)}
          footer={<>
            <span>{t('plugins.uninstallConfirm.footer')}</span>
            <div className="actions">
              <button type="button" onClick={() => setPendingUninstall(null)}>{t('action.cancel')}</button>
              <button className="danger" onClick={() => void confirmUninstall()}>{t('plugins.uninstall')}</button>
            </div>
          </>}>
          <ul className={styles.uninstallFiles}>
            <li className="text-mono">{pendingUninstall.dirName}/SKILL.md</li>
            {pendingUninstall.files.filter(file => file.path !== 'SKILL.md').map(file => (
              <li key={file.path} className="text-mono">{pendingUninstall.dirName}/{file.path}</li>
            ))}
          </ul>
          <p className={styles.note}>{t('plugins.uninstallConfirm.managed')}</p>
        </Dialog>
      )}
    </div>
  );
}
