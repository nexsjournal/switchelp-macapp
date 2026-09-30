import { useState } from 'react';
import { SegmentedTabs } from '@/components/SegmentedTabs';
import { type DesktopClient } from '@/desktop/client';
import { FREE_TIER_CATALOG } from './freeTierData';
import { FREE_TIER_CATEGORIES, type FreeTierCategory } from './freeTierPolicy';
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
 * 数据是随包的静态清单（freeTierData.ts），不走内容中心的抓取调度：
 * 每张卡必显「核实于」日期，过期 = 改清单随版本发布（第 2 期接在线刷新）。
 */
export function FreeTierPage({ client, onAccessInGateway }: {
  client: DesktopClient;
  /** 「在网关中接入」：带着预设 id 跳去网关页，打开预选好预设的添加供应商弹窗。 */
  onAccessInGateway?: (presetId: string) => void;
}) {
  const [category, setCategory] = useState<'all' | FreeTierCategory>('all');

  return (
    <div className={styles.page}>
      <section className={styles.head}>
        <p className={styles.disclaimer}>{t('content.freeTier.disclaimer')}</p>
        <span className={styles.version}>{t('content.freeTier.version', {
          version: FREE_TIER_CATALOG.version, date: FREE_TIER_CATALOG.verifiedAt })}</span>
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
          {FREE_TIER_CATALOG.entries
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
