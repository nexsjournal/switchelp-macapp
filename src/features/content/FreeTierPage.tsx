import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { SegmentedTabs } from '@/components/SegmentedTabs';
import { showToast } from '@/components/Toast';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { FREE_TIER_CATALOG } from './freeTierData';
import { FREE_TIER_CATEGORIES, type FreeTierCatalog, type FreeTierCategory } from './freeTierPolicy';
import { parseRemoteCatalog, readCache, shouldAutoCheck, writeCache } from './freeTierRemote';
import { freeTierIcon } from './freeTierIcons';
import styles from './FreeTierPage.module.css';

import { t } from '@/i18n';

/**
 * 免费额度页（docs/design/09）。
 *
 * 只做信息的汇集：把厂商官方公开发布的免费档/试用金汇成一份可读可点的清单，
 * 领取动作发生在官方站点（openExternalUrl 跳转，桌面层只放行 http/https）。
 * 不代领、不托管 Key、不收录共享凭据——页面顶部的声明就是产品立场，不是装饰。
 *
 * 数据默认是随包静态清单（freeTierData.ts），核实日期跟着发版走；在线刷新
 * （freeTierRemote.ts）让核实能在两次发版之间先行：打开页面超过 24 小时没查过
 * 就静默查一次，页头的「检查更新」随时手动查。远端只认 version 更新的一版，
 * 拉不到/数据坏了就继续用随包的——页面永远有内容，只是新旧不同。
 */
export function FreeTierPage({ client, onAccessInGateway }: {
  client: DesktopClient;
  /** 「在网关中接入」：带着预设 id 跳去网关页，打开预选好预设的添加供应商弹窗。 */
  onAccessInGateway?: (presetId: string) => void;
}) {
  const [category, setCategory] = useState<'all' | FreeTierCategory>('all');
  // 上次会话采纳过更新的清单就直接上路（只认比随包新的，version 门槛见 freeTierRemote）。
  const cached = useMemo(() => {
    const cache = readCache();
    return cache !== null && cache.catalog !== null && cache.catalog.version > FREE_TIER_CATALOG.version
      ? cache.catalog
      : null;
  }, []);
  const [catalog, setCatalog] = useState<FreeTierCatalog>(cached ?? FREE_TIER_CATALOG);
  const [fromRemote, setFromRemote] = useState(cached !== null);
  const [checking, setChecking] = useState(false);
  // 自动检查与手动按钮共用一个闸：同时只允许一个在飞。catalogRef 让回调里
  // 拿得到最新版本号，不把 refresh 挂进依赖。
  const checkingRef = useRef(false);
  const catalogRef = useRef(catalog);
  useEffect(() => { catalogRef.current = catalog; }, [catalog]);

  const refresh = useCallback(async (manual: boolean) => {
    if (checkingRef.current) return;
    checkingRef.current = true;
    if (manual) setChecking(true);
    try {
      const parsed = parseRemoteCatalog(await client.fetchFreeTierCatalog());
      if (parsed !== null && parsed.version > catalogRef.current.version) {
        setCatalog(parsed);
        setFromRemote(true);
        writeCache({ attemptedAt: Date.now(), catalog: parsed });
        showToast(t('content.freeTier.toast.updated', { version: parsed.version, date: parsed.verifiedAt }));
      } else {
        // 没有更新（或数据没过校验）：只记「查过」的时间让自动检查歇 24 小时；
        // 已采纳的缓存清单保持不动。手动点的时候要给个交代，自动的保持安静。
        writeCache({ attemptedAt: Date.now(), catalog: readCache()?.catalog ?? null });
        if (manual) {
          showToast(parsed !== null
            ? t('content.freeTier.toast.upToDate', { version: catalogRef.current.version })
            : t('content.freeTier.toast.failed'), parsed !== null ? 'success' : 'danger');
        }
      }
    } catch (cause) {
      if (manual) {
        const core = toCoreError(cause);
        showToast(core.safeDetails[0] ?? t(core.messageKey), 'danger');
      }
    } finally {
      checkingRef.current = false;
      if (manual) setChecking(false);
    }
  }, [client]);

  useEffect(() => {
    if (shouldAutoCheck(readCache()?.attemptedAt ?? null)) void refresh(false);
  }, [refresh]);

  return (
    <div className={styles.page}>
      <section className={styles.head}>
        <p className={styles.disclaimer}>{t('content.freeTier.disclaimer')}</p>
        <span className={styles.versionBox}>
          <span className={styles.version}>{t(fromRemote ? 'content.freeTier.versionRemote' : 'content.freeTier.version', {
            version: catalog.version, date: catalog.verifiedAt })}</span>
          <button type="button" className={styles.refresh} disabled={checking} aria-busy={checking}
            onClick={() => void refresh(true)}>
            {t(checking ? 'content.freeTier.checking' : 'content.freeTier.checkUpdate')}
          </button>
        </span>
      </section>
      {/* 类别筛选与页签同一个分段控件，不再另画一套。 */}
      <SegmentedTabs ariaLabel={t('content.freeTier.categories')} active={category}
        onChange={id => setCategory(id as 'all' | FreeTierCategory)}
        tabs={[
          { id: 'all', label: t('content.freeTier.cat.all') },
          ...FREE_TIER_CATEGORIES.map(item => ({ id: item.id, label: t(item.labelKey) })),
        ]} />
      <section className={styles.card}>
        <ul className={styles.grid}>
          {catalog.entries
            .filter(entry => category === 'all' || entry.category === category)
            .map(entry => (
              <li key={entry.id}>
                <article className={`${styles.card__inner} ${entry.retired ? styles.retired : ''}`}>
                  <header className={styles.head_row}>
                    <span className={styles.icon} aria-hidden="true">
                      {freeTierIcon(entry.icon) ?? <span className={styles.monogram}>{entry.provider.slice(0, 1)}</span>}
                    </span>
                    <span className={styles.provider}>{entry.provider}</span>
                    {entry.retired && <span className="badge">{t('content.freeTier.retiredBadge')}</span>}
                  </header>
                  <p className={styles.title}>{entry.title}</p>
                  <p className={styles.quota}>{entry.quota}</p>
                  {/* 落点预期：点「去领取」之后是登录就有、还是要实名/绑卡——先说清，别让用户白跑。 */}
                  {!entry.retired && entry.claimFlow && (
                    <p className={styles.flow}>{t(`content.freeTier.flow.${entry.claimFlow}`)}</p>
                  )}
                  {entry.retired && <p className={styles.note}>{t('content.freeTier.retired', {
                    date: entry.retired.at, note: entry.retired.note })}</p>}
                  <div className={styles.actions}>
                    {!entry.retired && entry.claimUrl
                      && <button type="button" className="primary" onClick={() => void client.openExternalUrl(entry.claimUrl!)}>
                        {/* 本机离线那档没有「领取」可言，按钮说的是下载。 */}
                        {entry.category === 'local' ? t('content.freeTier.download') : t('content.freeTier.claim')}</button>}
                    <button type="button" onClick={() => void client.openExternalUrl(entry.docsUrl)}>
                      {t('content.freeTier.docs')}</button>
                    {!entry.retired && entry.presetId && onAccessInGateway
                      && <button type="button" onClick={() => onAccessInGateway(entry.presetId!)}>
                        {t('content.freeTier.gateway')}</button>}
                  </div>
                  <span className={styles.verified}>{t('content.freeTier.verified', { date: entry.lastVerifiedAt })}</span>
                </article>
              </li>
            ))}
        </ul>
      </section>
    </div>
  );
}
