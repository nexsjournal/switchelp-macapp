import { describe, expect, it } from 'vitest';

import { FREE_TIER_CATALOG } from './freeTierData';
import {
  parseRemoteCatalog,
  readCache,
  shouldAutoCheck,
  validateCatalog,
  writeCache,
  FREE_TIER_CACHE_KEY,
} from './freeTierRemote';

/**
 * 在线清单的校验器与缓存（docs/design/09 §3.1「在线刷新」）。
 *
 * 校验器是「宁可少更新，不渲染半信半疑的数据」的落点：在线 JSON 按**不可信输入**
 * 处理，任何一条不满足就整份丢弃回落随包。这里的用例就是那条边界本身。
 */

/** 在随包清单基础上造一版「远端更新」：抬 version + 换核实日期，其余不动。 */
function remoteCatalog(overrides: Partial<typeof FREE_TIER_CATALOG> = {}): typeof FREE_TIER_CATALOG {
  return { ...FREE_TIER_CATALOG, version: FREE_TIER_CATALOG.version + 1, verifiedAt: '2026-10-06', ...overrides };
}

/** 把一个字段改成非法值后整份应被拒收。 */
function withEntryField(mutate: (entry: Record<string, unknown>) => void) {
  const catalog = remoteCatalog();
  const entry = { ...catalog.entries[0]! } as unknown as Record<string, unknown>;
  mutate(entry);
  return { ...catalog, entries: [entry, ...catalog.entries.slice(1)] };
}

describe('在线清单校验器', () => {
  it('随包清单本身必须通过校验（远端与随包同源同构）', () => {
    expect(validateCatalog(FREE_TIER_CATALOG)).toEqual(FREE_TIER_CATALOG);
  });

  it('抬了 version 的合法远端清单通过，且 version 原样保留', () => {
    const parsed = parseRemoteCatalog(JSON.stringify(remoteCatalog()));
    expect(parsed).not.toBeNull();
    expect(parsed!.version).toBe(FREE_TIER_CATALOG.version + 1);
  });

  it('不是 JSON、不是对象、缺 entries 都拒收', () => {
    expect(parseRemoteCatalog('not json')).toBeNull();
    expect(parseRemoteCatalog('"a string"')).toBeNull();
    expect(parseRemoteCatalog('{"version":4,"verifiedAt":"2026-10-06"}')).toBeNull();
  });

  it('version 必须是正整数', () => {
    expect(validateCatalog(remoteCatalog({ version: 3.5 }))).toBeNull();
    expect(validateCatalog(remoteCatalog({ version: 0 }))).toBeNull();
    expect(validateCatalog(remoteCatalog({ version: -1 }))).toBeNull();
  });

  it('日期字段不符合 YYYY-MM-DD 拒收', () => {
    expect(validateCatalog(remoteCatalog({ verifiedAt: '2026年10月' }))).toBeNull();
    expect(validateCatalog(withEntryField(entry => { entry.lastVerifiedAt = '昨天'; }))).toBeNull();
  });

  it('类别与领取方式不在白名单里拒收', () => {
    expect(validateCatalog(withEntryField(entry => { entry.category = '限时折扣'; }))).toBeNull();
    expect(validateCatalog(withEntryField(entry => { entry.claimFlow = '自动代领'; }))).toBeNull();
  });

  it('链接只认 https', () => {
    expect(validateCatalog(withEntryField(entry => { entry.docsUrl = 'http://example.com/docs'; }))).toBeNull();
    expect(validateCatalog(withEntryField(entry => { entry.claimUrl = 'javascript:alert(1)'; }))).toBeNull();
  });

  it('重复的条目 id 拒收', () => {
    const catalog = remoteCatalog();
    const duplicate = { ...catalog.entries[0]!, id: 'duplicate' };
    const clash = { ...catalog.entries[1]!, id: 'duplicate' };
    expect(validateCatalog({ ...catalog, entries: [duplicate, clash] })).toBeNull();
  });

  it('文本字段超长拒收', () => {
    expect(validateCatalog(withEntryField(entry => { entry.quota = '长'.repeat(1001); }))).toBeNull();
    expect(validateCatalog(withEntryField(entry => { entry.provider = ''; }))).toBeNull();
  });

  it('retired 形状不对拒收；未知字段不拒收（远端加字段时老版本照常工作）', () => {
    expect(validateCatalog(withEntryField(entry => { entry.retired = { at: 'not-a-date', note: 'x' }; }))).toBeNull();
    const withExtra = { ...remoteCatalog(), futureField: { anything: true } };
    expect(validateCatalog(withExtra)).not.toBeNull();
  });
});

describe('在线清单缓存', () => {
  it('写入后能读回；坏缓存（形状不对）读出来按目录处理', () => {
    localStorage.clear();
    writeCache({ attemptedAt: 1_700_000_000_000, catalog: remoteCatalog() });
    const cache = readCache();
    expect(cache?.attemptedAt).toBe(1_700_000_000_000);
    expect(cache?.catalog?.version).toBe(FREE_TIER_CATALOG.version + 1);

    localStorage.setItem(FREE_TIER_CACHE_KEY, '{broken');
    expect(readCache()).toBeNull();
    localStorage.setItem(FREE_TIER_CACHE_KEY, JSON.stringify({ attemptedAt: 'yesterday' }));
    expect(readCache()).toBeNull();
    // 缓存里的清单过不了校验也不采纳（catalog 置空，但「查过的时间」还在）。
    localStorage.setItem(FREE_TIER_CACHE_KEY, JSON.stringify({ attemptedAt: 1, catalog: { junk: true } }));
    expect(readCache()).toEqual({ attemptedAt: 1, catalog: null });
  });

  it('自动检查的节流：没查过或超过 24 小时要查，刚查过不查', () => {
    expect(shouldAutoCheck(null)).toBe(true);
    const now = Date.now();
    expect(shouldAutoCheck(now - 25 * 60 * 60 * 1000, now)).toBe(true);
    expect(shouldAutoCheck(now - 1 * 60 * 60 * 1000, now)).toBe(false);
  });
});
