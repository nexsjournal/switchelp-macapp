import { useCallback, useEffect, useMemo, useState } from 'react';
import { Check, ChevronLeft, Download, ExternalLink, PackageOpen, Plus, RefreshCw, Store, Trash2, TriangleAlert } from 'lucide-react';
import { Github } from '@lobehub/icons';
import type {
  ConflictChoice, CoreError, InstallPreview, InstallReport, PluginSource, RepoCatalog, RepoSkill, SkillRecord, SkillTarget, UpdateInfo,
} from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { GithubTokenDialog } from '@/components/GithubTokenDialog';
import { Notice } from '@/components/Notice';
import { SegmentedTabs } from '@/components/SegmentedTabs';
import { EmptyState } from '@/components/EmptyState';
import { showToast } from '@/components/Toast';
import { t } from '@/i18n';
import styles from './PluginHubPage.module.css';

/**
 * 目录失败的摘要：限额与其它失败各一句。
 *
 * 两者的下一步完全不同——限额是「等等或填个令牌」，仓库不存在 / 连不上是「换个来源或查网络」——
 * 所以摘要不能合并成一句「加载失败」。核心给的完整原因放详情里，不铺在页面上。
 */
function catalogSummary(error: CoreError): string {
  if (error.messageKey === 'error.pluginRateLimited') return t('plugins.catalog.rateLimited');
  // 技能市场自己也会限流，但那跟 GitHub 限额是两件事，别共用一句话。
  if (error.messageKey === 'error.pluginRegistryRateLimited') return t('plugins.catalog.registryRateLimited');
  return t('plugins.catalog.failed');
}

/** 会被限流「等一会儿就好」的失败：这类用会自动消失的提示，不拿 danger 吓人。 */
function isRateLimited(error: CoreError): boolean {
  return error.messageKey === 'error.pluginRateLimited'
    || error.messageKey === 'error.pluginRegistryRateLimited';
}

/** 只有 GitHub 的限额能靠填令牌解决。技能市场不认 GitHub 令牌，给它按钮等于让人白做一件事。 */
function tokenHelps(error: CoreError): boolean {
  return error.messageKey === 'error.pluginRateLimited';
}

/**
 * 已经弹过提示的错误签名（`messageKey`）。
 *
 * 目录每进这一页都会重抓，而限额不会自己恢复——每挂载一次就弹一条，用户看到的就是
 * 「一直提示这个信息」（报告原文）。所以提示只在**第一次**撞上时说一声，之后只留常驻的
 * 那一行 `Notice`。模块级：切走再切回、组件重新挂载都不该再弹。
 */
const announcedCatalogErrors = new Set<string>();

/** 测试用：清掉「已经弹过」的记录，免得用例之间互相吞掉提示（与 Toast 的 `resetToasts` 同理）。 */
export function resetCatalogErrorAnnouncements() {
  announcedCatalogErrors.clear();
}

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
  /** 保留整个 CoreError：限额与其它失败要说不同的话、给不同的下一步。 */
  const [catalogError, setCatalogError] = useState<CoreError | null>(null);
  const [tokenConfigured, setTokenConfigured] = useState(false);
  const [tokenDialog, setTokenDialog] = useState(false);
  /** 正在看详情的技能目录名；null ＝ 停在卡片网格上（列表视图与详情视图互斥）。 */
  const [detailSkill, setDetailSkill] = useState<string | null>(null);
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
      // 来源列表取不到只影响上面那个选择器，走一次性提示；目录错误的那行提醒只属于
      // 「浏览目录」这条路径，不能拿它顶替。
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey), 'danger');
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

  const browse = useCallback(async (target: string, search?: string, skill?: string) => {
    if (!target) return;
    setLoadingCatalog(true);
    setCatalogError(null);
    setReport(null);
    // 重新取目录就回列表：目录换了之后，刚才在看的那条技能可能已经不在里面了。
    setDetailSkill(null);
    try {
      const result = await client.browsePluginRepo(target, search, skill);
      setCatalog(result);
      setMarkdownOpen(false);
    } catch (cause) {
      const core = toCoreError(cause);
      setCatalog(null);
      setCatalogError(core);
      // 第一次撞上时弹一次（之后只留那一行提醒，见 announcedCatalogErrors）。
      // 限额等一会儿会自己好，用会自动消失的 info；其它失败要人决定下一步，留到手动关掉。
      if (!announcedCatalogErrors.has(core.messageKey)) {
        announcedCatalogErrors.add(core.messageKey);
        showToast(catalogSummary(core), isRateLimited(core) ? 'info' : 'danger');
      }
    } finally {
      setLoadingCatalog(false);
    }
  }, [client]);

  useEffect(() => { if (repo) void browse(repo); }, [repo, browse]);

  /**
   * 技能市场（ClawHub / SkillHub）的目录有几千条，本地筛没有意义：搜索框要把关键词送到
   * 对方的搜索接口。GitHub 来源保持原来的本地筛，不额外打网络。
   * 判据是来源标识里没有 `/`——GitHub 来源永远是 `owner/repo` 形状。
   */
  const isRegistry = !repo.includes('/');
  useEffect(() => {
    if (!repo || !isRegistry) return;
    const timer = window.setTimeout(() => { void browse(repo, query.trim()); }, 400);
    return () => window.clearTimeout(timer);
  }, [query, repo, isRegistry, browse]);

  useEffect(() => {
    void client.contentGithubTokenStatus().then(setTokenConfigured).catch(() => setTokenConfigured(false));
  }, [client]);

  const skills = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!catalog) return [];
    // 市场来源的结果已经是按关键词搜出来的，再筛一遍只会把对方的相关性命中滤掉。
    if (!needle || isRegistry) return catalog.skills;
    return catalog.skills.filter(skill =>
      `${skill.dirName} ${skill.document.id} ${skill.document.description ?? ''}`
        .toLocaleLowerCase()
        .includes(needle));
  }, [catalog, query, isRegistry]);

  const current: RepoSkill | undefined = useMemo(
    () => catalog?.skills.find(skill => skill.dirName === detailSkill),
    [catalog, detailSkill],
  );

  /** 已经装过的（技能 + 工具）组合，用于卡片上的「已装」标记。 */
  const installedKeys = useMemo(
    () => new Set(installed.map(record => `${record.skillId}::${record.targetTool}`)),
    [installed],
  );

  /** 卡片 → 详情。展开状态跟着技能走：换一条就重新折叠。 */
  /** 列表阶段的目录条目只有名字与描述，正文要等打开时才取（见 registry 的 CATALOG_BLOB）。 */
  const bodyPending = (skill: RepoSkill) =>
    isRegistry && !(skill.files.find(file => file.path.endsWith('SKILL.md'))?.text ?? '').trim();

  const openDetail = (skill: RepoSkill) => {
    setDetailSkill(skill.dirName);
    setMarkdownOpen(false);
    // 市场来源按需取这一个技能的正文与文件清单：不进详情就不花这次请求。
    if (bodyPending(skill)) void browse(repo, query.trim(), skill.dirName);
  };

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

  /**
   * 打开仓库主页。**必须交回系统**：`window.open` 在 Tauri 的 webview 里没有浏览器新窗口，
   * 点了就是没反应（内容中心的资讯卡片踩过同一个坑，那边也只走 `open_external_url`）。
   */
  const openRepo = async () => {
    if (!catalog) return;
    // 技能市场（ClawHub）要求回链到它自己的技能页；GitHub 来源没有这个字段，回落到仓库地址。
    const url = catalog.homepage ?? `https://github.com/${catalog.repo}`;
    try {
      await client.openExternalUrl(url);
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey), 'danger');
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

  /**
   * 目录空态。
   *
   * 只在「真的取到了目录、里面没有技能」时说：取不到目录时说它会让人以为仓库是空的，
   * 那种情况下上面的常驻提醒才是出口。技能市场另说一套——它不是「没选来源」，
   * 而是「这个关键词没搜到」。
   */
  const emptyCatalog = (
    <EmptyState icon={PackageOpen}
      title={t(isRegistry ? 'plugins.market.emptyRegistry.title' : 'plugins.market.empty.title')}
      description={t(isRegistry ? 'plugins.market.emptyRegistry.body' : 'plugins.market.empty.body')} />
  );

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
              /* 市场来源没有 git 提交可言（那个标记是平台自己的版本日期），别写「钉在提交」；
                 它列的是**本次拿到的一页**，条数上限写在核心的 registry 里。 */
              <p className={styles.commit}>{isRegistry
                ? (catalog.total
                    ? t('plugins.source.listedTotal', { total: catalog.total, count: catalog.skills.length })
                    : t('plugins.source.listed', { count: catalog.skills.length }))
                : t('plugins.source.commit', { commit: catalog.commit.slice(0, 8), count: catalog.skills.length })}</p>
            )}
            {/* 提醒与上面的来源行之间要有间距：卡片自身没有行间距，直接相邻会贴在一起。 */}
            {catalogError && (
              <div className={styles.catalogNotice}>
              <Notice tone="warning" summary={catalogSummary(catalogError)}
                details={catalogError.safeDetails[0] ?? t(catalogError.messageKey)}
                actions={<>
                  <button type="button" onClick={() => void browse(repo)}>{t('action.retry')}</button>
                  {tokenHelps(catalogError) && (
                    <button type="button" onClick={() => setTokenDialog(true)}>
                      {tokenConfigured ? t('github.token.update') : t('github.token.set')}
                    </button>
                  )}
                </>} />
              </div>
            )}
          </section>

          {/*
            列表视图与详情视图互斥：列表是卡片网格（点卡片进详情），详情有返回按钮回列表。
            目录报错时两个视图都不画——原因与下一步都在上面那一行提醒里。
          */}
          {detailSkill && current ? (
            <section className={styles.detail} aria-label={t('plugins.detail.label')}>
              <button type="button" className={`${styles.back} text-button`} onClick={() => setDetailSkill(null)}>
                <ChevronLeft size={14} />{t('plugins.backToList')}
              </button>

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
              {markdownOpen && (bodyPending(current)
                ? <p className={styles.fileList}>{t('plugins.documentPending')}</p>
                : <pre className={styles.markdown}>{current.files.find(file => file.path.endsWith('SKILL.md'))?.text ?? current.files[0]?.text}</pre>)}
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
                <button type="button" onClick={() => void openRepo()}>
                  <ExternalLink size={15} />{catalog?.homepage ? t('plugins.openSource') : t('plugins.openRepo')}
                </button>
              </div>
            </section>
          ) : catalogError ? null : (
            /* 列表视图＝技能列表这一块（搜索框 + 卡片网格），与详情视图互斥。 */
            <section className={styles.listView} aria-label={t('plugins.market.listLabel')}>
              {/*
                搜索框不跟加载态一起卸载：技能市场的搜索是一次服务端往返（防抖 400ms），
                请求期间把输入框卸载，焦点与后半截关键词会一起丢；搜不到时空态写着
                「换一个关键词再试」，那时也必须留着这个框。
              */}
              {catalog && (
                <input className={styles.search} value={query} onChange={event => setQuery(event.target.value)}
                  placeholder={t('plugins.searchPlaceholder')} aria-label={t('plugins.search')} />
              )}
              {loadingCatalog ? (
                <div className={styles.empty} role="status" aria-live="polite">{t(isRegistry ? 'plugins.loadingRegistry' : 'plugins.loadingCatalog')}</div>
              ) : catalog && catalog.skills.length > 0 ? (
                <ul className={styles.grid} aria-label={t('plugins.gridLabel')}>
                  {skills.map(skill => {
                    const installedHere = chosen.some(toolId => installedKeys.has(`${skill.document.id}::${toolId}`))
                      || installed.some(record => record.skillId === skill.document.id || record.dirName === skill.dirName);
                    return (
                      <li key={skill.dirName} className={styles.cardItem}>
                        {/*
                          整张卡可点：卡片本体是这一层 li，点击是那枚铺满卡片的透明按钮。
                          不写成「按钮套按钮」——React 会报 validateDOMNesting（button 不能是 button 的
                          后代），那既是无效 HTML，读屏也会把两张卡的名字与操作揉成一团。
                          两个图标按钮是它的兄弟节点、压在点击层之上，所以点它们不会顺带跳进详情。
                        */}
                        <button type="button" className={styles.cardHit} aria-label={skill.document.id}
                          onClick={() => openDetail(skill)} />
                        <span className={styles.cardHead}>
                          <span className={styles.skillName}>{skill.document.id}</span>
                          {installedHere && <span className={styles.installedTag}>{t('plugins.installedTag')}</span>}
                        </span>
                        <span className={styles.skillDescription}>
                          {skill.document.description ?? t('plugins.noDescription')}
                        </span>
                        <span className={styles.cardFoot}>
                          <span className={styles.cardSource}>{skill.sourcePath}</span>
                          <span className={styles.cardActions}>
                            {/* 加号不直接装：走与详情页同一条安装计划弹窗，写文件之前先把
                                「写到哪、装给哪些工具」说清。 */}
                            <button type="button" className={styles.iconAction} aria-label={t('plugins.cardAdd')}
                              onClick={() => void startInstall(skill)}>
                              <Download size={16} aria-hidden="true" />
                            </button>
                            {/* 图标按来源类型分：GitHub 来源用 GitHub 标记，技能市场用它自己的
                                含义（商店）——给市场放 GitHub 图标等于指错地方（ClawHub / SkillHub
                                都不是 GitHub）。两个图标同尺寸同粗细，一行看起来是齐的。 */}
                            <button type="button" className={styles.iconAction} aria-label={t('plugins.cardOpen')}
                              onClick={() => void openRepo()}>
                              {isRegistry
                                ? <Store size={16} aria-hidden="true" />
                                : <Github size={16} aria-hidden="true" />}
                            </button>
                          </span>
                        </span>
                      </li>
                    );
                  })}
                  {!skills.length && <li className={styles.empty}>{t('plugins.noMatch')}</li>}
                </ul>
              ) : (
                emptyCatalog
              )}
            </section>
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

      {tokenDialog && (
        <GithubTokenDialog client={client} configured={tokenConfigured}
          // 存了令牌立刻再抓一次：限额的解法就是它，不自动重试等于让用户自己再点一遍。
          onSaved={next => { setTokenConfigured(next); void browse(repo); }}
          onClose={() => setTokenDialog(false)} />
      )}
    </div>
  );
}
