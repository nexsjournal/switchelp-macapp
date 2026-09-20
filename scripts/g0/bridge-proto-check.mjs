#!/usr/bin/env node
/**
 * 原型的验收脚本：像宿主那样对 bridge 说话，然后核对三件事。
 *
 * 1. `model/list` 里**同时**有原生模型和我们目录里的 slug（这是「同一份菜单」）；
 * 2. 用原生模型起线程 → 请求落到原生那根子进程（走账号登录）；
 * 3. 用我们的 slug 起线程 → 落到托管那根（走网关 provider）。
 *
 * 这三条正是「菜单里能选」与「请求真能路由」必须同时成立的那个约束。
 */
import { readFileSync } from 'node:fs';
import { AppServer } from './rpc.mjs';

const BRIDGE = new URL('./codex-bridge.mjs', import.meta.url).pathname;
const LOG = '/tmp/switchelp-bridge.log';
const MANAGED_HOME = process.env.SWITCHELP_PROTO_HOME;
if (!MANAGED_HOME) throw new Error('需要 SWITCHELP_PROTO_HOME 指向托管 CODEX_HOME');

const bridge = new AppServer(BRIDGE, { ...process.env, SWITCHELP_PROTO_HOME: MANAGED_HOME });
const failures = [];
const note = (ok, label, detail) => {
  console.log(`${ok ? '✓' : '✗'} ${label}${detail ? ` — ${detail}` : ''}`);
  if (!ok) failures.push(label);
};

try {
  const info = await bridge.initialize();
  note(true, 'initialize 通过', `codexHome=${info?.codexHome}`);

  const list = await bridge.call('model/list', { includeHidden: true, cursor: null, limit: 100 }, 60_000);
  const models = list?.data ?? list?.models ?? [];
  const ids = models.flatMap(m => [m?.id, m?.model].filter(Boolean));
  const ours = ids.filter(id => String(id).startsWith('gs/'));
  note(models.length >= 2, `菜单里有 ${models.length} 条模型`);
  note(ours.length >= 2, `其中属于我们目录的 slug ${ours.length} 条`, ours[0]);

  // 原生模型：拿菜单里第一条不属于我们的。
  const nativeModel = models.find(m => !String(m?.id ?? m?.model ?? '').startsWith('gs/'));
  const nativeId = nativeModel?.model ?? nativeModel?.id;
  if (nativeId) {
    const started = await bridge.call('thread/start', { model: nativeId, cwd: process.cwd() }, 90_000).catch(e => ({ error: e.message }));
    const threadId = started?.thread?.id ?? started?.threadId ?? started?.id ?? null;
    const routed = readLog().filter(e => e.event === 'routing' && e.method === 'thread/start').at(-1);
    note(routed?.child === 'native', `原生模型 thread/start 落到原生子进程`, `${nativeId} → ${routed?.child}（${routed?.why}）`);
    note(Boolean(threadId), `拿到 threadId`, threadId ?? '无');
  } else {
    note(false, '菜单里找不到原生模型（无法验证原生路由）');
  }

  if (ours.length) {
    const started = await bridge.call('thread/start', { model: ours[0], cwd: process.cwd() }, 90_000).catch(e => ({ error: e.message }));
    const routed = readLog().filter(e => e.event === 'routing' && e.method === 'thread/start').at(-1);
    note(routed?.child === 'managed', '我们的 slug thread/start 落到托管子进程', `${ours[0]} → ${routed?.child}（${routed?.why}）`);
    note(!started?.error, '托管线程请求本身没有把 bridge 打挂', started?.error ?? 'ok');

    // 关键回归：后续调用往往**不带模型**、只带 threadId（续接、打断、读历史）。
    // 那时必须跟着线程走，不能靠模型猜——否则一个已开的会话会在两根子进程之间跳，
    // 表现是「聊到一半上下文丢了」。
    const oursThread = started?.thread?.id ?? started?.threadId ?? started?.id ?? null;
    if (oursThread) {
      await bridge.call('thread/resume', { threadId: oursThread }, 60_000).catch(() => null);
      const resumed = readLog().filter(e => e.event === 'routing' && e.method === 'thread/resume').at(-1);
      note(resumed?.child === 'managed' && resumed?.why === 'thread-pinned',
        '只带 threadId 的续接跟着线程走', `→ ${resumed?.child}（${resumed?.why}）`);
    } else {
      note(false, '托管线程没有返回 threadId（无法验证线程钉住）');
    }
  }
} finally {
  bridge.child.kill();
}

function readLog() {
  try {
    return readFileSync(LOG, 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line.slice(line.indexOf('{'))));
  } catch { return []; }
}

console.log(failures.length ? `\n${failures.length} 项未通过：${failures.join(' / ')}` : '\n全部通过');
process.exit(failures.length ? 1 : 0);
