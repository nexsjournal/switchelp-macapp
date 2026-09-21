import { useSyncExternalStore } from 'react';

import type { ConnectionState } from './policy';

/**
 * 「最近一次测试」的结果表：**活在本次会话里**。
 *
 * 为什么不落库：探测结果是**当时**的事实（Key 可能刚换、上游可能刚挂），持久化只会让
 * 用户对着一个过期的绿点下判断。没测过就是中性。
 *
 * 为什么也不能放在页面组件的 state 里（这就是这个模块存在的理由）：供应商页内嵌的模型表
 * 与「网关」页读的是同一批模型，而组件 state 一离开这一页就整表清空——用户测试完切一下
 * 页面回来，刚变成「已通过」的那一行又退回「未测试」（用户反馈原话：「我一切页面，
 * 这里就又重置变成未测试了」）。
 *
 * 放在模块作用域，生命周期正好是「这次会话」：切页面还在，退出应用就没了。
 */
let table: Record<string, ConnectionState> = {};
const listeners = new Set<() => void>();

export function rememberConnection(modelId: string, state: ConnectionState): void {
  table = { ...table, [modelId]: state };
  listeners.forEach(listener => listener());
}

/** 测试之间要把会话表清空，否则上一个用例的「已通过」会漏到下一个用例。 */
export function resetConnections(): void {
  table = {};
  listeners.forEach(listener => listener());
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function useConnections(): Record<string, ConnectionState> {
  return useSyncExternalStore(subscribe, () => table);
}
