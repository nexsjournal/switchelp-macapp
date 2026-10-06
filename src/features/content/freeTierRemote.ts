import {
  FREE_TIER_CATEGORIES,
  type FreeTierCatalog,
  type FreeTierCategory,
  type FreeTierClaimFlow,
  type FreeTierEntry,
} from './freeTierPolicy';

/**
 * 在线清单的解析、校验与本地缓存（docs/design/09 §3.1「在线刷新」）。
 *
 * 在线数据来自我们自己仓库里的一份 JSON（Rust 侧固定地址取回原文），但这里
 * 仍按**不可信输入**处理：形状不对、字段越界、URL 不是 https、类别或领取方式
 * 不在白名单里——任何一条不满足就整份丢弃，回落随包清单。宁可少更新，
 * 不渲染半信半疑的数据；这也是断网/坏数据时页面照常可用的原因。
 *
 * 新旧只认 `version`（单调递增，见 freeTierPolicy.ts）：同版本不覆盖，
 * 所以清单热修时必须把 version 一起抬。
 */

/** 自动检查的间隔：打开页面时距上次**成功**检查不足 24 小时就不再请求。失败下次打开重试。 */
export const FREE_TIER_REMOTE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

/** localStorage 键。缓存里只存「上次成功检查的时间」与「已采纳的在线清单」（如有）。 */
export const FREE_TIER_CACHE_KEY = 'switchelp.freeTier.remote.v1';

export interface FreeTierCache {
  /** 上次成功取回在线清单原文的时间（毫秒）。失败不记账，下次打开页面会重试。 */
  attemptedAt: number;
  /** 已采纳的在线清单；还没有采纳过（远端没比随包新）时为 null。 */
  catalog: FreeTierCatalog | null;
}

const CATEGORY_IDS: readonly string[] = FREE_TIER_CATEGORIES.map(item => item.id);
const CLAIM_FLOWS: readonly string[] = ['instant', 'afterLogin', 'needsVerification', 'needsEligibility', 'noAccount'];
const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;
const MAX_ENTRIES = 300;

const isDate = (value: unknown): value is string =>
  typeof value === 'string' && DATE_PATTERN.test(value);
/** 在线条目的链接只认 https：随包清单里也全是 https，收紧不损失任何东西。 */
const isHttpsUrl = (value: unknown): value is string =>
  typeof value === 'string' && value.startsWith('https://') && value.length <= 400;
const isText =
  (max: number) =>
  (value: unknown): value is string =>
    typeof value === 'string' && value.length > 0 && value.length <= max;

function parseEntry(value: unknown): FreeTierEntry | null {
  if (typeof value !== 'object' || value === null) return null;
  const raw = value as Record<string, unknown>;
  if (!isText(80)(raw.id) || !isText(80)(raw.provider)) return null;
  if (!CATEGORY_IDS.includes(raw.category as string)) return null;
  if (!isText(200)(raw.title) || !isText(1000)(raw.quota)) return null;
  if (!isHttpsUrl(raw.docsUrl)) return null;
  if (raw.claimUrl !== undefined && raw.claimUrl !== null && !isHttpsUrl(raw.claimUrl)) return null;
  let claimFlow: FreeTierClaimFlow | undefined;
  if (raw.claimFlow !== undefined && raw.claimFlow !== null) {
    if (!CLAIM_FLOWS.includes(raw.claimFlow as string)) return null;
    claimFlow = raw.claimFlow as FreeTierClaimFlow;
  }
  if (raw.presetId !== undefined && raw.presetId !== null
    && !isText(80)(raw.presetId)) return null;
  if (raw.icon !== undefined && raw.icon !== null && !isText(40)(raw.icon)) return null;
  if (!isDate(raw.lastVerifiedAt)) return null;
  let retired: FreeTierEntry['retired'];
  if (raw.retired !== undefined && raw.retired !== null) {
    if (typeof raw.retired !== 'object' || raw.retired === null) return null;
    const note = (raw.retired as Record<string, unknown>).note;
    if (!isDate((raw.retired as Record<string, unknown>).at) || !isText(300)(note)) return null;
    retired = { at: (raw.retired as Record<string, unknown>).at as string, note: note as string };
  }
  // 未知字段不拒收：远端清单加字段时老版本应用照常工作（只挑认识的）。
  return {
    id: raw.id,
    provider: raw.provider,
    icon: raw.icon ?? undefined,
    category: raw.category as FreeTierCategory,
    title: raw.title,
    quota: raw.quota,
    docsUrl: raw.docsUrl,
    claimUrl: raw.claimUrl ?? undefined,
    claimFlow,
    presetId: raw.presetId ?? undefined,
    retired,
    lastVerifiedAt: raw.lastVerifiedAt,
  };
}

/** 严格校验一份在线清单；任何一条不满足就返回 null（整份丢弃，不做部分采纳）。 */
export function validateCatalog(value: unknown): FreeTierCatalog | null {
  if (typeof value !== 'object' || value === null) return null;
  const raw = value as Record<string, unknown>;
  if (!Number.isInteger(raw.version) || (raw.version as number) < 1 || (raw.version as number) > 1_000_000) return null;
  if (!isDate(raw.verifiedAt)) return null;
  if (!Array.isArray(raw.entries) || raw.entries.length < 1 || raw.entries.length > MAX_ENTRIES) return null;
  const seen = new Set<string>();
  const entries: FreeTierEntry[] = [];
  for (const item of raw.entries) {
    const entry = parseEntry(item);
    if (entry === null || seen.has(entry.id)) return null;
    seen.add(entry.id);
    entries.push(entry);
  }
  return { version: raw.version as number, verifiedAt: raw.verifiedAt, entries };
}

/** 解析在线清单原文；不是合法 JSON 或没通过校验都返回 null。 */
export function parseRemoteCatalog(text: string): FreeTierCatalog | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  return validateCatalog(parsed);
}

export function readCache(): FreeTierCache | null {
  try {
    const raw = window.localStorage.getItem(FREE_TIER_CACHE_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== 'object' || parsed === null) return null;
    const record = parsed as Record<string, unknown>;
    if (typeof record.attemptedAt !== 'number' || !Number.isFinite(record.attemptedAt)) return null;
    return {
      attemptedAt: record.attemptedAt,
      catalog: record.catalog === undefined || record.catalog === null ? null : validateCatalog(record.catalog),
    };
  } catch {
    return null;
  }
}

export function writeCache(cache: FreeTierCache): void {
  try {
    window.localStorage.setItem(FREE_TIER_CACHE_KEY, JSON.stringify(cache));
  } catch {
    // 存不进（隐私模式等）就算了：本次会话内内存里仍是新清单，下次打开回落随包。
  }
}

/** 打开页面时要不要自动检查：从没成功检查过，或距上次已满 24 小时。 */
export function shouldAutoCheck(attemptedAt: number | null, now = Date.now()): boolean {
  return attemptedAt === null || now - attemptedAt >= FREE_TIER_REMOTE_CHECK_INTERVAL_MS;
}
