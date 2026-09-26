import assert from 'node:assert/strict';
import { mkdtemp, writeFile, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { testCatalog } from './catalog.mjs';
import { AppServer } from './rpc.mjs';
import { resolveCodexBinary } from './codex-binary.mjs';

const binary = resolveCodexBinary();
const testRoot = await mkdtemp(join(tmpdir(), 'gptswitch-g0-'));
const testCodexHome = join(testRoot, 'codex-home');
await mkdir(testCodexHome, { mode: 0o700 });
const catalogPath = join(testCodexHome, 'models.json');
await writeFile(catalogPath, JSON.stringify(testCatalog(), null, 2));
await writeFile(join(testCodexHome, 'config.toml'), [
  'model = "gptswitch/probe-a"',
  'model_provider = "gptswitch"',
  `model_catalog_json = ${JSON.stringify(catalogPath)}`,
  '[analytics]',
  'enabled = false',
  '[model_providers.gptswitch]',
  'name = "Switchelp"',
  'base_url = "http://127.0.0.1:18765/v1"',
  'wire_api = "responses"',
  'experimental_bearer_token = "synthetic-g0-token"',
].join('\n') + '\n', { mode: 0o600 });

const environment = {
  PATH: process.env.PATH,
  HOME: process.env.HOME,
  TMPDIR: process.env.TMPDIR ?? tmpdir(),
  LANG: 'en_US.UTF-8',
  CODEX_HOME: testCodexHome,
};
const rpc = new AppServer(binary, environment);
try {
  const initialized = await rpc.initialize();
  const models = [];
  let cursor;
  do {
    const page = await rpc.call('model/list', { limit: 1, ...(cursor ? { cursor } : {}) });
    models.push(...page.data);
    cursor = page.nextCursor;
  } while (cursor);
  assert.equal(models.length, 2);
  assert.deepEqual(models.map(m => m.model).sort(), ['gptswitch/probe-a', 'gptswitch/probe-b']);
  assert.deepEqual(models[0].inputModalities, ['text']);
  assert.deepEqual(models[1].inputModalities, ['text', 'image']);
  const report = {
    observedAt: new Date().toISOString(),
    binaryVersion: execFileSync(binary, ['--version'], { encoding: 'utf8' }).trim(),
    testRoot,
    testCodexHome,
    initialized,
    models,
    result: 'app-server custom catalog and pagination passed; Desktop UI and routing still unverified',
  };
  await mkdir(resolve('.local'), { recursive: true });
  await writeFile(resolve('.local/g0-catalog.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
} finally {
  rpc.stop();
}
