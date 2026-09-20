#!/usr/bin/env node
/**
 * 探针：顶替真实的 codex 可执行文件，记录宿主的调用契约，再把一切原样转发。
 *
 * 为什么需要它：桌面端会用 `CODEX_CLI_PATH` 指向的 CLI 顶替内置那份（app.asar 里
 * `fi({rawValue: e.CODEX_CLI_PATH}) ?? di({...resourcesPath,'codex'})`），本地这条走的是
 * stdio 上的 app-server。但**本地分支的确切 argv 与握手顺序只能从真宿主上观察**——
 * 所以这里只做两件事：记下来，然后透明转发。
 *
 * 用法（一次性探针，不是产品代码）：
 *
 *   osascript -e 'quit app "ChatGPT"'
 *   open -a ChatGPT --env CODEX_CLI_PATH="$PWD/scripts/g0/codex-cli-probe.mjs"
 *
 * 日志写到 /tmp/switchelp-cli-probe.log。要还原：不带 --env 重启一次即可。
 *
 * 两条硬约束：
 * - **字节透明**：stdin/stdout/stderr 一律原样通过，不做行解析后再拼回去。
 * - **不记内容**：只记 JSON-RPC 的 method/id 与被调参数**键名**，不记 payload——
 *   握手与后续消息里有账号凭据，日志落到磁盘就是泄漏。
 */
import { spawn } from 'node:child_process';
import { appendFileSync, writeFileSync } from 'node:fs';
import { Transform } from 'node:stream';

const LOG = '/tmp/switchelp-cli-probe.log';
const REAL_CODEX = '/Applications/ChatGPT.app/Contents/Resources/codex';
const MAX_EVENTS = 400;

const argv = process.argv.slice(2);
let events = 0;

function log(entry) {
  if (events >= MAX_EVENTS) return;
  events += 1;
  appendFileSync(LOG, `${new Date().toISOString()} ${JSON.stringify(entry)}\n`);
}

writeFileSync(LOG, '');
log({
  event: 'spawned',
  argv,
  cwd: process.cwd(),
  // 只记 CODEX_* 的值与其余变量的**键名**：环境里有会话令牌，值不能落盘。
  codexEnv: Object.fromEntries(Object.entries(process.env).filter(([k]) => k.startsWith('CODEX_'))),
  envKeys: Object.keys(process.env).sort(),
  node: process.version,
});

/** 记一条消息的形状：能解析成 JSON 就只记方法名与参数键名，否则只记长度。 */
function describe(direction, chunk) {
  const text = chunk.toString('utf8');
  for (const line of text.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const message = JSON.parse(trimmed);
      log({
        event: 'message',
        direction,
        method: message.method ?? null,
        id: message.id ?? null,
        paramKeys: message.params ? Object.keys(message.params).sort() : null,
        resultKeys: message.result ? Object.keys(message.result).sort() : null,
        errorCode: message.error?.code ?? null,
      });
    } catch {
      log({ event: 'bytes', direction, length: trimmed.length });
    }
  }
}

/** 透明转发：原样 push，另外抄一份给日志。 */
function tee(direction) {
  return new Transform({
    transform(chunk, _encoding, callback) {
      describe(direction, chunk);
      callback(null, chunk);
    },
  });
}

const child = spawn(REAL_CODEX, argv, {
  stdio: ['pipe', 'pipe', 'pipe'],
  env: process.env,
});

process.stdin.pipe(tee('in')).pipe(child.stdin);
child.stdout.pipe(tee('out')).pipe(process.stdout);
child.stderr.pipe(tee('err')).pipe(process.stderr);

child.on('exit', (code, signal) => {
  log({ event: 'child-exit', code, signal });
  process.exit(code ?? 1);
});
child.on('error', (error) => {
  log({ event: 'child-error', message: error.message });
  process.exit(127);
});
