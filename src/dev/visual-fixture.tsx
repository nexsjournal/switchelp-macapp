/**
 * 视觉检查夹具：用合成数据渲染真实界面，供与 `referimg/` 参考截图逐页比对。
 *
 * 只在开发服务器上使用（`pnpm dev` 后访问 `/visual.html`）；不参与打包，
 * 也不作为业务真相——这里的数据只用于看排版、层级、间距和状态文案。
 *
 * 支持 `?view=codex|app` 直接进入对应页面，方便自动截图。
 */
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import type { ApplyPlan, CodexInstance, Credential, FieldChange, Model, Provider } from '@/contracts/types';
import type { ApplyStatus, DesktopClient, InspectResult } from '@/desktop/client';
import { App } from '@/app/App';
import { applyTheme, readThemePreference } from '@/theme';
import '@/styles/tokens.css';
import '@/styles/global.css';

// 与正式入口一样走主题模块；`?theme=light` 便于逐主题走查。
const themeOverride = new URLSearchParams(window.location.search).get('theme');
applyTheme(themeOverride === 'light' || themeOverride === 'dark' ? themeOverride : readThemePreference());

const provider = (id: string, name: string, endpoint: string, active: string | null): Provider => ({
  id, name, endpoint, protocol: 'responses', authKind: 'api_key', activeCredentialId: active,
  enabled: true, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
});

function model(id: string, providerId: string, displayName: string, upstreamId: string, context: number, output: number, hostState: Model['hostState']): Model {
  return {
    id, providerId, upstreamId, catalogAlias: `gs/${id}`, displayName, lifecycle: 'saved', hostState,
    inCatalog: hostState !== 'not_in_catalog',
    policy: {
      contextLimit: context, outputLimit: output, compactLimit: null,
      reasoning: { support: 'supported', control: 'effort', allowedValues: ['low', 'high'], defaultValue: 'low', budgetTokens: null, mappingId: 'reasoning.effort.v1' },
      inputs: [],
      tools: { functionTools: 'supported', parallelTools: 'unknown', customTools: 'unsupported', verification: 'declared' },
    },
    displayNameLayer: { discovered: null, userValue: displayName, overridden: true },
    capabilityRevision: 1, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
  };
}

const providers = [
  provider('p_a', '示例供应商 A', 'https://api.example-a.com/v1', 'k_1'),
  provider('p_b', 'Example Provider B', 'https://api.example-b.com/v1', null),
];
const models = [
  model('m_1', 'p_a', '深度推理模型', 'vendor/reasoner-pro', 1_048_576, 65_536, 'awaiting_reload'),
  model('m_2', 'p_a', '视觉多模态模型', 'vendor/vision-flash', 262_144, 32_768, 'awaiting_reload'),
  model('m_3', 'p_b', '轻量快速模型', 'vendor/fast-mini', 131_072, 8_192, 'loaded'),
  model('m_4', 'p_b', '长文本模型', 'vendor/long-context', 1_000_000, 16_384, 'not_in_catalog'),
];
const credentials: Credential[] = [
  { id: 'k_1', providerId: 'p_a', label: '日常', secretRef: 'gptswitch/p_a/k_1/v1', secretVersion: 1, maskedSuffix: '••••4f2a', status: 'verified', scope: null, lastVerifiedAt: '2026-09-18T00:00:00Z', version: 1, createdAt: '2026-09-18T00:00:00Z' },
  { id: 'k_2', providerId: 'p_a', label: '备用', secretRef: 'gptswitch/p_a/k_2/v1', secretVersion: 1, maskedSuffix: '••••91c7', status: 'saved', scope: null, lastVerifiedAt: null, version: 1, createdAt: '2026-09-18T00:00:00Z' },
];

const instance: CodexInstance = {
  id: 'inst_a', appPath: '/Applications/ChatGPT.app', cliPath: '/Applications/ChatGPT.app/Contents/Resources/codex',
  desktopVersion: '1.0.0', cliVersion: '0.1.0', configRoot: '/Users/me/.codex',
  configFile: '/Users/me/.codex/config.toml', configExists: true, startupMode: 'not_running',
  compatibility: 'unverified', fingerprint: { cliVersion: '0.1.0' }, conflictingManagers: ['other-tool'], blockedReasonKey: null,
};

const changes: FieldChange[] = [
  { keyPath: 'model', before: null, after: 'gs/m_1', reasonKey: 'reason.defaultModel' },
  { keyPath: 'model_provider', before: 'openai', after: 'gptswitch', reasonKey: 'reason.providerRoute' },
  { keyPath: 'model_catalog_json', before: '/Users/me/Library/Application Support/OpenCodex/custom_model_catalog.json', after: '/Users/me/Library/Application Support/app.gptswitch.desktop/catalogs/rev_046731cc/models.json', reasonKey: 'reason.catalog' },
  { keyPath: 'model_providers.gptswitch', before: null, after: 'base_url = "http://127.0.0.1:18765/i/inst_a/c/rev_046731cc/v1"', reasonKey: 'reason.gatewayProvider' },
];

const plan: ApplyPlan = {
  id: 'plan_a', instanceId: 'inst_a', revisionId: 'rev_046731cc', planHash: 'hash', expectedConfigHash: 'cfg',
  expectedConfigExists: true, configPath: '/Users/me/.codex/config.toml', createdAtUnix: 1_789_701_895, ttlSecs: 600,
  changes, reloadScope: 'host_reload', catalogRevision: 'rev_046731cc', catalogAliases: ['gs/m_1', 'gs/m_2'],
  warnings: ['warning.reasoningNotSelectableInHost：轻量快速模型的推理控制在 Codex 中不可切换，仅使用网关固定策略'],
  touchesActiveTasks: false,
};

const inspect: InspectResult = {
  instanceId: 'inst_a', configPath: '/Users/me/.codex/config.toml',
  redactedPreview: 'model = "gpt-5.6-sol"\nmodel_provider = "openai"\napproval_policy = "on-request"\n\n[model_providers.other-tool]\nname = "其他配置工具"\nbase_url = "http://127.0.0.1:49152/v1"\nexperimental_bearer_token = "••••redacted"\n',
  managedFields: [], conflicts: ['other-tool'],
};

const status: ApplyStatus = {
  operationId: 'op_a', open: true,
  events: [
    { schemaVersion: 1, operationId: 'op_a', sequence: 0, phase: 'validating', revisionId: 'rev_046731cc', messageKey: 'stage.validating', safeArgs: {}, cancellable: true, timestamp: '2026-09-18T03:30:00Z' },
    { schemaVersion: 1, operationId: 'op_a', sequence: 1, phase: 'prepared', revisionId: 'rev_046731cc', messageKey: 'stage.prepared', safeArgs: {}, cancellable: true, timestamp: '2026-09-18T03:30:01Z' },
    { schemaVersion: 1, operationId: 'op_a', sequence: 2, phase: 'committing', revisionId: 'rev_046731cc', messageKey: 'stage.committing', safeArgs: {}, cancellable: false, timestamp: '2026-09-18T03:30:02Z' },
    { schemaVersion: 1, operationId: 'op_a', sequence: 3, phase: 'awaiting_reload', revisionId: 'rev_046731cc', messageKey: 'stage.awaitingReload', safeArgs: {}, cancellable: false, timestamp: '2026-09-18T03:30:03Z' },
  ],
};

const client: DesktopClient = {
  detectInstances: async () => [instance],
  platformInfo: async () => ({ platform: 'macos', titlebarHeight: 44, leadingReserve: 84, systemDecorations: true }),
  applySummary: async () => ({ operationId: 'op_a', instanceId: 'inst_a', catalogRevision: 'rev_046731cc', defaultModel: 'gs/m_1', aliasCount: 2, stage: 'verified', appliedAt: '2026-09-18T03:30:03Z' }),
  gatewayStatus: async () => ({ running: true, paused: false, port: 18765, served: 12, revisions: ['rev_046731cc'], tokenFingerprint: '3f9a1c04', error: null }),
  setGatewayPaused: async (paused: boolean) => paused,
  listBackups: async () => ([{ id: 'b_1', sourcePath: '/Users/me/.codex/config.toml', createdAt: '2026-09-18T03:20:00Z', contentHash: 'a1b2c3d4', bytes: 412, mayContainSecrets: true }]),
  createBackup: async () => ({ id: 'b_2', sourcePath: '/Users/me/.codex/config.toml', createdAt: '2026-09-18T04:00:00Z', contentHash: 'e5f6a7b8', bytes: 420, mayContainSecrets: true }),
  previewBackup: async () => 'model = "gpt-5-codex"\nmodel_provider = "••••••••"\n',
  restoreBackup: async () => '/Users/me/.codex/config.toml',
  checkUpdate: async () => ({ current: '0.1.0', latest: '0.2.0', hasUpdate: true, releaseUrl: 'https://github.com/nexsjournal/switchelp-macapp/releases', publishedAt: '2026-09-18T00:00:00Z', error: null }),
  listProviders: async () => ({ items: providers, nextCursor: null }),
  saveProvider: async draft => provider('p_new', draft.name, draft.endpoint, null),
  listPresets: async () => [],
  listCredentials: async id => credentials.filter(c => c.providerId === id),
  addCredential: async () => credentials[0]!,
  replaceCredential: async () => credentials[0]!,
  selectCredential: async () => undefined,
  discoverModels: async () => [],
  listModels: async () => models,
  saveModel: async () => models[0]!,
  deleteModel: async () => undefined,
  deleteCredential: async () => undefined,
  deleteProvider: async () => undefined,
  startProbe: async () => { throw new Error('fixture'); },
  cancelProbe: async () => undefined,
  inspectConfig: async () => inspect,
  planApply: async () => plan,
  executeApply: async () => ({ operationId: 'op_a' }),
  applyStatus: async () => status,
  confirmReload: async () => ({ operationId: 'op_a', open: false, events: [...status.events, { ...status.events[3]!, sequence: 4, phase: 'verified', messageKey: 'stage.verified' }] }),
  planRestore: async () => plan,
  executeRestore: async () => ({ operationId: 'op_r' }),
  listDiagnostics: async () => ({ items: [], nextCursor: null }),
  previewDiagnostics: async () => ({ items: [], totalBytes: 0 }),
  exportDiagnostics: async () => ({ savedPath: '/tmp/diagnostics.json' }),
  clearDiagnostics: async () => 0,
};

const view = new URLSearchParams(window.location.search).get('view');
const container = document.getElementById('root');
if (!container) throw new Error('缺少 #root 容器');
// 走查首次接入向导：清空供应商，让向导自动展开。
if (view === 'onboarding') {
  (client as { listProviders: unknown }).listProviders = async () => ({ items: [], nextCursor: null });
  (client as { listModels: unknown }).listModels = async () => [];
}

createRoot(container).render(
  <StrictMode>
    <App client={client} initialPage={view === 'codex' ? 'codexConfig' : 'overview'} />
  </StrictMode>,
);
