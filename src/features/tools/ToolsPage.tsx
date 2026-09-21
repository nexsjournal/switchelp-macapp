import { useCallback, useEffect, useMemo, useState } from 'react';
import { ChevronDown, ExternalLink, FolderOpen, RefreshCw, Search } from 'lucide-react';
import type { ToolState } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { showToast } from '@/components/Toast';
import { EmptyState } from '@/components/EmptyState';
import { t, useLocale } from '@/i18n';
import { catalogLocale, categoryLabel, describeProbedAt, monogram, statusBadge } from './toolsPolicy';
import styles from './ToolsPage.module.css';

/**
 * 工具管理（设计 P-T1）。
 *
 * 这一页只做**观察**：本机有哪些 agent 工作台与命令行工具、装在哪、探针怎么说。
 * 安装第三方工具不在本版范围内，所以这一页没有任何安装按钮——
 * 界面上不放做不到的入口（见设计文档 §10 决定 1）。
 *
 * 每个徽章都必须能追到一次真实探测：路径存在、探针退出码、或静态判据文件。
 * 「未验证」与「已就绪」必须看起来不一样，那是这一页最重要的区分。
 */
export function ToolsPage({ client }: { client: DesktopClient }) {
  const locale = useLocale();
  const [tools, setTools] = useState<ToolState[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState('');
  const [query, setQuery] = useState('');
  const [expanded, setExpanded] = useState<string | null>(null);
  /** 「目录不可用」是环境问题，和「扫描失败」要分开显示。 */
  const [catalogBroken, setCatalogBroken] = useState(false);

  const load = useCallback(async (refresh: boolean) => {
    if (refresh) setRefreshing(true);
    try {
      const states = await client.listTools({ refresh, locale: catalogLocale(locale) });
      setTools(states);
      setCatalogBroken(false);
      setError('');
    } catch (cause) {
      const core = toCoreError(cause);
      // 清单加载失败与普通扫描失败的原因不同，分开说，避免用户去查网络。
      if (core.messageKey === 'error.toolCatalogUnavailable' || core.code === 'CATALOG_SCHEMA_MISMATCH') {
        setCatalogBroken(true);
        setError(core.safeDetails[0] ?? t(core.messageKey));
      } else {
        setError(core.safeDetails[0] ?? t(core.messageKey));
      }
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [client, locale]);

  useEffect(() => { void load(false); }, [load]);

  /** 只重探一个工具：整页重扫会连带跑十几个子进程，点一行不该有那个代价。 */
  const probeOne = useCallback(async (toolId: string) => {
    setRefreshing(true);
    try {
      const state = await client.probeTool(toolId, catalogLocale(locale));
      setTools(current => current.map(tool => (tool.id === state.id ? state : tool)));
      showToast(t('tools.probedOne', { name: state.displayName }));
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    } finally {
      setRefreshing(false);
    }
  }, [client, locale]);

  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return tools;
    return tools.filter(tool =>
      `${tool.displayName} ${tool.id} ${tool.description} ${tool.installed?.path ?? ''}`
        .toLocaleLowerCase()
        .includes(needle));
  }, [tools, query]);

  const now = Math.floor(Date.now() / 1000);
  const counts = useMemo(() => ({
    ready: tools.filter(tool => tool.status === 'ready').length,
    installed: tools.filter(tool => tool.status !== 'notInstalled' && tool.status !== 'unsupportedPlatform').length,
    total: tools.length,
  }), [tools]);

  const openLink = (url: string) => {
    // 外链交给系统浏览器：本工具不内嵌浏览器（见桌面壳复审）。
    window.open(url, '_blank', 'noreferrer');
  };

  return (
    <div className={styles.page}>
      {/*
       * 一张卡装完：标题与计数 → 搜索 → 状态说明 → 列表 → 页脚说明。
       * 以前拆成「摘要卡 + 列表卡」两张，两张之间空一大截、列表卡又没有标题，
       * 看上去像两块互不相干的东西。
       */}
      <section className={styles.card}>
        <div className={styles.header}>
          <div>
            <h2>{t('tools.summary.title')}</h2>
            <p className={styles.hint}>
              {t('tools.summary.counts', { installed: counts.installed, ready: counts.ready, total: counts.total })}
            </p>
          </div>
          <button className="icon-button" aria-label={t('tools.refresh')} disabled={refreshing || loading}
            onClick={() => void load(true)}>
            <RefreshCw size={18} className={refreshing ? styles.spin : ''} />
          </button>
        </div>

        {/* 页面级状态留在页面里；动作结果走 Toast。 */}
        {error && <div className="error-message" role="alert">{error}</div>}

        {!error && !catalogBroken && (
          <>
            <div className={styles.searchRow}>
              <Search size={15} />
              <input value={query} onChange={event => setQuery(event.target.value)}
                placeholder={t('tools.searchPlaceholder')} aria-label={t('tools.search')} />
            </div>

            {/*
             * 状态说明默认折叠：这一页最容易被误读的就是「已安装」和「未验证」的区别
             * （一个探针过了、一个没过），把它写清楚比让人猜要便宜得多。
             */}
            <details className={styles.legend}>
              <summary>{t('tools.legend.summary')}</summary>
              <dl>
                {(['ready', 'needsLogin', 'installed', 'unverified', 'notInstalled', 'unsupportedPlatform'] as const).map(id => (
                  <div key={id}>
                    <dt><span className={`badge ${statusBadge(id).tone === 'muted' ? '' : statusBadge(id).tone}`}>{t(statusBadge(id).labelKey)}</span></dt>
                    <dd>{t(`tools.legend.${id}`)}</dd>
                  </div>
                ))}
              </dl>
            </details>
          </>
        )}

        {loading ? (
          <div className={styles.empty} role="status" aria-live="polite">{t('tools.loading')}</div>
        ) : catalogBroken ? (
          <EmptyState title={t('tools.catalogBroken.title')} description={t('tools.catalogBroken.body')} />
        ) : !tools.length ? (
          <EmptyState title={t('tools.empty.title')} description={t('tools.empty.body')} />
        ) : !filtered.length ? (
          <EmptyState title={t('tools.noMatch.title')} description={t('tools.noMatch.body')} />
        ) : (
          <ul className={styles.list}>
            {filtered.map(tool => {
              const badge = statusBadge(tool.status);
              const open = expanded === tool.id;
              return (
                <li key={tool.id} className={styles.row}>
                  <div className={styles.head}>
                    <button type="button" className={styles.toggle} aria-expanded={open}
                      onClick={() => setExpanded(open ? null : tool.id)}>
                      <ChevronDown size={16} className={open ? styles.chevronOpen : styles.chevron} />
                      <span className={styles.tile} aria-hidden="true">{monogram(tool.displayName)}</span>
                      <span className={styles.identity}>
                        <span className={styles.nameLine}>
                          <span className={styles.name}>{tool.displayName}</span>
                          <span className={styles.category}>{t(categoryLabel(tool.category))}</span>
                        </span>
                        {/* 不展开也能知道它是什么、装在哪——这一行是这一页最常被读的内容。 */}
                        <span className={`${styles.fact} text-mono break-anywhere`}>
                          {tool.installed
                            ? `${tool.installed.path}${tool.installed.version ? ` · ${tool.installed.version}` : ''}`
                            : t(tool.status === 'unsupportedPlatform' ? 'tools.value.unsupported' : 'tools.value.notFound')}
                        </span>
                      </span>
                    </button>
                    <span className={`badge ${badge.tone === 'muted' ? '' : badge.tone}`}>{t(badge.labelKey)}</span>
                    <span className={styles.probedAt}>
                      {t('tools.probedAt', { when: describeProbedAt(tool.probedAt, now, locale) })}
                    </span>
                  </div>

                  {open && (
                    <div className={styles.detail}>
                      <p className={styles.description}>{tool.description}</p>
                      <dl className={styles.facts}>
                        <dt>{t('tools.field.version')}</dt>
                        <dd className="text-mono">{tool.installed?.version ?? t('tools.value.unknown')}</dd>
                        <dt>{t('tools.field.config')}</dt>
                        <dd className="text-mono break-anywhere">
                          {tool.installed?.configPath
                            ? `${tool.installed.configPath}${tool.installed.configExists ? '' : ` · ${t('tools.value.missing')}`}`
                            : t('tools.value.notApplicable')}
                        </dd>
                        <dt>{t('tools.field.skills')}</dt>
                        <dd className="text-mono">
                          {tool.installed?.skillsPath
                            ? `${tool.installed.skillsCount} · ${tool.installed.skillsPath}`
                            : t('tools.value.notApplicable')}
                        </dd>
                        <dt>{t('tools.field.pathSource')}</dt>
                        <dd>{t(tool.installed?.pathSource === 'path' ? 'tools.pathSource.path' : 'tools.pathSource.candidate')}</dd>
                      </dl>

                      {tool.notes.length > 0 && (
                        <ul className={styles.notes}>
                          {tool.notes.map(note => <li key={note}>{note}</li>)}
                        </ul>
                      )}

                      {/*
                       * 下面的每一节都自带标题与上方分割线：展开之后是一段有结构的长内容，
                       * 靠留白分不出「事实表 / 用法 / 探针原文」的边界（用户原话：
                       * 「展开后…我都看不到它下面是怎么区分分隔的，有的间距都快挨住了」）。
                       */}
                      {tool.agentUsage.nonInteractive.length > 0 && (
                        <section className={styles.block}>
                          <h3>{t('tools.field.agentUsage')}</h3>
                          <ul className={styles.commands}>{tool.agentUsage.nonInteractive.map(line => <li key={line} className="text-mono">{line}</li>)}</ul>
                        </section>
                      )}

                      <section className={styles.block}>
                        <h3>{t('tools.field.probeOutput')}</h3>
                        <pre>{tool.versionProbeTail || t('tools.value.noOutput')}</pre>
                        {tool.authProbeTail !== null && tool.authProbeTail !== undefined && (
                          <>
                            <h4>{t('tools.field.authProbe')}</h4>
                            <pre>{tool.authProbeTail || t('tools.value.noOutput')}</pre>
                          </>
                        )}
                      </section>

                      {/* 动作与它上面的内容用一条线分开：这不是又一个信息块，是可以点的出口。 */}
                      <div className={styles.detailActions}>
                        <button type="button" onClick={() => void probeOne(tool.id)} disabled={refreshing}>
                          <RefreshCw size={15} />{t('tools.probeThis')}
                        </button>
                        {tool.installed?.path && (
                          <button type="button" onClick={() => {
                            // 「在访达里显示」需要宿主能力；本版只用系统浏览器与复制路径两种确定可行的动作。
                            void navigator.clipboard?.writeText(tool.installed!.path);
                            showToast(t('tools.pathCopied'));
                          }}>
                            <FolderOpen size={15} />{t('tools.copyPath')}
                          </button>
                        )}
                        {tool.docs && (
                          <button type="button" onClick={() => openLink(tool.docs!)}>
                            <ExternalLink size={15} />{t('tools.openDocs')}
                          </button>
                        )}
                      </div>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        )}

        <p className={styles.footer}>{t('tools.openToolsNote')}</p>
      </section>
    </div>
  );
}
