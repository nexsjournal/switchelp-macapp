import type { DiscoveredModel } from '@/desktop/client';
import { type DesktopClient } from '@/desktop/client';
import { recommendedPolicy } from '@/features/models/policy';

import { t } from '@/i18n';
import { showToast } from '@/components/Toast';

/**
 * 把「获取可用模型」勾选弹窗确认的那一份清单写进库。
 *
 * 两个调用方共用：供应商弹窗里的「获取可用模型」，以及方案一的
 * 「预设来源 + 填了 Key 的保存 → 自动接上发现」（见 docs/design/08）。
 * 上游不返回长度，统一用推荐默认值写入，之后逐个核对；
 * 发现出来的模型一律跟随供应商协议（上游列表不会告诉我们它走哪套协议）。
 */
export async function addDiscoveredModels(
  client: DesktopClient,
  providerId: string,
  selected: DiscoveredModel[],
  onChanged: () => Promise<void> | void,
): Promise<void> {
  for (const model of selected) {
    await client.saveModel({
      providerId, upstreamId: model.upstreamId,
      displayName: model.displayName || model.upstreamId, catalogAlias: '',
      policy: recommendedPolicy(), inCatalog: true, displayNameOverridden: false,
      protocolOverride: null,
    }, 0);
  }
  await onChanged();
  showToast(t('providers.discoverAdded', { count: selected.length }));
}
