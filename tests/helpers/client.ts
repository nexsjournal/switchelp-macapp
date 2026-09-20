import { vi } from 'vitest';
import type { ApplyPlan, CodexInstance, FieldChange } from '@/contracts/types';
import type { DesktopClient } from '@/desktop/client';
import type { Provider } from '@/contracts/types';

export const provider: Provider = { id: 'p_test', name: '测试供应商', endpoint: 'https://example.test/v1', protocol: 'responses',
  authKind: 'api_key', activeCredentialId: null, enabled: true, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z' };

export const instance: CodexInstance = { id: 'inst_test', appPath: '/Applications/ChatGPT.app', cliPath: '/Applications/ChatGPT.app/Contents/Resources/codex',
  desktopVersion: null, cliVersion: null, configRoot: '/tmp/gptswitch-test/.codex', configFile: '/tmp/gptswitch-test/.codex/config.toml',
  configExists: true, startupMode: 'not_running', compatibility: 'unverified', fingerprint: {}, conflictingManagers: [], blockedReasonKey: null };

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
    setGatewayPaused: vi.fn().mockResolvedValue(false),
    listBackups: vi.fn().mockResolvedValue([]),
    createBackup: failing(),
    previewBackup: failing(),
    restoreBackup: failing(),
    checkUpdate: failing(),
    platformInfo: vi.fn().mockResolvedValue({ platform: 'macos', titlebarHeight: 44, leadingReserve: 84, systemDecorations: true }),
    gatewayStatus: vi.fn().mockResolvedValue({ running: true, paused: false, port: 18765, served: 0, revisions: [], tokenFingerprint: 'deadbeef', error: null }),
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
    ...overrides,
  };
}
