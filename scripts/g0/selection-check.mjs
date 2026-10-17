#!/usr/bin/env node
/**
 * 「选了第三方模型又被切回原生」这条链路的真机协议验收。
 *
 * 对**真的 bridge 二进制**说话，由它起**两根真的 codex**，把宿主在选模型时会发的
 * 那几条报文照原样打一遍（形状取自真机 bridge.log）：
 *
 *   1. `model/list` 把两份菜单合起来（网关模型在里面）；
 *   2. `config/batchWrite` 写入网关模型 —— 那一笔必须被摘掉，不许进原生配置；
 *   3. 紧接着的 `config/read` —— **读回来必须还是刚选的那个网关模型**，
 *      否则宿主的选择器会自己勾回原生模型（用户报的症状本身）；
 *   4. `thread/start` 带着这个模型 + 宿主会带上的 `modelProvider` —— 必须落在托管那根。
 *
 * 第 4 步特意跑两个 provider 取值：宿主从 `config/read` 里读到的那个（原生那根是
 * `custom`）与我们托管那根的 `gptswitch`。这一个差值决定了「选择不再回滚」之后
 * 能不能真的开出一条能用的线程。
 *
 * 用法：
 *   cargo build -p gptswitch-bridge
 *   cargo run -q -p switch-core --example g0_apply_pipeline -- /tmp/g0-selection >/dev/null
 *   node scripts/g0/selection-check.mjs /tmp/g0-selection
 */
import { existsSync, readFileSync, rmSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { AppServer } from './rpc.mjs';
import { resolveCodexBinary } from './codex-binary.mjs';

const workdir = process.argv[2] ?? '/tmp/g0-selection';
const REPO = new URL('../..', import.meta.url).pathname;
const BRIDGE = join(REPO, 'target', 'debug', 'gptswitch-bridge');
const REAL_CODEX = resolveCodexBinary();
const LOG = join(workdir, 'selection.log');
const STATE = join(workdir, 'bridge-model-state');

rmSync(LOG, { force: true });
rmSync(STATE, { force: true });

const failures = [];
const note = (ok, label, detail) => {
  console.log(`${ok ? '✓' : '✗'} ${label}${detail ? ` — ${detail}` : ''}`);
  if (!ok) failures.push(label);
};
const entries = () => {
  try {
    return readFileSync(LOG, 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line));
  } catch { return []; }
};
const lastRouting = method => entries().filter(e => e.event === 'routing' && e.method === method).at(-1);
const keyOf = m => String(m?.id ?? m?.model ?? m?.slug ?? '');

const bridge = new AppServer(BRIDGE, {
  ...process.env,
  GPTSWITCH_BRIDGE_CODEX: REAL_CODEX,
  GPTSWITCH_BRIDGE_MANAGED_HOME: join(workdir, 'codex-home'),
  GPTSWITCH_BRIDGE_NATIVE_HOME: join(homedir(), '.codex'),
  GPTSWITCH_BRIDGE_LOG: LOG,
});

try {
  await bridge.initialize();

  const list = await bridge.call('model/list', { includeHidden: true, cursor: null, limit: 100 }, 60_000);
  const models = list?.data ?? list?.models ?? [];
  const ours = models.filter(m => keyOf(m).startsWith('gs/'));
  const native = models.filter(m => !keyOf(m).startsWith('gs/'));
  note(ours.length > 0 && native.length > 0,
    '菜单里两边的模型都在', `原生 ${native.length} 条 / 我们 ${ours.length} 条`);
  const slug = keyOf(ours[0]);

  // 选之前：原生那根报的就是用户真实的当前模型。
  const before = await bridge.call('config/read', { includeLayers: false }, 60_000);
  const nativeModel = before?.config?.model;
  const nativeProvider = before?.config?.model_provider;
  console.log(`  （原生 config/read：model=${nativeModel} provider=${nativeProvider}）`);

  // 宿主点选网关模型的报文形状：与真机日志逐字一致。
  const written = await bridge.call('config/batchWrite', {
    edits: [{ keyPath: 'model', value: slug, mergeStrategy: 'replace' }],
    expectedVersion: null,
    filePath: join(homedir(), '.codex', 'config.toml'),
    reloadUserConfig: true,
  }, 60_000);
  const routing = lastRouting('config/batchWrite');
  note(routing?.child === 'native', '写入仍然落在原生那根（我们自己不写原生配置）', routing?.child);
  note(entries().some(e => e.event === 'config-write-sanitized' && e.dropped > 0),
    '网关模型那一笔被摘掉了');

  // 宿主写完立刻重读 —— 这一条就是用户报的症状本身。
  const after = await bridge.call('config/read', { includeLayers: false }, 60_000);
  note(after?.config?.model === slug,
    '**选完之后 config/read 读回来还是用户选的那个网关模型**',
    `读到 ${after?.config?.model}`);
  note(after?.config?.model_provider === 'gptswitch',
    'provider 也跟着换成了托管那根的名字',
    `读到 provider=${after?.config?.model_provider}（原生那根是 ${nativeProvider}）`);
  note(existsSync(STATE), '选择落进了跨进程的状态文件 bridge-model-state');

  // 开线程：宿主会把刚从 config/read 读到的那对 model/provider 一起带上。
  const fromConfig = after?.config?.model_provider;
  const worked = await bridge.call('thread/start', {
    model: slug, modelProvider: fromConfig, cwd: workdir, approvalPolicy: 'never',
  }, 90_000).then(result => ({ result })).catch(error => ({ error: error.message }));
  const routed = lastRouting('thread/start');
  note(routed?.child === 'managed' && !worked.error,
    '带 config/read 那对取值开线程，落在托管那根并成功',
    worked.error
      ? `→ ${routed?.child}（${routed?.why}）${String(worked.error).slice(0, 300)}`
      : `→ ${routed?.child}（${routed?.why}）thread=${worked.result?.thread?.id ?? '?'}`);

  // 反例留证：provider 不跟着换会怎样。这不是断言，是让「为什么必须一起盖」有可复现的证据。
  const stale = await bridge.call('thread/start', {
    model: slug, modelProvider: nativeProvider, cwd: workdir, approvalPolicy: 'never',
  }, 90_000).then(result => ({ result })).catch(error => ({ error: error.message }));
  console.log(`  （provider 仍是原生的 ${nativeProvider} 时：${stale.error ? String(stale.error).slice(0, 160) : '居然成功了'}）`);

  const chosenFacts = entries().filter(e => e.event === 'config-write-sanitized');
  console.log(`  （摘写事件 ${chosenFacts.length} 条，remembered=${chosenFacts.at(-1)?.remembered}）`);
} finally {
  bridge.stop();
}

console.log(failures.length ? `\n${failures.length} 项未通过：${failures.join(' / ')}` : '\n全部通过');
process.exit(failures.length ? 1 : 0);
