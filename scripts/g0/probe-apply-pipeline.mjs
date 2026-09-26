/**
 * G0 端到端探针：把 Switchelp 真实管线产出的 config.toml 与模型目录交给真实 Codex。
 *
 * 与 probe-catalog.mjs 的区别：那个用手写目录验证“目录能被解析”，
 * 这个用 CatalogCompiler + apply_managed 的真实产物验证同一件事，并额外验证
 * auth helper 是否真的被宿主调用。所有输入合成，CODEX_HOME 与用户现有配置隔离。
 *
 * 用法：node scripts/g0/probe-apply-pipeline.mjs
 */
import assert from 'node:assert/strict';
import http from 'node:http';
import { execFileSync } from 'node:child_process';
import { mkdir, readFile, writeFile, rm, stat } from 'node:fs/promises';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { AppServer } from './rpc.mjs';
import { resolveCodexBinary } from './codex-binary.mjs';

const binary = resolveCodexBinary();
const workdir = process.env.GPTSWITCH_G0_DIR ?? join(await mkdtemp(join(tmpdir(), 'gptswitch-g0-apply-')), 'run');
await rm(workdir, { recursive: true, force: true });
await mkdir(workdir, { recursive: true, mode: 0o700 });

/** 第一步：跑真实管线，拿到它自己写出的 config.toml、目录与 auth helper。 */
const manifest = JSON.parse(execFileSync('cargo', ['run', '-q', '-p', 'switch-core', '--example', 'g0_apply_pipeline', '--', workdir], {
  cwd: resolve('.'), encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'],
}));
assert.equal(manifest.stage, 'AwaitingReload', '提交成功只能到等待重载');
assert.ok(!manifest.configText.includes('synthetic-upstream-secret'), 'config.toml 不得含上游 Key');

/** 第二步：按 config.toml 里的真实 base_url 起 mock 上游，端口与前缀都不改写。 */
const base = new URL(manifest.configText.match(/base_url = "([^"]+)"/)[1]);
assert.equal(base.port, '18765', 'base_url 端口应来自核心的默认网关端口');
const requests = [];
const server = http.createServer(async (req, res) => {
  if (req.method !== 'POST' || !req.url.endsWith('/responses')) {
    res.writeHead(404, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: { message: 'Not a mock Responses endpoint' } }));
    return;
  }
  if (req.headers.authorization !== `Bearer ${manifest.authToken}`) {
    res.writeHead(401, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: { message: `bad bearer: ${req.headers.authorization}` } }));
    return;
  }
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const body = JSON.parse(Buffer.concat(chunks).toString());
  const known = manifest.expectedAliases.includes(body.model);
  if (!known) {
    res.writeHead(400, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: { message: `unknown alias ${body.model}` } }));
    return;
  }
  requests.push({ path: req.url, model: body.model, effort: body.reasoning?.effort, inputItems: body.input?.length });
  emitResponses(res, body.model, `Switchelp 路由验证成功：${body.model}`);
});
await new Promise((ok, no) => server.listen(Number(base.port), '127.0.0.1', ok).on('error', no));

const rpc = new AppServer(binary, {
  PATH: process.env.PATH, HOME: process.env.HOME, TMPDIR: process.env.TMPDIR ?? tmpdir(),
  LANG: 'en_US.UTF-8', CODEX_HOME: manifest.codexHome,
});

const report = {
  observedAt: new Date().toISOString(),
  binaryVersion: execFileSync(binary, ['--version'], { encoding: 'utf8' }).trim(),
  workdir,
  codexHome: manifest.codexHome,
  configPath: manifest.configPath,
  catalogPath: manifest.catalogPath,
  appliedByPipeline: { operationId: manifest.operationId, planHash: manifest.planHash, stage: manifest.stage },
  catalog: manifest.catalog,
};

try {
  report.initialized = await rpc.initialize();

  // 第三步：模型菜单契约——真实 app-server 是否列出我们编译出的 alias。
  const models = [];
  let cursor;
  do {
    const page = await rpc.call('model/list', { limit: 1, ...(cursor ? { cursor } : {}) });
    models.push(...page.data);
    cursor = page.nextCursor;
  } while (cursor);
  report.modelList = models.map(m => ({ id: m.model, displayName: m.displayName, inputModalities: m.inputModalities, reasoningEfforts: (m.supportedReasoningEfforts ?? []).map(r => r.reasoningEffort), defaultEffort: m.defaultReasoningEffort }));

  const listed = models.map(m => m.model).sort();
  assert.deepEqual(listed, [...manifest.expectedAliases].sort(), '原生菜单应恰好列出本次编译出的 alias');
  for (const entry of manifest.catalog.models) {
    const observed = models.find(m => m.model === entry.slug);
    assert.ok(observed, `目录条目未出现在 model/list：${entry.slug}`);
    assert.deepEqual(observed.inputModalities, entry.input_modalities, `input_modalities 不一致：${entry.slug}`);
    assert.deepEqual((observed.supportedReasoningEfforts ?? []).map(r => r.reasoningEffort).sort(), entry.supported_reasoning_levels.map(l => l.effort).sort(), `reasoning 档位不一致：${entry.slug}`);
    assert.equal(observed.displayName, entry.display_name, `display_name 不一致：${entry.slug}`);
  }

  // 第四步：路由——真实一轮对话是否经 auth helper 打到 base_url 的实例前缀上。
  for (const alias of manifest.expectedAliases) {
    const started = await rpc.call('thread/start', { model: alias, modelProvider: 'gptswitch', cwd: workdir, approvalPolicy: 'never', sandbox: 'read-only', ephemeral: true });
    const turn = await rpc.call('turn/start', { threadId: started.thread.id, input: [{ type: 'text', text: 'Say the route verification result only.', text_elements: [] }] });
    const expires = Date.now() + 20000;
    while (!rpc.notifications.some(n => n.method === 'turn/completed' && n.params?.turn?.id === turn.turn.id)) {
      if (Date.now() > expires) throw new Error(`回合未完成：${alias}；${rpc.stderr}`);
      await new Promise(ok => setTimeout(ok, 100));
    }
    const completed = rpc.notifications.find(n => n.method === 'turn/completed' && n.params?.turn?.id === turn.turn.id);
    if (completed.params.turn.status !== 'completed') throw new Error(JSON.stringify(completed.params.turn));
  }
  report.routing = { status: 'passed', requests };
  assert.equal(requests.length, manifest.expectedAliases.length, '每个模型应各产生一次上游请求');
  assert.ok(requests.every(r => r.path.startsWith(base.pathname)), '请求必须带本实例的目录前缀');

  // 第五步：auth helper 是否真的被宿主调用（决定 Command 型认证可用性）。
  report.authHelperInvocations = await helperLog(manifest.authHelper);
  assert.ok(report.authHelperInvocations.length > 0, '宿主从未调用 auth helper；Command 型认证未生效');

  report.result = 'passed: 真实管线产物在真实 Codex 中列出并被路由（app-server 层）；Desktop UI 仍未验证';
  await mkdir(resolve('.local'), { recursive: true });
  await writeFile(resolve('.local/g0-apply-pipeline.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ status: report.result, modelList: report.modelList, requests: report.routing.requests, authHelperInvocations: report.authHelperInvocations }, null, 2));
} finally {
  rpc.stop();
  server.close();
}

async function helperLog(helperPath) {
  const logPath = join(helperPath, '..', 'auth-helper-invocations.log');
  try {
    await stat(logPath);
  } catch {
    return [];
  }
  return (await readFile(logPath, 'utf8')).trim().split('\n').filter(Boolean);
}

function emitResponses(res, model, text) {
  const id = `resp_gptswitch_${Date.now()}`;
  const itemId = `msg_gptswitch_${Date.now()}`;
  const part = { type: 'output_text', text, annotations: [] };
  const item = { id: itemId, type: 'message', role: 'assistant', status: 'completed', content: [part] };
  const response = { id, object: 'response', created_at: Math.floor(Date.now() / 1000), model, status: 'completed', output: [item], usage: { input_tokens: 12, output_tokens: 8, total_tokens: 20, input_tokens_details: { cached_tokens: 0 }, output_tokens_details: { reasoning_tokens: 0 } } };
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  let sequence = 0;
  const emit = (type, payload) => res.write(`event: ${type}\ndata: ${JSON.stringify({ type, sequence_number: sequence++, ...payload })}\n\n`);
  emit('response.created', { response: { ...response, status: 'in_progress', output: [], usage: null } });
  emit('response.output_item.added', { output_index: 0, item: { ...item, status: 'in_progress', content: [] } });
  emit('response.content_part.added', { item_id: itemId, output_index: 0, content_index: 0, part: { ...part, text: '' } });
  emit('response.output_text.delta', { item_id: itemId, output_index: 0, content_index: 0, delta: text });
  emit('response.output_text.done', { item_id: itemId, output_index: 0, content_index: 0, text });
  emit('response.content_part.done', { item_id: itemId, output_index: 0, content_index: 0, part });
  emit('response.output_item.done', { output_index: 0, item });
  emit('response.completed', { response });
  res.end();
}
