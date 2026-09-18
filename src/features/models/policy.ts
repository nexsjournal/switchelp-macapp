import type { InputKind, Model, ModelPolicy, Support } from '@/contracts/types';
import type { ModelDraft } from '@/desktop/client';

export const inputLabels: Record<InputKind, string> = { text: '文本', image: '图像', audio: '音频', video: '视频', pdf: 'PDF', document: '其他文档' };
export const inputKinds = Object.keys(inputLabels) as InputKind[];

export function defaultPolicy(): ModelPolicy {
  return { contextLimit: null, outputLimit: null, compactLimit: null,
    reasoning: { support: 'unknown', control: 'none', allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
    inputs: inputKinds.map(kind => ({ kind, upstream: kind === 'text' ? 'supported' : 'unknown', gateway: 'unknown', host: 'unknown',
      effectivePath: 'blocked', mimeTypes: [], maxBytes: null, conversionId: null, verification: 'declared', blockedReasonKey: null })),
    tools: { functionTools: 'unknown', parallelTools: 'unknown', customTools: 'unknown', verification: 'declared' } };
}

export function parseTokens(value: string): number | null {
  const input = value.trim().replace(/[,_\s]/g, '');
  if (!input) return null;
  const match = /^(\d+)(ki|mi|k|m)?$/i.exec(input);
  if (!match) throw new Error('Token 数值须为正整数，可使用 32k、128k 等写法。');
  const multiplier = ({ k: 1000, m: 1000000, ki: 1024, mi: 1048576 } as Record<string, number>)[match[2]?.toLowerCase() ?? ''] ?? 1;
  const result = Number(match[1]) * multiplier;
  if (!Number.isSafeInteger(result) || result <= 0 || result > 2147483647) throw new Error('Token 数值须在 1 到 2,147,483,647 之间。');
  return result;
}

export function policyFromForm(data: FormData, previous = defaultPolicy()): ModelPolicy {
  const support = data.get('reasoningSupport') as Support;
  const control = data.get('reasoningControl') as ModelPolicy['reasoning']['control'];
  const allowedValues = support === 'supported' ? String(data.get('allowedValues') ?? '').split(/[,，\s]+/).filter(Boolean) : [];
  const policy: ModelPolicy = { ...previous,
    contextLimit: parseTokens(String(data.get('contextLimit') ?? '')),
    outputLimit: parseTokens(String(data.get('outputLimit') ?? '')),
    compactLimit: parseTokens(String(data.get('compactLimit') ?? '')),
    reasoning: { support, control: support === 'supported' ? control : 'none', allowedValues,
      defaultValue: support === 'supported' ? String(data.get('defaultValue') ?? '').trim() || null : null,
      budgetTokens: support === 'supported' && control === 'budget' ? parseTokens(String(data.get('budgetTokens') ?? '')) : null,
      mappingId: null },
    inputs: previous.inputs.map(input => ({ ...input, upstream: data.get(`input-${input.kind}`) as Support })),
    tools: { functionTools: data.get('functionTools') as Support, parallelTools: data.get('parallelTools') as Support,
      customTools: 'unknown', verification: 'declared' },
  };
  if (policy.contextLimit !== null && policy.outputLimit !== null && policy.outputLimit >= policy.contextLimit) throw new Error('最大输出必须小于上下文窗口。');
  if (policy.reasoning.defaultValue && !allowedValues.includes(policy.reasoning.defaultValue)) throw new Error('默认思考档位必须属于支持的档位。');
  return policy;
}

/** 由已有模型构造保存草稿。界面上的「纳入 / 移出目录」等快捷操作复用它，
 *  避免与编辑器各写一份字段映射。 */
export function modelDraft(model: Model, overrides: Partial<ModelDraft> = {}): ModelDraft {
  return {
    id: model.id,
    providerId: model.providerId,
    upstreamId: model.upstreamId,
    catalogAlias: model.catalogAlias,
    displayName: model.displayName,
    policy: model.policy,
    inCatalog: model.inCatalog,
    displayNameOverridden: true,
    ...overrides,
  };
}
