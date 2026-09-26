/**
 * 端到端验收：**真实应用 + 真实网关 + 真实 Codex**，上游是本地 mock。
 *
 * 链路：codex app-server → 应用自己的网关（`127.0.0.1:18765`，应用启动时拉起）
 * → 应用安装的 auth helper（宿主真实调用取得令牌）→ chat 适配器 → mock 上游。
 *
 * 与 `probe-apply-pipeline.mjs` 的区别：那个用探针自建网关，这个用的是**应用本身**，
 * 因此同时覆盖了壳层装配、启动恢复重发布路由、helper 安装与令牌轮换。
 *
 * 用法：node scripts/g0/probe-full-loop.mjs
 * 前置：`pnpm exec tauri build --debug --bundles app` 已经产出应用。
 */
import assert from 'node:assert/strict';
import http from 'node:http';
import { execFileSync, spawn } from 'node:child_process';
import { mkdir, readFile, rm, writeFile, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { AppServer } from './rpc.mjs';
import { resolveCodexBinary } from './codex-binary.mjs';

const binary = resolveCodexBinary();
const appBinary =
  process.env.GPTSWITCH_APP_BINARY ??
  resolve('target/debug/bundle/macos/Switchelp.app/Contents/MacOS/gptswitch');
const workdir = process.env.GPTSWITCH_E2E_DIR ?? join(tmpdir(), 'gptswitch-e2e');
const codexHome = join(workdir, 'codex-home');
const configPath = join(codexHome, 'config.toml');
const gatewayOrigin = 'http://127.0.0.1:18765';

await stat(appBinary).catch(() => {
  throw new Error(`未找到应用产物：${appBinary}；先运行 pnpm exec tauri build --debug --bundles app`);
});

/** 第一步：起 mock 上游（chat 协议），端口确定后再写进供应商配置。 */
const requests = [];
const upstream = http.createServer(async (req, res) => {
  if (req.method !== 'POST' || !req.url.endsWith('/chat/completions')) {
    res.writeHead(404, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: { message: 'not a chat endpoint' } }));
    return;
  }
  if (!req.headers.authorization?.startsWith('Bearer ')) {
    res.writeHead(401, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: { message: 'missing bearer' } }));
    return;
  }
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const body = JSON.parse(Buffer.concat(chunks).toString());
  requests.push({ path: req.url, model: body.model, maxTokens: body.max_tokens, effort: body.reasoning_effort, messages: body.messages?.length, bearer: req.headers.authorization });
  const text = `端到端路由成功：${body.model}`;
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  let sequence = 0;
  const emit = (payload) => res.write(`data: ${JSON.stringify(payload)}\n\n`.replace(/\n\n$/, '\n\n'));
  void sequence;
  emit({ id: 'chatcmpl-e2e', choices: [{ index: 0, delta: { role: 'assistant' } }] });
  emit({ choices: [{ index: 0, delta: { content: text } }] });
  emit({ choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] });
  emit({ choices: [], usage: { prompt_tokens: 9, completion_tokens: 3, total_tokens: 12 } });
  res.write('data: [DONE]\n\n');
  res.end();
});
await new Promise((ok, no) => upstream.listen(0, '127.0.0.1', ok).on('error', no));
const upstreamBase = `http://127.0.0.1:${upstream.address().port}/v1`;

/** 第二步：用真实管线把事务写进应用的数据目录（应用必须先停）。 */
await rm(workdir, { recursive: true, force: true });
await mkdir(workdir, { recursive: true, mode: 0o700 });
const seed = JSON.parse(
  execFileSync('cargo', ['run', '-q', '-p', 'switch-core', '--example', 'g0_seed_app', '--', workdir, configPath, upstreamBase], {
    cwd: resolve('.'), encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'],
  }),
);
assert.equal(seed.stage, 'AwaitingReload', '种子事务必须停在等待重载，交给启动恢复发布路由');
assert.ok(!seed.configText.includes('synthetic-seed-secret'), 'config.toml 不得含上游 Key');

/**
 * 第二步半：把凭据写进系统凭据库。
 *
 * 应用运行时读的是系统凭据库，而种子进程用的是内存库——所以这里按种子报出的
 * `secretRef` 精确写入同一条目，`-A` 是为了不产生“某应用请求访问钥匙串”的弹窗。
 * 条目在 finally 里按同一 account 精确删除，不会碰用户自己的凭据。
 */
const keychainService = 'app.gptswitch.desktop';
const syntheticSecret = 'synthetic-seed-secret-not-a-real-key';
if (process.platform === 'darwin') {
  execFileSync('security', ['add-generic-password', '-s', keychainService, '-a', seed.secretRef, '-w', syntheticSecret, '-A', '-U']);
}
const removeKeychainEntry = () => {
  if (process.platform !== 'darwin') return;
  try {
    execFileSync('security', ['delete-generic-password', '-s', keychainService, '-a', seed.secretRef], { stdio: 'ignore' });
  } catch {
    // 已经不存在就是成功。
  }
};

/** 第三步：启动真实应用。它应当生成令牌、安装 helper，并重发布已完成事务的路由。 */
const app = spawn(appBinary, [], {
  env: { PATH: process.env.PATH, HOME: process.env.HOME, GPTSWITCH_TEST_DATA_DIR: workdir },
  stdio: ['ignore', 'pipe', 'pipe'],
});
let appLog = '';
app.stdout.on('data', (text) => { appLog += text; });
app.stderr.on('data', (text) => { appLog += text; });

const report = { observedAt: new Date().toISOString(), workdir, upstreamBase, appBinary };
try {
  /** 第四步：等应用把路由发布出来，并用 helper 的令牌核对目录前缀可服务。 */
  const revision = seed.configText.match(/\/c\/([^/]+)\/v1/)[1];
  const token = await waitForGateway(revision);
  report.helperTokenLength = token.length;
  report.gatewayModels = await fetchModels(revision, token);

  assert.deepEqual(
    [...report.gatewayModels].sort(),
    [...seed.aliases].sort(),
    '应用网关必须按已发布的目录版本提供全部 alias',
  );

  /** 第五步：真实 Codex 走完整链路。 */
  const rpc = new AppServer(binary, {
    PATH: process.env.PATH, HOME: process.env.HOME, TMPDIR: process.env.TMPDIR ?? tmpdir(),
    LANG: 'en_US.UTF-8', CODEX_HOME: codexHome,
  });
  try {
    report.initialized = await rpc.initialize();

    const models = [];
    let cursor;
    do {
      const page = await rpc.call('model/list', { limit: 1, ...(cursor ? { cursor } : {}) });
      models.push(...page.data);
      cursor = page.nextCursor;
    } while (cursor);
    report.modelList = models.map((model) => model.model);
    assert.deepEqual([...report.modelList].sort(), [...seed.aliases].sort(), '宿主菜单必须列出全部自定义模型');

    // 逐个模型跑真实一轮。
    for (const alias of seed.aliases) {
      const started = await rpc.call('thread/start', {
        model: alias, modelProvider: 'gptswitch', cwd: workdir,
        approvalPolicy: 'never', sandbox: 'read-only', ephemeral: true,
      });
      const turn = await rpc.call('turn/start', {
        threadId: started.thread.id,
        input: [{ type: 'text', text: 'Say the route verification result only.', text_elements: [] }],
      });
      const expires = Date.now() + 30_000;
      while (!rpc.notifications.some((n) => n.method === 'turn/completed' && n.params?.turn?.id === turn.turn.id)) {
        if (Date.now() > expires) throw new Error(`回合未完成：${alias}；${rpc.stderr}`);
        await new Promise((ok) => setTimeout(ok, 100));
      }
      const completed = rpc.notifications.find((n) => n.method === 'turn/completed' && n.params?.turn?.id === turn.turn.id);
      if (completed.params.turn.status !== 'completed') throw new Error(JSON.stringify(completed.params.turn));
    }
  } finally {
    rpc.stop();
  }

  /** 第六步：核对上游实际收到的参数——这才是“策略真的执行了”的证据。 */
  report.upstreamRequests = requests.map(({ bearer, ...rest }) => ({ ...rest, bearerSeen: bearer.startsWith('Bearer ') }));
  assert.equal(requests.length, seed.aliases.length, '每个模型应各产生一次上游请求');
  assert.ok(
    requests.every((request) => seed.upstreamIds.includes(request.model)),
    '上游收到的必须是真实上游 ID，不是 alias',
  );
  assert.ok(
    requests.every((request) => request.maxTokens === 4096),
    `模型声明的输出上限必须落到上游参数上，实际：${requests.map((r) => r.maxTokens).join(',')}`,
  );
  assert.ok(
    requests.every((request) => request.effort === 'low'),
    `声明的思考档位必须映射为 reasoning_effort，实际：${requests.map((r) => r.effort).join(',')}`,
  );
  assert.ok(requests.every((request) => request.messages >= 2), 'chat 适配必须产出 system + 用户消息');

  report.result = 'passed: 真实应用 + 真实网关 + 真实 Codex + chat 适配全链路打通（上游为本地 mock）';
  console.log(JSON.stringify(report, null, 2));
} finally {
  app.kill('SIGTERM');
  upstream.close();
  removeKeychainEntry();
  if (appLog.trim()) console.error(`--- 应用日志 ---\n${appLog}`);
}

/** 等应用监听端口，并用它自己安装的 helper 取令牌。 */
async function waitForGateway(revision) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      await stat(join(workdir, 'bin', 'gptswitch-auth-helper'));
      const token = execFileSync(join(workdir, 'bin', 'gptswitch-auth-helper'), ['--instance', 'local-main'], { encoding: 'utf8' });
      if (token.trim().length === 64) return token.trim();
    } catch {
      // 应用还没起来，继续等。
    }
    await new Promise((ok) => setTimeout(ok, 300));
  }
  throw new Error(`应用未在 30 秒内就绪；日志：${appLog}`);
}

async function fetchModels(revision, token) {
  const deadline = Date.now() + 20_000;
  let last = '';
  while (Date.now() < deadline) {
    const response = await fetch(`${gatewayOrigin}/i/inst_e2e/c/${revision}/v1/models`, {
      headers: { authorization: `Bearer ${token}` },
    }).catch((error) => ({ status: 0, text: async () => String(error) }));
    const text = await response.text();
    if (response.status === 200) {
      return JSON.parse(text).data.map((entry) => entry.id);
    }
    last = `${response.status} ${text}`;
    await new Promise((ok) => setTimeout(ok, 300));
  }
  throw new Error(`网关未在 20 秒内提供已发布目录；最后响应：${last}；日志：${appLog}`);
}

/** 写一份供人工核对的令牌副本（仅验收期间存在）。 */
await writeFile(join(workdir, 'e2e-note.txt'), '由 probe-full-loop.mjs 生成，可随时删除。\n').catch(() => {});
void readFile;
