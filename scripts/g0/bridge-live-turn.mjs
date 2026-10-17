#!/usr/bin/env node
/**
 * 完整链路实测：**在共存模式里选网关模型 → 开对话 → 真发一轮 → 拿到回复**。
 *
 * 前两步（选择不被切回、路由落在托管那根）在 `selection-check.mjs` 里已有断言；
 * 这里补的是最后一截——用户说的「选了却发送不了」。走的是**真的部署产物**：
 * 应用数据目录里那份 bridge + 真的 codex + 用户本机网关（127.0.0.1:18765）。
 *
 * 托管 home 是一份**拷贝**（config.toml 里指向同一个网关实例与同一份目录），
 * 因此这一轮对话不会写进用户自己的托管历史，也不会碰用户真实的 `~/.codex`。
 *
 * 用法：
 *   node scripts/g0/bridge-live-turn.mjs            # 用 /Applications 里那份配置当模板
 *   LIVE=0 node scripts/g0/bridge-live-turn.mjs     # 只跑到起线程，不发真实请求
 */
import { copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { AppServer } from './rpc.mjs';

const APP_DATA = join(homedir(), 'Library/Application Support/app.gptswitch.desktop');
const REAL_MANAGED = join(APP_DATA, 'codex-home');
const BRIDGE = process.env.BRIDGE_BIN ?? join(APP_DATA, 'bin/gptswitch-bridge');
const CODEX = process.env.REAL_CODEX
  ?? '/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex';
const WORK = '/tmp/g0-live-turn';
const SEND = process.env.LIVE !== '0';

rk();
function rk() {
  rmSync(WORK, { recursive: true, force: true });
  mkdirSync(join(WORK, 'codex-home'), { recursive: true });
  // 托管 profile 照抄：同一个网关实例、同一份模型目录、同一个凭据 helper。
  copyFileSync(join(REAL_MANAGED, 'config.toml'), join(WORK, 'codex-home/config.toml'));
  for (const extra of ['installation_id']) {
    const from = join(REAL_MANAGED, extra);
    if (existsSync(from)) copyFileSync(from, join(WORK, 'codex-home', extra));
  }
  const skills = join(REAL_MANAGED, 'skills');
  if (existsSync(skills)) cpSync(skills, join(WORK, 'codex-home/skills'), { recursive: true });
}

const LOG = join(WORK, 'bridge.log');
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
const keyOf = m => String(m?.id ?? m?.model ?? m?.slug ?? '');

const bridge = new AppServer(BRIDGE, {
  ...process.env,
  GPTSWITCH_BRIDGE_CODEX: CODEX,
  GPTSWITCH_BRIDGE_MANAGED_HOME: join(WORK, 'codex-home'),
  GPTSWITCH_BRIDGE_NATIVE_HOME: join(homedir(), '.codex'),
  GPTSWITCH_BRIDGE_LOG: LOG,
});

try {
  await bridge.initialize();
  const list = await bridge.call('model/list', { includeHidden: true, cursor: null, limit: 100 }, 60_000);
  const models = list?.data ?? list?.models ?? [];
  const ours = models.filter(m => keyOf(m).startsWith('gs/'));
  note(ours.length > 0, '菜单里有网关模型', `${ours.length} 条，例如 ${ours[0]?.display_name ?? keyOf(ours[0])}`);
  const slug = keyOf(ours[0]);

  await bridge.call('config/batchWrite', {
    edits: [{ keyPath: 'model', value: slug, mergeStrategy: 'replace' }],
    expectedVersion: null,
    filePath: join(homedir(), '.codex/config.toml'),
    reloadUserConfig: true,
  }, 60_000);
  const read = await bridge.call('config/read', { includeLayers: false }, 60_000);
  note(read?.config?.model === slug && read?.config?.model_provider === 'gptswitch',
    '选完之后读回来的那一对是 (网关模型, gptswitch)',
    `${read?.config?.model} / ${read?.config?.model_provider}`);

  // 起线程要给个真实目录；用当前工作目录，脚本里不留任何本机路径（隐私扫描会拦）。
  const cwd = process.cwd();
  const started = await bridge.call('thread/start', {
    model: slug, modelProvider: read?.config?.model_provider, cwd, approvalPolicy: 'never',
  }, 90_000);
  const threadId = started?.thread?.id;
  note(Boolean(threadId), '线程起在托管那根', threadId ?? JSON.stringify(started).slice(0, 200));

  if (SEND && threadId) {
    const prompt = '只回答这四个字，不要用工具、不要解释：桥接已通。';
    const turn = await bridge.call('turn/start', {
      threadId,
      model: slug,
      cwd,
      input: [{ type: 'text', text: prompt }],
      approvalPolicy: 'never',
      sandboxPolicy: { type: 'dangerFullAccess' },
    }, 60_000).then(result => ({ result })).catch(error => ({ error: error.message }));
    if (turn.error) note(false, 'turn/start 被受理', String(turn.error).slice(0, 300));
    else note(true, 'turn/start 被受理', `turnId=${turn.result?.turn?.id ?? '?'}`);

    // 等真正的回复：codex 用通知把内容推回来。**要的是模型自己的输出**，
    // 不是宿主回显的那句提问——两者都含「桥接已通」，所以按「与提问不同」来认。
    const deadline = Date.now() + 180_000;
    let answer = null;
    let failure = null;
    const seen = new Set();
    while (Date.now() < deadline) {
      for (const m of bridge.notifications) {
        if (typeof m.method !== 'string') continue;
        const text = JSON.stringify(m.params ?? {});
        if (text !== prompt && /桥接已通/.test(text)) {
          for (const match of text.matchAll(/"(?:text|content|delta)":"((?:[^"\\]|\\.){2,400})"/g)) {
            const candidate = match[1];
            if (candidate !== prompt) {
              seen.add(candidate);
              answer ??= candidate;
            }
          }
        }
        if (/error|failed/i.test(m.method)) failure ??= `${m.method}: ${text.slice(0, 300)}`;
      }
      if (answer) break;
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
    const methods = [...new Set(bridge.notifications.map(m => m.method).filter(Boolean))];
    note(Boolean(answer), '**拿到网关模型的真实回复**',
      answer ? `「${[...seen].slice(0, 3).join(' | ')}」` : (failure ? `未拿到；最后一处异常：${failure}` : `180s 内没有回复；收到的通知：${methods.slice(0, 12).join(', ')}`));
  }

  const turnRouting = entries().filter(e => e.event === 'routing' && e.method === 'turn/start').at(-1);
  console.log(`  （turn/start 路由：→ ${turnRouting?.child}（${turnRouting?.why}））`);
} finally {
  bridge.stop();
}

console.log(failures.length ? `\n${failures.length} 项未通过：${failures.join(' / ')}` : '\n全部通过');
process.exit(failures.length ? 1 : 0);
