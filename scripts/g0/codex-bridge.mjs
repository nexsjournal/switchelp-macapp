#!/usr/bin/env node
/**
 * 原型：让原生模型和第三方模型出现在**同一份菜单**里，并按选择把请求交给不同的 codex。
 *
 * 为什么需要两根子进程：`model_provider` 是**进程级**配置——一个 codex 进程只有一套
 * 上游设置。所以「选原生走账号登录、选第三方走供应商」不可能在同一个进程里做到：
 * 原生那份用用户真实的 `~/.codex`（登录态、官方 provider 原样不动），
 * 第三方那份用 Switchelp 托管的 CODEX_HOME（网关 provider + 我们发布的目录）。
 * bridge 负责合并 `model/list`，并按线程把每个会话钉到对应那一根上。
 *
 * 本文件是**原型**，不是产品代码：产品形态（托管 home 放在哪）还没定。用法：
 *
 *   SWITCHELP_PROTO_HOME=/tmp/proto-home/codex-home \
 *     open -a ChatGPT --env CODEX_CLI_PATH="$PWD/scripts/g0/codex-bridge.mjs"
 *
 * 路由证据写在 /tmp/switchelp-bridge.log。
 */
import { spawn } from 'node:child_process';
import { appendFileSync, writeFileSync } from 'node:fs';
import { createInterface } from 'node:readline';
import { homedir } from 'node:os';
import { resolveCodexBinary } from './codex-binary.mjs';

const LOG = '/tmp/switchelp-bridge.log';
const REAL_CODEX = resolveCodexBinary();
/**
 * 两份 home 用环境变量指定，便于在真机上做对照实验：
 * - 原生那份应当是**没被我们改过**的配置（真实场景里由产品决定放哪，原型里用备份拼一份）；
 * - 托管那份是含网关 provider 与我们发布目录的那份。
 * 默认值只是原地退化：没有指定时两边都指向用户真实的 home，等于纯透传。
 */
const NATIVE_HOME = process.env.SWITCHELP_PROTO_NATIVE_HOME ?? `${homedir()}/.codex`;
const MANAGED_HOME = process.env.SWITCHELP_PROTO_MANAGED_HOME ?? process.env.SWITCHELP_PROTO_HOME;
const argv = process.argv.slice(2);

writeFileSync(LOG, '');
function log(entry) {
  appendFileSync(LOG, `${new Date().toISOString()} ${JSON.stringify(entry)}\n`);
}

log({ event: 'bridge-started', argv, nativeHome: NATIVE_HOME, managedHome: MANAGED_HOME ?? null });
if (!MANAGED_HOME) {
  // 没有托管 home 就退化成纯透传：宁可什么都不做，也不要做出一个「菜单里能选、
  // 一发请求就失败」的半成品。
  log({ event: 'no-managed-home-passthrough' });
}

class Codex {
  constructor(name, codexHome) {
    this.name = name;
    this.child = spawn(REAL_CODEX, argv, {
      cwd: '/',
      env: { ...process.env, CODEX_HOME: codexHome },
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    this.stderr = '';
    this.child.stderr.setEncoding('utf8');
    this.child.stderr.on('data', text => { this.stderr = (this.stderr + text).slice(-4000); });
    createInterface({ input: this.child.stdout }).on('line', line => {
      const trimmed = line.trim();
      if (!trimmed) return;
      let message;
      try { message = JSON.parse(trimmed); } catch { return; }
      if (message.id === undefined) {
        // 通知：原样转发。宿主按 threadId 过滤自己关心的事件。
        process.stdout.write(trimmed + '\n');
        return;
      }
      this.pending.get(message.id)?.(message);  // 解包在 request 里做，这里只投递
    });
    this.child.on('exit', (code, signal) => {
      log({ event: 'child-exit', child: name, code, signal, stderr: this.stderr.slice(-500) });
    });
  }

  pending = new Map();

  /** 发一条请求并等它自己那条响应；id 沿用宿主给的，便于原样回写。 */
  request(message) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(message.id);
        reject(new Error(`${this.name} 超时：${message.method}`));
      }, message.method === 'initialize' ? 60_000 : 120_000);
      this.pending.set(message.id, reply => {
        clearTimeout(timer);
        this.pending.delete(message.id);
        // JSON-RPC 的结果在 `result` 里；把整条消息当结果用，会让下游算出「0 条模型」
        // 这种看似正常、实则全错的数据。
        if (reply.error) reject(new Error(`${this.name}: ${JSON.stringify(reply.error).slice(0, 200)}`));
        else resolve(reply.result);
      });
      this.child.stdin.write(JSON.stringify(message) + '\n');
    });
  }

  notify(message) {
    this.child.stdin.write(JSON.stringify(message) + '\n');
  }
}

const native = new Codex('native', NATIVE_HOME);
const managed = MANAGED_HOME ? new Codex('managed', MANAGED_HOME) : null;

/** threadId → 子进程名。宿主后续带 threadId 的调用都跟着这条记录走。 */
const pinned = new Map();
/** 我们目录里的 slug，用来判断「这个选择属于哪一边」。 */
let managedSlugs = new Set();

/** 从模型列表结果里取数据数组；不同版本字段名不同，都认。 */
function listOf(result) {
  return Array.isArray(result?.data) ? result.data : Array.isArray(result?.models) ? result.models : [];
}

/** 线程 id 可能在几个不同位置；都由 bridge 归一后记住。 */
function threadIdOf(result) {
  return result?.thread?.id ?? result?.threadId ?? result?.id ?? null;
}

function modelIn(message) {
  return message.params?.model ?? message.params?.modelId ?? message.params?.modelSlug ?? null;
}

/** 选哪根子进程：先看线程归属，再看模型归属。 */
function pickChild(message) {
  const threadId = message.params?.threadId;
  if (threadId && pinned.has(threadId)) return { child: pinned.get(threadId), why: 'thread-pinned' };
  const model = modelIn(message);
  if (model && managedSlugs.has(model)) return { child: 'managed', why: 'model-is-ours' };
  if (model) return { child: 'native', why: 'model-is-native' };
  return { child: 'native', why: 'default' };
}

/** 合并菜单：原生在前、我们的在后；同一 id 只留一份。 */
function mergeModelLists(nativeResult, managedResult) {
  const merged = [];
  const seen = new Set();
  for (const item of [...listOf(nativeResult), ...listOf(managedResult)]) {
    const key = item?.id ?? item?.model;
    if (key == null || seen.has(key)) continue;
    seen.add(key);
    merged.push(item);
  }
  const shape = Array.isArray(nativeResult?.data) ? { data: merged } : { models: merged };
  return { ...nativeResult, ...shape, nextCursor: null, cursor: null };
}

/** 合法的 JSON-RPC 响应，别漏 jsonrpc 字段：宿主会直接丢。 */
function sendResult(id, result) {
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\n');
}

function sendError(id, code, message) {
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, error: { code, message } }) + '\n');
}

createInterface({ input: process.stdin }).on('line', async line => {
  const trimmed = line.trim();
  if (!trimmed) return;
  let message;
  try { message = JSON.parse(trimmed); } catch {
    log({ event: 'host-bytes-unparsed', length: trimmed.length });
    return;
  }
  if (message.id === undefined) {
    // 宿主发来的通知（如 initialized）：两边都要知道。
    native.notify(message);
    managed?.notify(message);
    return;
  }

  try {
    // 两根子进程都要先握手：app-server 在 initialize 之前会拒绝一切调用
    // （实测托管那根回 `-32600 Not initialized`）。回给宿主的是**原生那根**的结果，
    // 因为它代表用户真实的 home 与登录环境。
    if (message.method === 'initialize') {
      const nativeResult = await native.request(message);
      if (managed) {
        await managed
          .request({ ...message, id: `proto-init-${Date.now()}` })
          .catch(error => log({ event: 'managed-init-failed', message: error.message }));
      }
      log({ event: 'initialized', children: managed ? 2 : 1 });
      return void sendResult(message.id, nativeResult);
    }

    if (message.method === 'model/list') {
      const nativeResult = await native.request(message);
      if (!managed) return void sendResult(message.id, nativeResult);
      // 托管那根出错不该拖垮菜单：记下来，只给原生那些，并留下证据。
      const managedResult = await managed
        .request({ ...message, id: `proto-model-list-${Date.now()}` })
        .catch(error => { log({ event: 'managed-list-failed', message: error.message }); return { data: [] }; });
      managedSlugs = new Set(listOf(managedResult).flatMap(item => [item?.id, item?.model].filter(Boolean)));
      const merged = mergeModelLists(nativeResult, managedResult);
      log({ event: 'model-list-merged', native: listOf(nativeResult).length, managed: listOf(managedResult).length, merged: listOf(merged).length });
      return void sendResult(message.id, merged);
    }

    const { child, why } = pickChild(message);
    const target = child === 'managed' && managed ? managed : native;
    if (child === 'managed' && !managed) {
      // 没有托管那根时不能假装成功：如实报错，别让一个注定失败的请求看起来像在跑。
      sendError(message.id, -32603, 'Switchelp 托管配置未就绪');
      log({ event: 'rejected-no-managed', method: message.method });
      return;
    }
    // 决策先记：即使这次调用随后失败，路由对不对也是我们要断言的事实。
    log({ event: 'routing', method: message.method, child: target.name, why, model: modelIn(message) });
    const response = await target.request(message);
    const threadId = threadIdOf(response);
    if (threadId && ['thread/start', 'thread/resume', 'thread/fork'].includes(message.method)) {
      pinned.set(threadId, target.name);
    }
    log({ event: 'routed', method: message.method, child: target.name, why, threadId, model: modelIn(message) });
    sendResult(message.id, response);
  } catch (error) {
    log({ event: 'route-failed', method: message.method, message: error.message });
    sendError(message.id, -32603, `bridge: ${error.message}`);
  }
});

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => { native.child.kill(); managed?.child.kill(); process.exit(0); });
}
