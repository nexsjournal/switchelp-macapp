import { describe, expect, it } from 'vitest';

import { defaultPolicy, inputLabel, modelDraft, parseTokens, policyFromForm } from './policy';
import type { Model } from '@/contracts/types';

/**
 * 保存前的数据闸门。
 *
 * 这里过去没有任何测试，而它决定了写进 Codex 目录与网关请求的值：Token 解析、输出上限
 * 与上下文的关系、思考档位与默认值是否自洽。Rust 侧对应规则有测试，界面这一侧没有。
 */

function form(fields: Record<string, string>): FormData {
  const data = new FormData();
  for (const [key, value] of Object.entries(fields)) data.set(key, value);
  return data;
}

describe('Token 解析', () => {
  it('接受常见写法与分隔符', () => {
    expect(parseTokens('128000')).toBe(128_000);
    expect(parseTokens('128k')).toBe(128_000);
    expect(parseTokens('128K')).toBe(128_000);
    expect(parseTokens('1m')).toBe(1_000_000);
    expect(parseTokens('1M')).toBe(1_000_000);
    // 二进制单位与十进制单位是两回事，不能混为一谈。
    expect(parseTokens('64ki')).toBe(65_536);
    expect(parseTokens('1mi')).toBe(1_048_576);
    expect(parseTokens(' 1,000 ')).toBe(1_000);
    expect(parseTokens('1_000')).toBe(1_000);
    expect(parseTokens('')).toBeNull();
    expect(parseTokens('   ')).toBeNull();
  });

  it('拒绝非法或越界的值，而不是静默取一个近似值', () => {
    expect(() => parseTokens('12k8')).toThrow(/正整数/);
    expect(() => parseTokens('-5')).toThrow(/正整数/);
    expect(() => parseTokens('0')).toThrow(/1 到 2,147,483,647/);
    // 上界是 2^31-1：超出后网关与宿主都无法表达。
    expect(parseTokens('2147483647')).toBe(2_147_483_647);
    expect(() => parseTokens('2147483648')).toThrow(/1 到 2,147,483,647/);
  });

  it('输入能力的名称跟着语言走，不冻结在启动语言上', () => {
    expect(inputLabel('text')).toBe('文本');
    expect(inputLabel('pdf')).toBe('PDF');
  });
});

describe('表单到策略', () => {
  it('输出上限必须小于上下文窗口', () => {
    expect(() => policyFromForm(form({ contextLimit: '128k', outputLimit: '128k' })))
      .toThrow(/最大输出必须小于上下文窗口/);
    expect(() => policyFromForm(form({ contextLimit: '128k', outputLimit: '200k' })))
      .toThrow(/最大输出必须小于上下文窗口/);
    const policy = policyFromForm(form({ contextLimit: '128k', outputLimit: '8k' }));
    expect(policy.contextLimit).toBe(128_000);
    // 'k' 是十进制（8000）；二进制要用 'ki'（8192）。
    expect(policy.outputLimit).toBe(8_000);
  });

  it('默认思考档位必须属于已声明档位', () => {
    expect(() => policyFromForm(form({
      contextLimit: '128k', reasoningSupport: 'supported', reasoningControl: 'effort',
      allowedValues: 'low, high', defaultValue: 'medium',
    }))).toThrow(/默认思考档位/);

    const policy = policyFromForm(form({
      contextLimit: '128k', reasoningSupport: 'supported', reasoningControl: 'effort',
      allowedValues: 'low, high', defaultValue: 'high',
    }));
    expect(policy.reasoning.allowedValues).toEqual(['low', 'high']);
    expect(policy.reasoning.defaultValue).toBe('high');
  });

  it('未声明支持时不保留档位与预算，避免留下无效组合', () => {
    const policy = policyFromForm(form({
      contextLimit: '128k', reasoningSupport: 'unsupported', reasoningControl: 'budget',
      allowedValues: 'low', defaultValue: 'low', budgetTokens: '4096',
    }));
    expect(policy.reasoning.control).toBe('none');
    expect(policy.reasoning.allowedValues).toEqual([]);
    expect(policy.reasoning.defaultValue).toBeNull();
    expect(policy.reasoning.budgetTokens).toBeNull();
  });

  it('归档草稿时把 ID、别名与目录归属按契约映射', () => {
    const model = {
      id: 'm_1', providerId: 'p_a', upstreamId: 'vendor/x', catalogAlias: 'gs/m_1', displayName: '甲',
      lifecycle: 'saved', hostState: 'loaded', inCatalog: true, policy: defaultPolicy(),
      displayNameLayer: { discovered: null, userValue: '甲', overridden: true },
      capabilityRevision: 1, version: 7, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
    } as Model;

    // 改归属：只动 inCatalog，其余字段（含版本号）原样带上。
    const draft = modelDraft(model, { inCatalog: false });
    expect(draft.inCatalog).toBe(false);
    expect(draft.id).toBe('m_1');
    expect(draft.catalogAlias).toBe('gs/m_1');
    expect(draft.upstreamId).toBe('vendor/x');
    expect(draft.policy).toEqual(model.policy);
  });
});
