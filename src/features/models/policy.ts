import type { InputKind, Model, ModelPolicy, Support } from '@/contracts/types';
import { t } from '@/i18n';
import type { ModelDraft } from '@/desktop/client';

/** 输入能力只存文案键：模块只加载一次，存文案会被冻结在启动时的语言上。 */
const inputLabelKeys: Record<InputKind, string> = {
  text: 'capability.text', image: 'capability.image', audio: 'capability.audio',
  video: 'capability.video', pdf: 'capability.pdf', document: 'capability.document',
};
export const inputKinds = Object.keys(inputLabelKeys) as InputKind[];

export function inputLabel(kind: InputKind): string {
  return t(inputLabelKeys[kind]);
}

/**
 * 当前链路不能原生发送的输入（需求 R22：可见但不可启用）。
 * 这是接入方式的硬边界，不随声明变化——核心层本来就会把它们排除出宿主能力。
 */
const BLOCKED_INPUT_KINDS: ReadonlySet<InputKind> = new Set(['pdf', 'video']);

export function inputBlocked(kind: InputKind): boolean {
  return BLOCKED_INPUT_KINDS.has(kind);
}

/** 模型在 Codex 宿主侧的状态文案；供应商弹窗与模型目录共用一份。 */
export const hostStateKeys: Record<Model['hostState'], string> = {
  not_in_catalog: 'models.hostNotInCatalog',
  pending_apply: 'host.pendingApply',
  awaiting_reload: 'models.hostAwaitingReload',
  loaded: 'host.loaded',
  load_unconfirmed: 'host.loadUnconfirmed',
};

/**
 * 宿主状态的语义色。设计规范里语义色与强调色是两套，状态不借强调色表达，
 * 所以这里只映射到成功 / 警告，其余状态保持中性——「不纳入目录」不是问题，
 * 「无法确认」也不该用一个颜色替用户下结论。
 */
export function hostStateVariant(state: Model['hostState']): 'success' | 'warning' | '' {
  if (state === 'loaded') return 'success';
  if (state === 'pending_apply' || state === 'awaiting_reload') return 'warning';
  return '';
}

/**
 * 模型行的连接状态：最近一次「测试」的结果。
 *
 * 三种取值里 `untested` 是**中性**的：没有证据不等于有问题。这一点与
 * [hostStateVariant] 同一个道理——颜色不用来替用户下结论。
 */
export type ConnectionState = 'passed' | 'failed' | 'untested';

export const connectionKeys: Record<ConnectionState, string> = {
  passed: 'models.connectionPassed',
  failed: 'models.connectionFailed',
  untested: 'models.connectionUntested',
};

export function connectionVariant(state: ConnectionState): 'success' | 'danger' | '' {
  if (state === 'passed') return 'success';
  if (state === 'failed') return 'danger';
  return '';
}

/**
 * 「这个模型有没有加进 Codex 目录」的状态点。
 *
 * 已加入＝成功色，未加入保持中性：没纳入目录是一种选择，不是错误（与
 * [hostStateVariant] 对 `not_in_catalog` 的处理一致）。
 */
export function catalogVariant(inCatalog: boolean): 'success' | '' {
  return inCatalog ? 'success' : '';
}

/**
 * 一个供应商的 Codex 状态：由它旗下模型的状态聚合而来。
 *
 * 用在供应商卡片上——「已加载」是**供应商这一层**的信息（用户原话：应该在左边卡片里提示），
 * 逐行重复没有意义。聚合规则要能一眼解释：有任何待应用的就说待应用；否则有等待重载的就说
 * 等待重载；否则一条都没纳入目录就说未纳入；剩下的才是已加载。
 */
export function providerHostState(own: Model[]): Model['hostState'] | null {
  if (!own.length) return null;
  if (own.some(model => model.hostState === 'pending_apply')) return 'pending_apply';
  if (own.some(model => model.hostState === 'awaiting_reload')) return 'awaiting_reload';
  if (!own.some(model => model.inCatalog)) return 'not_in_catalog';
  if (own.some(model => model.hostState === 'load_unconfirmed')) return 'load_unconfirmed';
  return 'loaded';
}

/** 从上游发现结果带出的预填：上游只给 ID 与显示名，其余能力值仍要人确认。 */
export type ModelPreset = { providerId: string; upstreamId: string; displayName: string };

export function defaultPolicy(): ModelPolicy {
  return { contextLimit: null, outputLimit: null, compactLimit: null,
    reasoning: { support: 'unknown', control: 'none', allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
    inputs: inputKinds.map(kind => ({ kind, upstream: kind === 'text' ? 'supported' : 'unknown', gateway: 'unknown', host: 'unknown',
      effectivePath: 'blocked', mimeTypes: [], maxBytes: null, conversionId: null, verification: 'declared', blockedReasonKey: null })),
    tools: { functionTools: 'unknown', parallelTools: 'unknown', customTools: 'unknown', verification: 'declared' } };
}

/**
 * 界面上可勾选的输入类型。
 *
 * 六类里只放这五种：「其他文件」（document）既不能被投影到 Codex 目录，链路上也没有
 * 它的通路，摆出来只会让人以为勾了有用。整页编辑器和添加模型弹窗用同一份，
 * 同一件事在两处不能长得不一样。
 */
export const EDITABLE_INPUT_KINDS: readonly InputKind[] = ['text', 'image', 'audio', 'video', 'pdf'];

/**
 * 从上游批量添加模型时用的默认长度。
 *
 * 上游的 `/models` 只返回 ID 和名称，不返回窗口大小，所以只能给一组**保守**的默认值：
 * 128K / 8K 对当前主流模型都成立，且输出小于上下文，能直接通过核心校验。
 * 「先用默认的」不等于「猜对了」——这两个值会写进模型策略，之后必须在编辑弹窗里按
 * 供应商文档核对，界面也要把这件事说出来。
 */
export const DISCOVERY_DEFAULT_LIMITS = { contextLimit: 128_000, outputLimit: 8_192 } as const;

/** 批量添加时的完整默认策略：长度取上面的默认值，能力保持未知等人确认。 */
export function discoveredModelPolicy(previous: ModelPolicy = defaultPolicy()): ModelPolicy {
  return { ...previous, contextLimit: DISCOVERY_DEFAULT_LIMITS.contextLimit, outputLimit: DISCOVERY_DEFAULT_LIMITS.outputLimit };
}

/** Token 数的紧凑写法（模型行上的徽章）：1048576 → `1M`，131072 → `131K`。 */
/**
 * 已保存的思考声明不是「档位式」时给一句说明，否则界面上只有几个空 chip，
 * 看起来像「这个模型没有思考」——它其实声明了，只是控制方式不在这页能编辑的范围内。
 * 返回文案键，没有需要说明的返回 null。
 */
export function reasoningKeptKey(policy: ModelPolicy): string | null {
  if (policy.reasoning.support !== 'supported' || policy.reasoning.control === 'effort') return null;
  return policy.reasoning.control === 'budget' ? 'editor.reasoningKeptBudget' : 'editor.reasoningKeptToggle';
}

/**
 * Codex 接受的思考档位，从低到高。
 *
 * 取值不是拍脑袋定的：`none / minimal / low / medium / high / xhigh / max / ultra`
 * 是从本机 ChatGPT.app 里那份 codex 二进制的 `ReasoningEffort` 枚举读出来的
 * （`strings` 里的变体名连写：`noneminimalmediumxhighmaxultrapersistent`）。
 * 档位会被投影进 Codex 的模型目录，写它不认识的值，轻则这一档在宿主里没有标签，
 * 重则整个目录条目被拒——所以这里给集合，而不是让人手输（用户点名的就是这个）。
 *
 * `none` 故意不在表里：它表示「这次不思考」，而核心的适配器注释里记着一个真实故障——
 * 宿主在没有档位可选时会送一个 `none`，转发出去上游直接拒绝（moonshot 返回 400
 * `reasoning.effort value "none" is not supported`）。把它摆成「最低档」会诱导用户
 * 声明出一个大概率被上游拒的值；不声明任何档位才是「这个模型不声明思考」的表达方式。
 */
export const REASONING_LEVEL_PRESETS: readonly { value: string; labelKey: string }[] = [
  { value: 'minimal', labelKey: 'editor.level.minimal' },
  { value: 'low', labelKey: 'editor.level.low' },
  { value: 'medium', labelKey: 'editor.level.medium' },
  { value: 'high', labelKey: 'editor.level.high' },
  { value: 'xhigh', labelKey: 'editor.level.xhigh' },
  { value: 'max', labelKey: 'editor.level.max' },
  { value: 'ultra', labelKey: 'editor.level.ultra' },
];

/** 带当前语言的标签：模块只加载一次，不能把文案冻在启动时的语言上。 */
export function reasoningLevelPresets(): { value: string; label: string }[] {
  return REASONING_LEVEL_PRESETS.map(preset => ({ value: preset.value, label: t(preset.labelKey) }));
}

/** Token 数的紧凑写法（模型行上的徽章）：1048576 → `1M`，131072 → `131K`。 */
export function compactTokens(value: number): string {
  if (value >= 1_000_000) return `${Number((value / 1_000_000).toFixed(1))}M`;
  if (value >= 1_000) return `${Number((value / 1_000).toFixed(0))}K`;
  return String(value);
}

/**
 * 模型能力表单的状态（整页编辑器与添加模型弹窗共用）。
 *
 * 能力项用 `Record<InputKind, Support>` 而不是「勾了没有」：核心的能力是三态
 * （支持 / 不支持 / 未知），而界面只给勾选。未知不能被渲染成不支持——把未知写成
 * 不支持会通过交集原则把这个能力从 Codex 目录里灭掉。所以每一格都带着当前值：
 * 勾上是支持、取消是不支持、**没点过**的项在保存时原样写回。
 */
export interface CapabilityState {
  contextLimit: number | null;
  outputLimit: number | null;
  inputs: Record<InputKind, Support>;
  functionTools: Support;
  parallelTools: Support;
  /** 思考档位，从低到高。空表示这个模型不声明档位。 */
  levels: string[];
  /** 默认档位，必须是 `levels` 里的一项。 */
  defaultLevel: string | null;
}

/** 初始状态：文本永远支持（链路底线），其余沿用已保存的值。 */
export function capabilityState(policy: ModelPolicy = defaultPolicy()): CapabilityState {
  const inputs = {} as Record<InputKind, Support>;
  for (const kind of EDITABLE_INPUT_KINDS) {
    inputs[kind] = kind === 'text' ? 'supported' : policy.inputs.find(entry => entry.kind === kind)?.upstream ?? 'unknown';
  }
  return {
    contextLimit: policy.contextLimit,
    outputLimit: policy.outputLimit,
    inputs,
    functionTools: policy.tools.functionTools,
    parallelTools: policy.tools.parallelTools,
    levels: [...policy.reasoning.allowedValues],
    defaultLevel: policy.reasoning.defaultValue,
  };
}

/**
 * 把表单状态写回模型策略。长度、档位与工具声明都在这里做一致性检查。
 *
 * `reasoningTouched` 是「用户有没有动过思考那一节」。没动过就**原样保留**已保存的声明：
 * 界面上的档位 chip 只表达「档位式（effort）」一种控制方式，而核心还支持
 * 「开启/关闭」与「Token 预算」两种。不保留的话，打开一次表单再保存就会把它们抹掉。
 */
export function policyFromCapability(input: CapabilityState, previous: ModelPolicy = defaultPolicy(), reasoningTouched = false): ModelPolicy {
  const levels = input.levels.map(level => level.trim()).filter(Boolean);
  const declared = levels.length > 0;
  if (input.contextLimit !== null && input.outputLimit !== null && input.outputLimit >= input.contextLimit) {
    throw new Error(t('editor.outputExceedsContext'));
  }
  return { ...previous,
    contextLimit: input.contextLimit,
    outputLimit: input.outputLimit,
    // 没有档位就是「不声明思考」：未知不会被投影到 Codex，也不会被发送到上游。
    reasoning: !reasoningTouched ? previous.reasoning
      : declared
        ? { support: 'supported', control: 'effort', allowedValues: levels,
            defaultValue: input.defaultLevel && levels.includes(input.defaultLevel) ? input.defaultLevel : levels[0]!,
            budgetTokens: null, mappingId: 'reasoning.effort.v1' }
        : { support: 'unknown', control: 'none', allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
    // 从完整能力表出发：已保存的数据缺某一类时用默认记录补上，
    // 界面上勾过的项才不会因为「原来没有这一条」而被丢掉。
    inputs: defaultPolicy().inputs.map(base => {
      const saved = previous.inputs.find(entry => entry.kind === base.kind) ?? base;
      return EDITABLE_INPUT_KINDS.includes(base.kind)
        ? { ...saved, upstream: input.inputs[base.kind], verification: 'declared' as const }
        : saved;
    }),
    tools: { ...previous.tools, functionTools: input.functionTools, parallelTools: input.parallelTools, verification: 'declared' },
  };
}

export function parseTokens(value: string): number | null {
  const input = value.trim().replace(/[,_\s]/g, '');
  if (!input) return null;
  const match = /^(\d+)(ki|mi|k|m)?$/i.exec(input);
  if (!match) throw new Error(t('editor.tokenPositive'));
  const multiplier = ({ k: 1000, m: 1000000, ki: 1024, mi: 1048576 } as Record<string, number>)[match[2]?.toLowerCase() ?? ''] ?? 1;
  const result = Number(match[1]) * multiplier;
  if (!Number.isSafeInteger(result) || result <= 0 || result > 2147483647) throw new Error(t('editor.tokenRange'));
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
    inputs: previous.inputs.map(input => ({ ...input,
      // 链路不支持的输入不渲染 select，表单里没有这个字段；保持原值而不是读出 null。
      upstream: inputBlocked(input.kind) || !data.has(`input-${input.kind}`)
        ? input.upstream
        : data.get(`input-${input.kind}`) as Support })),
    tools: { functionTools: data.get('functionTools') as Support, parallelTools: data.get('parallelTools') as Support,
      customTools: 'unknown', verification: 'declared' },
  };
  if (policy.contextLimit !== null && policy.outputLimit !== null && policy.outputLimit >= policy.contextLimit) throw new Error(t('editor.outputExceedsContext'));
  if (policy.reasoning.defaultValue && !allowedValues.includes(policy.reasoning.defaultValue)) throw new Error(t('editor.defaultNotAllowed'));
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
    protocolOverride: model.protocolOverride ?? null,
    ...overrides,
  };
}
