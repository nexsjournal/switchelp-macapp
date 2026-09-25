import { vi } from 'vitest';
import type { ApplyPlan, CodexInstance, FieldChange } from '@/contracts/types';
import type { DesktopClient } from '@/desktop/client';
import type { Provider } from '@/contracts/types';
import type { UsageReport, UsageTotals } from '@/contracts/types';

export const provider: Provider = { id: 'p_test', name: '测试供应商', endpoint: 'https://example.test/v1', protocol: 'responses',
  authKind: 'api_key', activeCredentialId: null, enabled: true, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z' };

export const instance: CodexInstance = { id: 'inst_test', appPath: '/Applications/ChatGPT.app', cliPath: '/Applications/ChatGPT.app/Contents/Resources/codex',
  desktopVersion: null, cliVersion: null, configRoot: '/tmp/gptswitch-test/.codex', configFile: '/tmp/gptswitch-test/.codex/config.toml',
  configExists: true, startupMode: 'not_running', compatibility: 'unverified', fingerprint: {}, conflictingManagers: [], blockedReasonKey: null };

/** 共存模式关闭时的状态。测试要用开启状态时自己覆盖字段。 */
export const coexistOff = {
  enabled: false,
  bridgeReady: true,
  bridgeDetail: null,
  bridgePath: '/tmp/gptswitch-test/bin/gptswitch-bridge',
  managedHome: '/tmp/gptswitch-test/codex-home',
  managedConfigExists: false,
  hostUnderBridge: null,
  ready: true,
  blockedReason: null,
};

/** 与应用/还原无关的字段保持默认，测试只声明自己关心的差异。 */
export function plan(changes: FieldChange[], overrides: Partial<ApplyPlan> = {}): ApplyPlan {
  return { id: 'plan_test', instanceId: 'inst_test', revisionId: 'rev_test', planHash: 'hash_test',
    expectedConfigHash: 'config_hash_before', expectedConfigExists: true, configPath: '/tmp/gptswitch-test/.codex/config.toml',
    createdAtUnix: 1_700_000_000, ttlSecs: 600, changes, reloadScope: 'host_reload', catalogRevision: 'rev_test',
    catalogAliases: ['p_test-alias'], warnings: [], touchesActiveTasks: false, ...overrides };
}

/**
 * 仅测试使用；未安排返回值的命令必须失败，避免模拟成功掩盖未实现流程。
 *
 * 每个方法各自一个 mock：早期版本共用一个，于是对 A 方法的调用会让
 * 「B 方法未被调用」的断言误报——而断言可靠正是这里存在的意义。
 */
export function testClient(overrides: Partial<DesktopClient> = {}): DesktopClient {
  const failing = () => vi.fn().mockRejectedValue(new Error('test command not configured'));
  return {
    detectInstances: failing(),
    applySummary: vi.fn().mockResolvedValue(null),
    reconcileReload: vi.fn().mockResolvedValue({ confirmedOperationIds: [] }),
    setGatewayPaused: vi.fn().mockResolvedValue(false),
    listBackups: vi.fn().mockResolvedValue([]),
    createBackup: failing(),
    previewBackup: failing(),
    restoreBackup: failing(),
    checkUpdate: failing(),
    installUpdate: failing(),
    // 默认「上次没有发生更新」：这是绝大多数用例的真实前提，也免得每次渲染都报一次未配置。
    takeUpdateResult: vi.fn().mockResolvedValue(null),
    openReleasePage: failing(),
    openExternalUrl: vi.fn().mockResolvedValue(undefined),
    onUpdateProgress: vi.fn().mockResolvedValue(() => {}),
    platformInfo: vi.fn().mockResolvedValue({ platform: 'macos', titlebarHeight: 44, leadingReserve: 84, systemDecorations: true }),
    gatewayStatus: vi.fn().mockResolvedValue({ running: true, paused: false, port: 18765, served: 0, revisions: [], tokenFingerprint: 'deadbeef', error: null, systemProxy: { httpEnabled: false, endpoint: null, bypassApplied: false } }),
    listProviders: vi.fn().mockResolvedValue({ items: [], nextCursor: null }),
    saveProvider: failing(),
    listCredentials: vi.fn().mockResolvedValue([]),
    addCredential: failing(),
    replaceCredential: failing(),
    selectCredential: failing(),
    renameCredential: failing(),
    setCredentialDisabled: failing(),
    deleteProvider: failing(),
    deleteCredential: failing(),
    deleteModel: failing(),
    discoverModels: failing(),
    listModels: vi.fn().mockResolvedValue([]),
    saveModel: failing(),
    startProbe: failing(),
    cancelProbe: failing(),
    inspectConfig: failing(),
    // 共存模式默认关：绝大多数测试跑的是「替换菜单」这条老路。
    coexistStatus: vi.fn().mockResolvedValue(coexistOff),
    setCoexist: failing(),
    resyncCoexist: failing(),
    planApply: failing(),
    executeApply: failing(),
    applyStatus: failing(),
    confirmReload: failing(),
    restartHost: failing(),
    planRestore: failing(),
    executeRestore: failing(),
    listDiagnostics: vi.fn().mockResolvedValue({ items: [], nextCursor: null }),
    previewDiagnostics: failing(),
    exportDiagnostics: failing(),
    clearDiagnostics: vi.fn().mockResolvedValue(0),
    // 扩展板块：读取类默认返回空清单，写类默认失败（未安排的命令不许假装成功）。
    listTools: vi.fn().mockResolvedValue([]),
    probeTool: failing(),
    listSkillTargets: vi.fn().mockResolvedValue([]),
    listPluginSources: vi.fn().mockResolvedValue([]),
    addPluginSource: failing(),
    removePluginSource: failing(),
    browsePluginRepo: failing(),
    previewPluginInstall: failing(),
    installPlugin: failing(),
    listInstalledSkills: vi.fn().mockResolvedValue([]),
    checkSkillUpdates: vi.fn().mockResolvedValue([]),
    setSkillEnabled: failing(),
    uninstallSkill: failing(),
    listFeedSources: vi.fn().mockResolvedValue([]),
    saveFeedSource: failing(),
    deleteFeedSource: failing(),
    listFeedItems: vi.fn().mockResolvedValue([]),
    refreshContent: failing(),
    contentStatus: vi.fn().mockResolvedValue({ lastOkAt: null, nextFetchAt: 0, scheduleHours: [6, 18], failing: [], totalItems: 0 }),
    contentGithubTokenStatus: vi.fn().mockResolvedValue(false),
    setContentGithubToken: failing(),
    // 默认「本机没有任何会话记录」：这是空态用例的前提，也免得每处渲染都要造一份数据。
    usageReport: vi.fn().mockResolvedValue(emptyUsageReport()),
    ...overrides,
  };
}

/** 零用量的用量报告；测试要造数据时用 `usageReport(days)` 覆盖。 */
export function emptyUsageReport(overrides: Partial<UsageReport> = {}): UsageReport {
  return {
    sourceDirectory: '/tmp/gptswitch-test/.codex',
    rangeDays: 30,
    scannedFiles: 0,
    unreadableFiles: 0,
    sessions: 0,
    totals: zeroTotals(),
    daily: [],
    byModel: [],
    byProvider: [],
    planWindow: null,
    ...overrides,
  };
}

export function zeroTotals(): UsageTotals {
  return { inputTokens: 0, cachedTokens: 0, cacheWriteTokens: 0, outputTokens: 0, reasoningTokens: 0, totalTokens: 0 };
}
