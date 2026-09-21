import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  ApplyPlan,
  CodexInstance,
  ContentStatus,
  Credential,
  DiagnosticEvent,
  FeedItem,
  FeedSource,
  InstallPreview,
  InstallReport,
  Model,
  PluginSource,
  ProbeResult,
  Provider,
  RefreshReport,
  RepoCatalog,
  SkillRecord,
  SkillTarget,
  ToolState,
  UninstallOutcome,
  UpdateInfo,
} from '@/contracts/types';
import type { AppliedSummary, ApplyStatus, BackupEntry, CoexistState, DesktopClient, GatewayReport, HostRestart, InspectResult, DiagnosticsPreview, PlatformReport, UpdateProgress, UpdateReport } from './client';
import type { DiscoveredModel } from './client';
import { t } from '@/i18n';

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw { code: 'INTERNAL', messageKey: 'error.desktopRequired',
      safeDetails: [t('error.desktopRequired')], retryable: false, recoveryActions: [] };
  }
  return invoke<T>(command, args);
}

/** 事务提交类命令的返回形状，与 Rust 侧 ExecuteResult 一致。 */
interface ExecuteResult { operationId: string }

export const desktopClient: DesktopClient = {
  detectInstances: explicitPath => call<CodexInstance[]>('instances_detect', { explicitPath: explicitPath ?? null }),
  gatewayStatus: () => call<GatewayReport>('gateway_status'),
  platformInfo: () => call<PlatformReport>('platform_info'),
  applySummary: () => call<AppliedSummary | null>('apply_summary'),
  setGatewayPaused: paused => call<boolean>('gateway_set_paused', { paused }),
  listBackups: () => call<BackupEntry[]>('backups_list'),
  createBackup: instanceId => call<BackupEntry>('backups_create', { instanceId }),
  previewBackup: backupId => call<string>('backups_preview', { backupId }),
  restoreBackup: backupId => call<string>('backups_restore', { backupId }),
  checkUpdate: () => call<UpdateReport>('update_check'),
  installUpdate: () => call<void>('update_install'),
  takeUpdateResult: () => call<string | null>('update_take_result'),
  openReleasePage: url => call<void>('update_open_release_page', { url }),
  async onUpdateProgress(listener) {
    // 浏览器夹具里没有事件通道：返回一个空订阅，不假装有进度。
    if (!isTauri()) return () => {};
    return listen<UpdateProgress>('update://progress', event => listener(event.payload));
  },
  async listProviders(filter) {
    const providers = await call<Provider[]>('providers_list');
    const query = filter?.query?.trim().toLocaleLowerCase();
    return { items: query ? providers.filter(p => `${p.name} ${p.endpoint}`.toLocaleLowerCase().includes(query)) : providers, nextCursor: null };
  },
  saveProvider: (draft, expectedVersion) => call<Provider>('providers_save', { draft, expectedVersion }),
  listCredentials: providerId => call<Credential[]>('credentials_list', { providerId }),
  addCredential: (providerId, label, secret) => call<Credential>('credentials_add', { providerId, label, secret }),
  replaceCredential: (credentialId, secret, expectedVersion) => call<Credential>('credentials_replace', { credentialId, secret, expectedVersion }),
  selectCredential: (providerId, credentialId) => call<void>('credentials_select', { providerId, credentialId }),
  renameCredential: (credentialId, label, expectedVersion) => call<Credential>('credentials_rename', { credentialId, label, expectedVersion }),
  setCredentialDisabled: (credentialId, disabled, expectedVersion) => call<Credential>('credentials_set_disabled', { credentialId, disabled, expectedVersion }),
  listModels: () => call<Model[]>('models_list'),
  saveModel: (draft, expectedVersion) => call<Model>('models_save', { draft, expectedVersion }),
  deleteModel: (modelId, expectedVersion) => call<void>('models_delete', { modelId, expectedVersion }),
  deleteCredential: credentialId => call<void>('credentials_delete', { credentialId }),
  deleteProvider: providerId => call<void>('providers_delete', { providerId }),
  discoverModels: (providerId, credentialId) => call<DiscoveredModel[]>('models_discover', { providerId, credentialId }),
  // probeId 由界面生成：探测是阻塞调用，等返回再拿 id 就来不及取消了。
  startProbe: (target, options) => call<ProbeResult>('probes_start', {
    probeId: globalThis.crypto?.randomUUID?.() ?? null,
    providerId: target.providerId,
    credentialId: target.credentialId,
    modelId: target.modelId,
    includeGenerate: options?.includeGenerate ?? false,
  }),
  cancelProbe: probeId => call<boolean>('probes_cancel', { probeId }).then(() => undefined),
  inspectConfig: instanceId => call<InspectResult>('config_inspect', { instanceId }),
  // 默认模型由核心选择（取本次目录的第一个 alias）；界面暂不暴露该选择。
  planApply: request => call<ApplyPlan>('apply_plan', { instanceId: request.instanceId, defaultAlias: null }),
  executeApply: request => call<ExecuteResult>('apply_execute', { planId: request.planId, planHash: request.planHash, idempotencyKey: request.idempotencyKey }),
  applyStatus: operationId => call<ApplyStatus>('apply_status', { operationId }),
  confirmReload: (operationId, loaded) => call<ApplyStatus>('apply_confirm_reload', { operationId, loaded }),
  reconcileReload: () => call<{ confirmedOperationIds: string[] }>('apply_reconcile_reload', {}),
  restartHost: instanceId => call<HostRestart>('host_restart', { instanceId }),
  planRestore: instanceId => call<ApplyPlan>('restore_plan', { instanceId }),
  executeRestore: request => call<ExecuteResult>('restore_execute', { planId: request.planId, planHash: request.planHash, idempotencyKey: request.idempotencyKey }),
  coexistStatus: instanceId => call<CoexistState>('coexist_status', { instanceId }),
  setCoexist: (instanceId, enabled) => call<CoexistState>('coexist_set', { instanceId, enabled }),
  resyncCoexist: instanceId => call<CoexistState>('coexist_resync', { instanceId }),
  async listDiagnostics(filter) {
    const result = await call<{ items: DiagnosticEvent[]; nextCursor: string | null }>('diagnostics_list', { level: filter?.level ?? null });
    return result;
  },
  previewDiagnostics: request => call<DiagnosticsPreview>('diagnostics_preview', { scopes: request.scopes }),
  exportDiagnostics: request => call<{ savedPath: string }>('diagnostics_export', { scopes: request.scopes }),
  clearDiagnostics: () => call<number>('diagnostics_clear'),

  // 工具管理：locale 由界面传入，核心不猜当前语言。
  listTools: options => call<ToolState[]>('tools_state', {
    refresh: options?.refresh ?? false,
    locale: options?.locale ?? null,
  }),
  probeTool: (toolId, locale) => call<ToolState>('tools_probe', { toolId, locale: locale ?? null }),
  listSkillTargets: () => call<SkillTarget[]>('tools_skill_targets'),

  listPluginSources: () => call<PluginSource[]>('plugins_sources'),
  addPluginSource: repo => call<PluginSource[]>('plugins_add_source', { repo }),
  removePluginSource: repo => call<PluginSource[]>('plugins_remove_source', { repo }),
  browsePluginRepo: repo => call<RepoCatalog>('plugins_browse', { repo }),
  previewPluginInstall: request => call<InstallPreview>('plugins_preview', { request }),
  installPlugin: request => call<InstallReport>('plugins_install', { request }),
  listInstalledSkills: () => call<SkillRecord[]>('plugins_installed'),
  checkSkillUpdates: () => call<UpdateInfo[]>('plugins_check_updates'),
  setSkillEnabled: (skillId, targetTool, enabled) =>
    call<SkillRecord>('plugins_set_enabled', { skillId, targetTool, enabled }),
  uninstallSkill: (skillId, targets) => call<UninstallOutcome[]>('plugins_uninstall', { skillId, targets }),

  listFeedSources: () => call<FeedSource[]>('content_sources'),
  saveFeedSource: draft => call<FeedSource>('content_save_source', { draft }),
  deleteFeedSource: sourceId => call<void>('content_delete_source', { sourceId }),
  listFeedItems: filter => call<FeedItem[]>('content_items', {
    sourceId: filter.sourceId ?? null,
    lang: filter.lang ?? null,
    limit: filter.limit ?? 100,
    offset: filter.offset ?? 0,
  }),
  refreshContent: options => call<RefreshReport>('content_refresh', {
    sourceId: options?.sourceId ?? null,
    force: options?.force ?? false,
  }),
  contentStatus: () => call<ContentStatus>('content_status'),
  contentGithubTokenStatus: () => call<boolean>('content_github_token_status'),
  setContentGithubToken: token => call<boolean>('content_set_github_token', { token }),
};
