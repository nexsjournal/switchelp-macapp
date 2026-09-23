/**
 * 视觉检查夹具：用合成数据渲染真实界面，供与 `referimg/` 参考截图逐页比对。
 *
 * 只在开发服务器上使用（`pnpm dev` 后访问 `/visual.html`）；不参与打包，
 * 也不作为业务真相——这里的数据只用于看排版、层级、间距和状态文案。
 *
 * 支持 `?view=codex|app|tools|plugins|content|settings` 直接进入对应页面，方便自动截图。
 */
import { StrictMode, type ReactElement } from 'react';
import { createRoot } from 'react-dom/client';
import type {
  ApplyPlan, CodexInstance, Credential, FeedItem, FeedSource, FieldChange, Model, Provider,
  RepoCatalog, SkillRecord, ToolState,
} from '@/contracts/types';
import type { ApplyStatus, DesktopClient, InspectResult } from '@/desktop/client';
import { App } from '@/app/App';
import { ProviderForm } from '@/features/providers/ProviderForm';
import { ModelFormDialog } from '@/features/models/ModelFormDialog';
import { ModelEditorPage } from '@/features/models/ModelEditorPage';
import { applyTheme, readThemePreference } from '@/theme';
import { defaultPolicy } from '@/features/models/policy';
import '@/styles/tokens.css';
import '@/styles/global.css';

// 与正式入口一样走主题模块；`?theme=light` 便于逐主题走查。
const themeOverride = new URLSearchParams(window.location.search).get('theme');
applyTheme(themeOverride === 'light' || themeOverride === 'dark' ? themeOverride : readThemePreference());

const provider = (id: string, name: string, endpoint: string, active: string | null): Provider => ({
  id, name, endpoint, protocol: 'responses', authKind: 'api_key', activeCredentialId: active,
  enabled: true, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
});

function model(id: string, providerId: string, displayName: string, upstreamId: string, context: number, output: number, hostState: Model['hostState'], vision = false): Model {
  return {
    id, providerId, upstreamId, catalogAlias: `gs/${id}`, displayName, lifecycle: 'saved', hostState,
    inCatalog: hostState !== 'not_in_catalog',
    policy: {
      contextLimit: context, outputLimit: output, compactLimit: null,
      reasoning: { support: 'supported', control: 'effort', allowedValues: ['low', 'high'], defaultValue: 'low', budgetTokens: null, mappingId: 'reasoning.effort.v1' },
      // 能力表：文本永远支持，视觉按参数给——模型行上的「视觉」徽章就是这么来的。
      inputs: defaultPolicy().inputs.map(entry => entry.kind === 'text' ? { ...entry, upstream: 'supported' as const }
        : entry.kind === 'image' && vision ? { ...entry, upstream: 'supported' as const, gateway: 'supported' as const } : entry),
      tools: { functionTools: 'supported', parallelTools: 'unknown', customTools: 'unsupported', builtinTools: 'unsupported', verification: 'declared' },
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
  model('m_1', 'p_a', 'deepseek-v4.1', 'vendor/reasoner-pro', 1_048_576, 65_536, 'awaiting_reload', true),
  model('m_2', 'p_a', '视觉多模态模型', 'vendor/vision-flash', 262_144, 32_768, 'awaiting_reload', true),
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

const coexist = {
  enabled: false,
  bridgeReady: true,
  bridgeDetail: null,
  bridgePath: '/Users/me/Library/Application Support/app.gptswitch.desktop/bin/gptswitch-bridge',
  managedHome: '/Users/me/Library/Application Support/app.gptswitch.desktop/codex-home',
  managedConfigExists: true,
  hostUnderBridge: null as boolean | null,
  ready: true,
  blockedReason: null as string | null,
};

/**
 * 工具管理：覆盖全部六种状态。夹具里刻意放一条 `unverified`（探针退出码不是 0），
 * 那是唯一可能被误看成「已就绪」的状态，必须在走查里看得见。
 */
const toolStates: ToolState[] = [
  {
    id: 'codex', displayName: 'Codex CLI', category: 'cliCode', description: 'OpenAI 的编码 agent CLI。本工具的供应商与模型配置最终写进它的 config.toml。',
    status: 'ready',
    installed: { path: '/opt/homebrew/bin/codex', pathSource: 'path', version: 'codex-cli 0.154.0-alpha.6.2', configPath: '/Users/me/.codex/config.toml', configExists: true, skillsPath: '/Users/me/.codex/skills', skillsCount: 3 },
    website: 'https://github.com/openai/codex', docs: 'https://github.com/openai/codex', modelConfig: true, skillTarget: true,
    agentUsage: { tags: ['编码 agent', 'CLI'], nonInteractive: ['codex exec "<任务>"', 'codex --help'] },
    versionProbeTail: 'codex-cli 0.154.0-alpha.6.2', authProbeTail: null, notes: [], probedAt: 1_789_956_000, cacheSeconds: 300,
  },
  {
    id: 'claude-code', displayName: 'Claude Code', category: 'cliCode', description: 'Anthropic 的编码 agent CLI。本工具只做检测与技能安装，不改它的模型配置。',
    status: 'needsLogin',
    installed: { path: '/Users/me/.npm-global/bin/claude', pathSource: 'path', version: '2.0.14 (Claude Code)', configPath: '/Users/me/.claude/settings.json', configExists: false, skillsPath: '/Users/me/.claude/skills', skillsCount: 12 },
    website: 'https://claude.com/product/claude-code', docs: 'https://docs.claude.com/en/docs/claude-code', modelConfig: true, skillTarget: true,
    agentUsage: { tags: ['编码 agent', 'CLI'], nonInteractive: ['claude -p "<任务>"'] },
    versionProbeTail: '2.0.14 (Claude Code)', authProbeTail: 'Not logged in. Run claude login to authenticate.', notes: [], probedAt: 1_789_956_000, cacheSeconds: 300,
  },
  {
    id: 'github-cli', displayName: 'GitHub CLI', category: 'utility', description: '官方 GitHub 命令行。它用 GitHub 自己的身份认证，没有模型可选。',
    status: 'installed',
    installed: { path: '/opt/homebrew/bin/gh', pathSource: 'path', version: 'gh version 2.62.0 (2025-01-15)', configPath: null, configExists: false, skillsPath: null, skillsCount: 0 },
    website: 'https://cli.github.com/', docs: 'https://cli.github.com/manual/', modelConfig: false, skillTarget: false,
    agentUsage: { tags: ['代码托管', 'GitHub'], nonInteractive: ['gh repo clone <owner/repo>', 'gh pr create --fill'] },
    versionProbeTail: 'gh version 2.62.0 (2025-01-15)', authProbeTail: 'operation not permitted: unable to read keyring', notes: [], probedAt: 1_789_956_000, cacheSeconds: 300,
  },
  {
    id: 'aider', displayName: 'Aider', category: 'cliCode', description: '在终端里结对改代码的 agent，按 diff 提交到 git。',
    status: 'unverified',
    installed: { path: '/Users/me/.local/bin/aider', pathSource: 'candidate', version: null, configPath: null, configExists: false, skillsPath: null, skillsCount: 0 },
    website: 'https://aider.chat/', docs: 'https://aider.chat/docs/', modelConfig: true, skillTarget: false,
    agentUsage: { tags: ['编码 agent', 'Python'], nonInteractive: [] },
    versionProbeTail: 'aider: command timed out after 10s', authProbeTail: null,
    notes: ['版本探针超时，已终止该进程'], probedAt: 1_789_956_000, cacheSeconds: 300,
  },
  {
    id: 'ffmpeg', displayName: 'FFmpeg', category: 'utility', description: '音视频转码工具。装上之后 agent 才能处理视频、音频与抽帧。',
    status: 'notInstalled', installed: null,
    website: 'https://ffmpeg.org/', docs: 'https://ffmpeg.org/documentation.html', modelConfig: false, skillTarget: false,
    agentUsage: { tags: ['音视频'], nonInteractive: ['ffmpeg -i in.mp4 out.gif'] },
    versionProbeTail: '', authProbeTail: null, notes: [], probedAt: 1_789_956_000, cacheSeconds: 3600,
  },
  {
    id: 'hermes', displayName: 'Hermes', category: 'cliCode', description: 'Nous Research 的 agent 运行时。',
    status: 'unsupportedPlatform', installed: null,
    website: null, docs: null, modelConfig: true, skillTarget: false,
    agentUsage: { tags: ['编码 agent'], nonInteractive: [] },
    versionProbeTail: '', authProbeTail: null,
    notes: ['清单里没有 macos 平台的候选路径，也没有可查找的命令名'], probedAt: 1_789_956_000, cacheSeconds: 300,
  },
];

const skillDocumentBody = `# 前端设计

在用户要求改界面时使用。先读现有组件规范，再动手。

## 规矩

- 深色、灰阶卡片、白色主按钮。
- 字号与行高成对定义，不单独改字号。
- 状态必须来自观察，不写没有来源的数字。`;

const pluginCatalog: RepoCatalog = {
  repo: 'anthropics/skills', commit: '9c1f0a7b2e4d5f6a7b8c9d0e1f2a3b4c5d6e7f80', fetchedAt: 1_789_956_000, truncated: false,
  skills: [
    {
      dirName: 'frontend-design', sourcePath: 'skills/frontend-design',
      document: { id: 'frontend-design', title: null, description: '界面设计与实现规范，改动前端时使用。', requiresBins: [], body: skillDocumentBody, frontMatterParsed: true },
      files: [{ path: 'SKILL.md', bytes: 412, text: skillDocumentBody }],
    },
    {
      dirName: 'doc-coauthoring', sourcePath: 'skills/doc-coauthoring',
      document: { id: 'doc-coauthoring', title: null, description: '与用户合写文档：先问再写，不替用户编。', requiresBins: ['pandoc'], body: '# 文档协作\n\n先确认受众与篇幅。', frontMatterParsed: true },
      files: [{ path: 'SKILL.md', bytes: 240, text: '# 文档协作' }],
    },
    {
      dirName: 'internal-comms', sourcePath: 'skills/internal-comms',
      document: { id: 'internal-comms', title: null, description: null, requiresBins: [], body: '正文（这份文档的头部没能解析，名字回落到目录名）。', frontMatterParsed: false },
      files: [{ path: 'SKILL.md', bytes: 180, text: '正文' }],
    },
  ],
};

const installedSkills: SkillRecord[] = [
  {
    skillId: 'xs-github', dirName: 'xs-github', targetTool: 'codex', targetDisplayName: 'Codex CLI',
    sourceRepo: 'anthropics/skills', sourceCommit: '9c1f0a7b2e4d5f6a7b8c9d0e1f2a3b4c5d6e7f80', sourcePath: 'skills/xs-github',
    installedPath: '/Users/me/.codex/skills/xs-github', enabled: true, installedAt: 1_789_900_000,
    files: [{ path: 'SKILL.md', sha256: 'a1b2c3', bytes: 1253 }],
  },
  {
    skillId: 'frontend-design', dirName: 'frontend-design.disabled', targetTool: 'claude-code', targetDisplayName: 'Claude Code',
    sourceRepo: 'anthropics/skills', sourceCommit: '0f1e2d3c4b5a6978', sourcePath: 'skills/frontend-design',
    installedPath: '/Users/me/.claude/skills/frontend-design.disabled', enabled: false, installedAt: 1_789_910_000,
    files: [{ path: 'SKILL.md', sha256: 'd4e5f6', bytes: 412 }],
  },
];

const feedSources: FeedSource[] = [
  { id: 'sspai', kind: 'rss', url: 'https://sspai.com/feed', label: '少数派', lang: 'zh', enabled: true, etag: '"v12"', lastModified: null, lastOkAt: 1_789_955_400, lastError: null, failStreak: 0, nextFetchAt: 1_789_959_000, builtin: true },
  { id: 'ruanyifeng', kind: 'rss', url: 'https://www.ruanyifeng.com/blog/atom.xml', label: '阮一峰的网络日志', lang: 'zh', enabled: true, etag: null, lastModified: 'Mon, 21 Sep 2026 02:10:00 GMT', lastOkAt: 1_789_955_400, lastError: null, failStreak: 0, nextFetchAt: 1_789_959_000, builtin: true },
  { id: 'hn', kind: 'rss', url: 'https://hnrss.org/newest?points=100', label: 'Hacker News（100 分以上）', lang: 'en', enabled: true, etag: null, lastModified: null, lastOkAt: 1_789_900_000, lastError: '访问 https://hnrss.org/newest?points=100 失败：连接超时', failStreak: 2, nextFetchAt: 1_789_960_000, builtin: true },
  { id: 'github-week', kind: 'githubSearch', url: 'week', label: 'GitHub 本周热门', lang: '', enabled: true, etag: null, lastModified: null, lastOkAt: 1_789_955_400, lastError: null, failStreak: 0, nextFetchAt: 1_789_959_000, builtin: true },
];

const feedItems: FeedItem[] = [
  { url: 'https://sspai.com/post/90001', sourceId: 'sspai', sourceLabel: '少数派', title: '我用三个月把本地模型接进了日常工作流', summary: '从选型到落地，一份能直接抄的配置清单。', publishedAt: 1_789_950_000, firstSeenAt: 1_789_950_100, lang: 'zh', stars: null, repo: null },
  { url: 'https://github.com/owner/agent-kit', sourceId: 'github-week', sourceLabel: 'GitHub 本周热门', title: 'owner/agent-kit', summary: 'TypeScript · 把 agent 的工具调用收进一个可测的接口层。', publishedAt: 1_789_955_400, firstSeenAt: 1_789_955_500, lang: '', stars: 4821, repo: 'owner/agent-kit' },
  { url: 'https://www.ruanyifeng.com/blog/2026/09/weekly.html', sourceId: 'ruanyifeng', sourceLabel: '阮一峰的网络日志', title: '科技爱好者周刊（第 380 期）', summary: '本期话题：本地优先的软件。', publishedAt: 1_789_900_000, firstSeenAt: 1_789_900_100, lang: 'zh', stars: null, repo: null },
];

const client: DesktopClient = {
  detectInstances: async () => [instance],
  platformInfo: async () => ({ platform: 'macos', titlebarHeight: 44, leadingReserve: 84, systemDecorations: true }),
  applySummary: async () => ({ operationId: 'op_a', instanceId: 'inst_a', catalogRevision: 'rev_046731cc', defaultModel: 'gs/m_1', aliasCount: 2, stage: 'verified', appliedAt: '2026-09-18T03:30:03Z' }),
  gatewayStatus: async () => ({ running: true, paused: false, port: 18765, served: 12, revisions: ['rev_046731cc'], tokenFingerprint: '3f9a1c04', error: null, systemProxy: { httpEnabled: false, endpoint: null, bypassApplied: false } }),
  setGatewayPaused: async (paused: boolean) => paused,
  listBackups: async () => ([{ id: 'b_1', sourcePath: '/Users/me/.codex/config.toml', createdAt: '2026-09-18T03:20:00Z', contentHash: 'a1b2c3d4', bytes: 412, mayContainSecrets: true }]),
  createBackup: async () => ({ id: 'b_2', sourcePath: '/Users/me/.codex/config.toml', createdAt: '2026-09-18T04:00:00Z', contentHash: 'e5f6a7b8', bytes: 420, mayContainSecrets: true }),
  previewBackup: async () => 'model = "gpt-5-codex"\nmodel_provider = "••••••••"\n',
  restoreBackup: async () => '/Users/me/.codex/config.toml',
  /*
   * 更新：夹具里永远是「有新版本」，好让走查每次都看得到侧栏那颗胶囊。
   * 安装按钮**不会真的装**——它只按节奏吐进度事件，然后停在那儿（真实安装成功时
   * 应用会重启，不会有收尾界面）。走查要看的正是「下载中」这个状态。
   */
  checkUpdate: async () => ({
    current: '0.2.0', latest: '0.3.0', hasUpdate: true,
    notes: '这一版把更新做成了应用内完成。\n\n## 更新\n\n- 侧栏左上角新增**更新胶囊**，有新版时顶掉副标题那一行\n- 安装包经 `minisign` 验签，校验不过就拒绝安装\n- 装完自动重启，重启后用一条提示告诉你「已更新到 x.y.z」\n\n## 修复\n\n- 修掉热切主题后颜色量不准的问题',
    releaseUrl: 'https://github.com/nexsjournal/switchelp-macapp/releases/tag/v0.3.0',
    publishedAt: '2026-09-21', error: null }),
  installUpdate: () => new Promise<void>(() => { /* 真实安装成功时不会 resolve（应用重启），夹具照此保持「下载中」。 */ }),
  takeUpdateResult: async () => null,
  openReleasePage: async () => undefined,
  onUpdateProgress: async listener => {
    let done = 0;
    const total = 5_457_549;
    const timer = setInterval(() => {
      done = Math.min(done + 700_000, total * 0.6);
      listener({ phase: 'download', downloaded: done, total });
    }, 250);
    return () => clearInterval(timer);
  },
  listProviders: async () => ({ items: providers, nextCursor: null }),
  saveProvider: async draft => provider('p_new', draft.name, draft.endpoint, null),
  listCredentials: async id => credentials.filter(c => c.providerId === id),
  addCredential: async () => credentials[0]!,
  replaceCredential: async () => credentials[0]!,
  selectCredential: async () => undefined,
  // 夹具里的 Key 池是只读的：改名/停用真的落到合成数据上，走查才能看到状态变化。
  renameCredential: async (credentialId: string, label: string) => {
    const found = credentials.find(item => item.id === credentialId);
    if (!found) throw new Error('missing credential');
    found.label = label;
    found.version += 1;
    return found;
  },
  setCredentialDisabled: async (credentialId: string, disabled: boolean) => {
    const found = credentials.find(item => item.id === credentialId);
    if (!found) throw new Error('missing credential');
    found.status = disabled ? 'disabled' : 'saved';
    found.version += 1;
    return found;
  },
  discoverModels: async () => [
    { upstreamId: 'vendor/reasoner-pro', displayName: '深度推理模型', alreadySaved: true },
    { upstreamId: 'vendor/new-vision', displayName: '新视觉模型', alreadySaved: false },
    { upstreamId: 'Vendor/Case-Sensitive-2.5-Pro', displayName: '大小写敏感模型 2.5 Pro', alreadySaved: false },
  ],
  listModels: async () => models,
  saveModel: async () => models[0]!,
  deleteModel: async () => undefined,
  deleteCredential: async () => undefined,
  deleteProvider: async () => undefined,
  // 只读探测的合成结果：夹具里不发真实请求，但四个阶段都要成功，才看得到「连接成功」那条结论。
  startProbe: async () => ({
    id: 'probe_fixture', targetLabel: '示例供应商 A / deepseek-v4.1', startedAt: '2026-09-18T00:00:00Z',
    stages: [
      { stageKey: 'connect', status: 'passed', elapsedMs: 42, messageKey: 'probe.connected' },
      { stageKey: 'credential', status: 'passed', elapsedMs: 12, messageKey: 'probe.credentialAccepted' },
      { stageKey: 'model', status: 'passed', elapsedMs: 88, messageKey: 'probe.modelFound' },
    ],
  }),
  cancelProbe: async () => undefined,
  inspectConfig: async () => inspect,
  planApply: async () => plan,
  executeApply: async () => ({ operationId: 'op_a' }),
  applyStatus: async () => status,
  restartHost: async () => ({ appPath: '/Applications/ChatGPT.app', quitConfirmed: true, quitForced: false, launchedConfirmed: true }),
  reconcileReload: async () => ({ confirmedOperationIds: [] }),
  confirmReload: async () => ({ operationId: 'op_a', open: false, events: [...status.events, { ...status.events[3]!, sequence: 4, phase: 'verified', messageKey: 'stage.verified' }] }),
  planRestore: async () => plan,
  executeRestore: async () => ({ operationId: 'op_r' }),
  // 走查夹具里的共存状态是可切的：走查要能看到开启/关闭两种版面。
  coexistStatus: async () => coexist,
  setCoexist: async (_instanceId: string, enabled: boolean) => {
    coexist.enabled = enabled;
    coexist.hostUnderBridge = enabled ? true : null;
    return coexist;
  },
  resyncCoexist: async () => coexist,
  listDiagnostics: async () => ({ items: [], nextCursor: null }),
  previewDiagnostics: async () => ({ items: [], totalBytes: 0 }),
  exportDiagnostics: async () => ({ savedPath: '/tmp/diagnostics.json' }),
  clearDiagnostics: async () => 0,
  // 工具管理：夹具返回合成状态。「重新检测」真的重排时间戳，走查时能看到变化。
  listTools: async options => options?.refresh
    ? toolStates.map(state => ({ ...state, probedAt: Math.floor(Date.now() / 1000) }))
    : toolStates,
  probeTool: async toolId => toolStates.find(state => state.id === toolId) ?? toolStates[0]!,
  listSkillTargets: async () => ([
    { toolId: 'codex', displayName: 'Codex CLI', root: '/Users/me/.codex/skills' },
    { toolId: 'claude-code', displayName: 'Claude Code', root: '/Users/me/.claude/skills' },
  ]),
  // 插件中心：目录与已装列表都是合成数据，安装动作只改内存里的已装列表。
  listPluginSources: async () => ([
    { repo: 'anthropics/skills', label: 'Anthropic 官方技能集合', description: '官方公开的 Agent Skills，包含文档、设计与协作相关的技能。', builtin: true },
    { repo: 'obra/superpowers', label: 'Superpowers', description: '社区维护的技能框架，覆盖头脑风暴、排查与并行协作等做法。', builtin: true },
    { repo: 'wshobson/agents', label: 'Agents 插件合集', description: '面向编码 agent 的插件与技能合集，数量多但取向偏工程。', builtin: true },
  ]),
  addPluginSource: async () => ([]),
  removePluginSource: async () => ([]),
  browsePluginRepo: async () => pluginCatalog,
  previewPluginInstall: async request => ({
    repo: pluginCatalog.repo,
    commit: pluginCatalog.commit,
    skills: pluginCatalog.skills
      .filter(skill => request.skillDirs.length === 0 || request.skillDirs.includes(skill.dirName))
      .map(skill => ({
        skillId: skill.document.id, dirName: skill.dirName, sourcePath: skill.sourcePath,
        description: skill.document.description, requiresBins: skill.document.requiresBins,
        targets: request.targets.map(toolId => {
          // 夹具里 doc-coauthoring 在 Codex 上已经有别人的同名目录，用来走查冲突态。
          const conflict = toolId === 'codex' && skill.dirName === 'doc-coauthoring';
          const keepBoth = request.conflictChoices[`${toolId}::${skill.dirName}`] === 'keepBoth';
          return {
            toolId,
            displayName: toolId === 'codex' ? 'Codex CLI' : 'Claude Code',
            root: toolId === 'codex' ? '/Users/me/.codex/skills' : '/Users/me/.claude/skills',
            dir: `${toolId === 'codex' ? '/Users/me/.codex/skills' : '/Users/me/.claude/skills'}/${keepBoth ? `${skill.dirName}-2` : skill.dirName}`,
            dirName: keepBoth ? `${skill.dirName}-2` : skill.dirName,
            action: conflict && !keepBoth ? 'conflict' as const : 'create' as const,
            files: skill.files.map(file => ({ path: file.path, bytes: file.bytes, sha256: 'synthetic' })),
            conflictDetail: conflict && !keepBoth ? '/Users/me/.codex/skills/doc-coauthoring 已存在，且含有 2 个不是本工具写入的文件' : null,
            foreignFiles: conflict ? ['notes.md', 'draft.md'] : [],
          };
        }),
      })),
  }),
  installPlugin: async request => {
    const records: SkillRecord[] = pluginCatalog.skills
      .filter(skill => request.skillDirs.includes(skill.dirName))
      .flatMap(skill => request.targets.map(toolId => ({
        skillId: skill.document.id, dirName: skill.dirName, targetTool: toolId,
        targetDisplayName: toolId === 'codex' ? 'Codex CLI' : 'Claude Code',
        sourceRepo: pluginCatalog.repo, sourceCommit: pluginCatalog.commit, sourcePath: skill.sourcePath,
        installedPath: `${toolId === 'codex' ? '/Users/me/.codex/skills' : '/Users/me/.claude/skills'}/${skill.dirName}`,
        enabled: true, installedAt: Math.floor(Date.now() / 1000),
        files: skill.files.map(file => ({ path: file.path, sha256: 'synthetic', bytes: file.bytes })),
      })));
    installedSkills.push(...records);
    return { repo: pluginCatalog.repo, commit: pluginCatalog.commit, installed: records, skipped: [], failed: [] };
  },
  listInstalledSkills: async () => installedSkills,
  checkSkillUpdates: async () => ([
    { skillId: 'xs-github', targetTool: 'codex', currentCommit: '9c1f0a7b2e4d5f6a', latestCommit: 'aa11bb22cc33dd44' },
  ]),
  setSkillEnabled: async (skillId, targetTool, enabled) => {
    const record = installedSkills.find(item => item.skillId === skillId && item.targetTool === targetTool)!;
    record.enabled = enabled;
    record.dirName = enabled ? record.dirName.replace(/\.disabled$/, '') : `${record.dirName.replace(/\.disabled$/, '')}.disabled`;
    record.installedPath = record.installedPath.replace(/\.disabled$/, '') + (enabled ? '' : '.disabled');
    return record;
  },
  uninstallSkill: async () => ([]),
  // 内容中心：失败源与退避时间都按真实语义合成，走查时能看到琥珀状态行。
  listFeedSources: async () => feedSources,
  saveFeedSource: async draft => ({
    id: draft.id ?? 'feed-new', kind: draft.kind, url: draft.url, label: draft.label, lang: draft.lang,
    enabled: draft.enabled, etag: null, lastModified: null, lastOkAt: null, lastError: null,
    failStreak: 0, nextFetchAt: Math.floor(Date.now() / 1000), builtin: false,
  }),
  deleteFeedSource: async () => undefined,
  listFeedItems: async filter => feedItems.filter(item => !filter.sourceId || item.sourceId === filter.sourceId),
  refreshContent: async () => ({
    attempted: feedSources.map(source => source.id),
    succeeded: feedSources.slice(0, 3).map(source => source.id),
    notModified: [], failed: [],
    skipped: [], newItems: 2, nextFetchAt: Math.floor(Date.now() / 1000) + 3600,
  }),
  contentStatus: async () => ({
    lastOkAt: 1_789_955_400, nextFetchAt: 1_789_959_000, scheduleHours: [6, 18],
    failing: [{ sourceId: 'hn', label: 'Hacker News（100 分以上）', message: '访问 https://hnrss.org/newest?points=100 失败：连接超时', failStreak: 2 }],
    totalItems: 128,
  }),
  contentGithubTokenStatus: async () => false,
  setContentGithubToken: async token => Boolean(token),
};

const view = new URLSearchParams(window.location.search).get('view') ?? '';
const container = document.getElementById('root');
if (!container) throw new Error('缺少 #root 容器');
// 走查首次接入向导：清空供应商，让向导自动展开。
if (view === 'onboarding') {
  // 向导的「已看过」标记是持久的（点过「稍后再说」就记下了），不清掉这个视图会时有时无。
  try { localStorage.removeItem('gptswitch.onboarding.dismissed'); } catch { /* 没有存储时按「没看过」处理 */ }
  (client as { listProviders: unknown }).listProviders = async () => ({ items: [], nextCursor: null });
  (client as { listModels: unknown }).listModels = async () => [];
}
// 走查「还没有应用」的那一版待应用条：默认夹具里模型是 awaiting_reload，量不到「N 个模型待应用」。
if (view === 'pending') {
  (client as { listModels: unknown }).listModels = async () => models.map(model => ({ ...model, hostState: 'pending_apply' as const }));
  (client as { applySummary: unknown }).applySummary = async () => null;
}
// 走查「系统代理把回环地址也代理走」的那一版连接状态：默认夹具是「没开代理」，
// 那一行量不到长句在卡片里的换行与对比度。
if (view === 'proxy') {
  (client as { gatewayStatus: unknown }).gatewayStatus = async () => ({
    running: true, paused: false, port: 18765, served: 12, revisions: ['rev_046731cc'],
    tokenFingerprint: '3f9a1c04', error: null,
    systemProxy: { httpEnabled: true, endpoint: '127.0.0.1:7890', bypassApplied: true },
  });
}

// 组件级直连视图：无交互截图用（headless Chrome / 审计脚本），不经过 App 壳。
const noop = () => {};
/**
 * 组件级视图没有宿主可退回，所以「关闭 / 取消 / 后退」这些出口一律真的跳回供应商页。
 *
 * 以前这些出口传的是空函数，于是走查时点「取消」界面纹丝不动——看起来像按钮坏了，
 * 其实是夹具没接。夹具的职责是让真实界面能被完整走一遍，包括出口。
 */
const backToProviders = () => { window.location.href = '/visual.html?view=providers'; };
const directViews: Record<string, () => ReactElement> = {
  // 供应商弹窗·编辑态：地址、格式、Key、模型列表
  'provider-form': () => <ProviderForm client={client} provider={providers[0]} providers={providers} models={models}
    onSaved={noop} onKeysChanged={noop} onChanged={noop} onClose={backToProviders} />,
  // 供应商弹窗·新建态
  'provider-form-new': () => <ProviderForm client={client} providers={providers} models={models}
    onSaved={noop} onKeysChanged={noop} onChanged={noop} onClose={backToProviders} />,
  // 手工添加模型（高级配置折叠）
  'model-form': () => <ModelFormDialog client={client} providerId="p_a" onSaved={noop} onClose={backToProviders} />,
  // 编辑已有模型（高级配置折叠，长度已填）
  'model-form-edit': () => <ModelFormDialog client={client} providerId="p_a" model={models[0]}
    onSaved={noop} onClose={backToProviders} />,
  // 整页模型编辑器（新模型：可见链路不支持输入的灰态）
  'model-editor': () => <ModelEditorPage client={client} providers={providers} onSaved={backToProviders} onCancel={backToProviders} />,
};

createRoot(container).render(
  <StrictMode>
    {directViews[view] ? (
      // 与 App 壳的 main 一致的容器：这些视图本来就在 main 的 32px 内边距里渲染。
      <main style={{ height: '100%', overflow: 'auto', padding: 'var(--content-padding)' }}>
        {directViews[view]()}
      </main>
    ) : <App client={client}
      initialPage={
        view === 'codex' ? 'codexConfig'
          : view === 'providers' ? 'providers'
          : view === 'tools' ? 'tools'
          : view === 'plugins' ? 'plugins'
          : view === 'content' ? 'content'
          : view === 'settings' ? 'settings'
          : 'overview'
      } />}
  </StrictMode>,
);
