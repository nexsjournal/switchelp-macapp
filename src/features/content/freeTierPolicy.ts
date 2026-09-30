/** 免费额度条目与清单的形状（docs/design/09 §3.1）。数据在 freeTierData.ts。 */
export type FreeTierCategory = 'model_free_tier' | 'trial_credit' | 'student_dev' | 'local';

export interface FreeTierEntry {
  id: string;
  provider: string;
  /** 对应 @lobehub/icons 的 id（freeTierIcons.tsx 的映射键）；缺省回退首字方块。 */
  icon?: string;
  category: FreeTierCategory;
  title: string;
  /** 官方文档里写明的限额（换算原话，不写营销话术）。 */
  quota: string;
  /** 写明限额的官方文档页——**必填**，没有官方背书的免费额度不收录。 */
  docsUrl: string;
  /** 领取/注册/控制台页；「没有免费档」这类反例条目没有。 */
  claimUrl?: string;
  /** 与网关预设目录联动：有值时卡片出现「在网关中接入」。 */
  presetId?: string;
  /** 退役条目置灰保留：它是「别再信旧攻略」的教育样本，不静默删除。 */
  retired?: { at: string; note: string };
  lastVerifiedAt: string;
}

export interface FreeTierCatalog {
  /** 清单版本，单调递增；在线刷新（第 2 期）以它判断新旧。 */
  version: number;
  verifiedAt: string;
  entries: FreeTierEntry[];
}

export const FREE_TIER_CATEGORIES: { id: FreeTierCategory; labelKey: string }[] = [
  { id: 'model_free_tier', labelKey: 'content.freeTier.cat.modelFreeTier' },
  { id: 'trial_credit', labelKey: 'content.freeTier.cat.trialCredit' },
  { id: 'student_dev', labelKey: 'content.freeTier.cat.studentDev' },
  { id: 'local', labelKey: 'content.freeTier.cat.local' },
];
