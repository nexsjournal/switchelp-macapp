/**
 * 幂等键：同一次「应用 / 还原」的重复提交必须被核心识别成同一件事。
 *
 * 放在这里而不是各自文件里——Codex 配置页与待应用条都会发起提交，两处必须用同一个规则。
 */
export function newIdempotencyKey(): string {
  return globalThis.crypto?.randomUUID?.() ?? `idem-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
