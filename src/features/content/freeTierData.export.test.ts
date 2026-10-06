import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { FREE_TIER_CATALOG } from './freeTierData';
import { parseRemoteCatalog } from './freeTierRemote';

/**
 * 在线清单导出件的守卫（docs/design/09 §3.1「在线刷新」）。
 *
 * 在线供体是随包 TS 的**导出件**：`freeTierData.json` 必须与 FREE_TIER_CATALOG
 * 逐字节一致，谁改了清单忘了重新导出，这里立刻红。重新导出：
 *
 *   FREE_TIER_EXPORT_UPDATE=1 pnpm vitest run src/features/content/freeTierData.export.test.ts
 *
 * 导出件提交进仓库后，应用的免费额度页会从仓库 raw main 拉它
 * （地址在 src-tauri 的 FREE_TIER_CATALOG_URL）。**改清单必须连 version 一起抬**：
 * 界面只认 version 更新的一版，同版本不覆盖。
 */
const FILE = join(dirname(fileURLToPath(import.meta.url)), 'freeTierData.json');

describe('免费额度清单导出件', () => {
  it('freeTierData.json 与随包清单一致（改了清单就跑 FREE_TIER_EXPORT_UPDATE=1 重新导出）', () => {
    const expected = `${JSON.stringify(FREE_TIER_CATALOG, null, 2)}\n`;
    if (process.env.FREE_TIER_EXPORT_UPDATE === '1') {
      writeFileSync(FILE, expected);
      return;
    }
    expect(readFileSync(FILE, 'utf8')).toBe(expected);
  });

  it('导出件能通过在线校验器——远端数据走的就是这条路', () => {
    expect(parseRemoteCatalog(readFileSync(FILE, 'utf8'))).toEqual(FREE_TIER_CATALOG);
  });
});
