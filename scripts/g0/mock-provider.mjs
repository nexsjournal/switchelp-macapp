import http from 'node:http';
import { readFile, writeFile, appendFile } from 'node:fs/promises';
import { join } from 'node:path';
import { AppServer } from './rpc.mjs';
import { resolveCodexBinary } from './codex-binary.mjs';

const report = JSON.parse(await readFile('.local/g0-catalog.json', 'utf8'));
const binary = resolveCodexBinary();
let serial = 0;
const requests = [];
const server = http.createServer(async (req, res) => {
  if (req.url?.endsWith('/responses') && req.method === 'POST') {
    if (req.headers.authorization !== 'Bearer synthetic-g0-token') {
      res.writeHead(401); res.end(); return;
    }
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const body = JSON.parse(Buffer.concat(chunks).toString());
    if (!['gptswitch/probe-a', 'gptswitch/probe-b'].includes(body.model)) {
      res.writeHead(400); res.end(JSON.stringify({ error: { message: 'Unknown test model' } })); return;
    }
    const record = { model: body.model, effort: body.reasoning?.effort, inputItems: body.input?.length, at: new Date().toISOString() };
    requests.push(record);
    await appendFile('.local/g0-requests.jsonl', JSON.stringify(record) + '\n');
    const id = `resp_gptswitch_${++serial}`;
    const itemId = `msg_gptswitch_${serial}`;
    const text = `Switchelp 路由验证成功：${body.model}`;
    const part = { type: 'output_text', text, annotations: [] };
    const item = { id: itemId, type: 'message', role: 'assistant', status: 'completed', content: [part] };
    const response = { id, object: 'response', created_at: Math.floor(Date.now() / 1000), model: body.model, status: 'completed', output: [item], usage: { input_tokens: 12, output_tokens: 8, total_tokens: 20, input_tokens_details: { cached_tokens: 0 }, output_tokens_details: { reasoning_tokens: 0 } } };
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
  } else {
    res.writeHead(404, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: { message: 'Not a mock Responses endpoint' } }));
  }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const port = server.address().port;
const configPath = join(report.testCodexHome, 'config.toml');
const config = await readFile(configPath, 'utf8');
await writeFile(configPath, config.replace(/base_url = .*/, `base_url = "http://127.0.0.1:${port}/v1"`));
report.mockPort = port;
report.mockPid = process.pid;
await writeFile('.local/g0-catalog.json', JSON.stringify(report, null, 2));

const rpc = new AppServer(binary, { PATH: process.env.PATH, HOME: process.env.HOME, CODEX_HOME: report.testCodexHome });
try {
  await rpc.initialize();
  for (const model of ['gptswitch/probe-a', 'gptswitch/probe-b']) {
    const started = await rpc.call('thread/start', { model, modelProvider: 'gptswitch', cwd: report.testRoot, approvalPolicy: 'never', sandbox: 'read-only', ephemeral: true });
    const turn = await rpc.call('turn/start', { threadId: started.thread.id, input: [{ type: 'text', text: 'Say the route verification result only.', text_elements: [] }] });
    const expires = Date.now() + 20000;
    while (!rpc.notifications.some(n => n.method === 'turn/completed' && n.params?.turn?.id === turn.turn.id)) {
      if (Date.now() > expires) throw new Error(`Turn did not complete: ${model}; ${rpc.stderr}`);
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    const completed = rpc.notifications.find(n => n.method === 'turn/completed' && n.params?.turn?.id === turn.turn.id);
    if (completed.params.turn.status !== 'completed') throw new Error(JSON.stringify(completed.params.turn));
  }
  report.routingProbe = { status: 'passed', requests, observedAt: new Date().toISOString() };
  await writeFile('.local/g0-catalog.json', JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ status: 'mock ready; both real app-server turns completed', port, pid: process.pid, requests, testCodexHome: report.testCodexHome }));
} catch (error) {
  server.close();
  throw error;
} finally { rpc.stop(); }
process.on('SIGTERM', () => server.close(() => process.exit(0)));
