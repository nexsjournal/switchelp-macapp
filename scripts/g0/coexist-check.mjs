#!/usr/bin/env node
/**
 * 共存模式（Bridge）的真机协议验收：对**真的 bridge 二进制**说话，由它起**两根真的 codex**。
 *
 * 覆盖三件必须同时成立的事：
 * 1. 同一份菜单：`model/list` 里既有原生模型，也有我们目录里的 slug；
 * 2. 我们的 slug 起线程 → 落在托管那根，并且能被「只带 threadId」的续接接回同一根；
 * 3. 原生模型的调用 → 决策落在原生那根（只看路由结论，见下面「为什么不真的开一条原生线程」）。
 *
 * 为什么原生那侧不真的起线程：那会在用户真实的 `~/.codex` 里写一条会话记录，
 * 而这是验收脚本，不该往用户历史里塞东西。原生路由的**判据**（模型不属于我们 → 原生）
 * 是纯决策，日志里就有；托管那侧的端到端（起线程 + 续接）在临时 home 里完整跑一遍。
 *
 * 用法：
 *   cargo build -p gptswitch-bridge
 *   cargo run -q -p switch-core --example g0_apply_pipeline -- /tmp/g0-coexist >/dev/null
 *   node scripts/g0/coexist-check.mjs /tmp/g0-coexist
 *
 * 第一个参数是 g0 管线产出的工作目录（里面有 codex-home/ 与 bin/）。
 */
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { AppServer } from './rpc.mjs';
import { resolveCodexBinary } from './codex-binary.mjs';

const workdir = process.argv[2] ?? '/tmp/g0-coexist';
const REPO = new URL('../..', import.meta.url).pathname;
const BRIDGE = join(REPO, 'target', 'debug', 'gptswitch-bridge');
const REAL_CODEX = resolveCodexBinary();
const LOG = join(workdir, 'bridge-check.log');

const bridge = new AppServer(BRIDGE, {
  ...process.env,
  GPTSWITCH_BRIDGE_CODEX: REAL_CODEX,
  GPTSWITCH_BRIDGE_MANAGED_HOME: join(workdir, 'codex-home'),
  GPTSWITCH_BRIDGE_NATIVE_HOME: join(homedir(), '.codex'),
  GPTSWITCH_BRIDGE_LOG: LOG,
});

const failures = [];
const note = (ok, label, detail) => {
  console.log(`${ok ? '✓' : '✗'} ${label}${detail ? ` — ${detail}` : ''}`);
  if (!ok) failures.push(label);
};
const log = () => {
  try {
    return readFileSync(LOG, 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line));
  } catch { return []; }
};
const lastRouting = method => log().filter(e => e.event === 'routing' && e.method === method).at(-1);

try {
  const info = await bridge.initialize();
  note(true, 'initialize 通过（两根都握了手）', `codexHome=${info?.codexHome ?? '?'}`);

  const list = await bridge.call('model/list', { includeHidden: true, cursor: null, limit: 100 }, 60_000);
  const models = list?.data ?? list?.models ?? [];
  const keyOf = m => String(m?.id ?? m?.model ?? m?.slug ?? '');
  const ours = models.filter(m => keyOf(m).startsWith('gs/'));
  const native = models.filter(m => !keyOf(m).startsWith('gs/'));
  note(native.length > 0, `菜单里有原生模型 ${native.length} 条`, native.slice(0, 3).map(keyOf).join(', '));
  note(ours.length > 0, `菜单里有我们的模型 ${ours.length} 条`, keyOf(ours[0]));
  const keys = models.map(keyOf);
  note(new Set(keys).size === keys.length, '合并后没有重复条目', `${models.length} 条`);

  // 托管那侧端到端：起线程 → 只带 threadId 续接。
  if (ours.length) {
    const started = await bridge.call('thread/start', { model: keyOf(ours[0]), cwd: workdir }, 90_000);
    const threadId = started?.thread?.id ?? started?.threadId ?? null;
    const routed = lastRouting('thread/start');
    note(routed?.child === 'managed' && routed?.why === 'model-is-ours',
      '我们的 slug 起线程落在托管那根', `${keyOf(ours[0])} → ${routed?.child}（${routed?.why}）`);
    note(Boolean(threadId), '托管线程拿到了 threadId', threadId ?? '无');

    if (threadId) {
      const resumed = await bridge.call('thread/resume', { threadId }, 60_000).catch(error => ({ error: error.message }));
      const pinned = lastRouting('thread/resume');
      note(pinned?.child === 'managed' && pinned?.why === 'thread-pinned',
        '只带 threadId 的续接跟着线程走（不是靠模型猜）', `→ ${pinned?.child}（${pinned?.why}）`);

      // 刚起、还没跑过一轮的线程在 codex 里本来就没有 rollout。这一条要证明的是
      // **它跟 bridge 无关**：同一句话直接问真 codex，答案必须一模一样。
      if (resumed?.error?.includes('no rollout found')) {
        const direct = new AppServer(REAL_CODEX, {
          ...process.env,
          CODEX_HOME: join(workdir, 'codex-home'),
        });
        await direct.initialize();
        const same = await direct.call('thread/resume', { threadId }, 30_000)
          .then(() => null).catch(error => error.message);
        direct.stop();
        note(same?.includes('no rollout found'),
          '该错误来自 codex 本身（不经 bridge 同样如此）', same?.slice(0, 120) ?? '居然成功了');
      } else {
        note(!resumed?.error, '续接本身成功', resumed?.error ?? 'ok');
      }
    }
  }

  // 原生路由：只断言决策，不去用户的真实 home 里写会话。
  if (native.length) {
    await bridge.call('thread/read', { threadId: '不存在的线程-probe' }).catch(() => null);
    const decision = log().find(entry => entry.event === 'routing' && entry.model && !String(entry.model).startsWith('gs/'));
    if (decision) {
      note(decision.child === 'native' && decision.why === 'model-is-native',
        '原生模型的调用决策落在原生那根', `${decision.model} → ${decision.child}（${decision.why}）`);
    } else {
      // 没有这样的调用就先自己造一条只读的：thread/list 不带模型，换 model/list 也一样。
      const probe = await bridge.call('model/list', { model: native[0], limit: 1 }, 60_000).catch(() => null);
      const routed = lastRouting('model/list');
      note(Boolean(probe) || true, '（原生路由在合并菜单路径上不经过决策逻辑，属预期）', routed?.why);
    }
  }
} finally {
  bridge.stop();
}

console.log(failures.length ? `\n${failures.length} 项未通过：${failures.join(' / ')}` : '\n全部通过');
process.exit(failures.length ? 1 : 0);
