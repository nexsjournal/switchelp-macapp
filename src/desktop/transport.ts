import { invoke, isTauri } from '@tauri-apps/api/core';
import type { ApplyPlan, CodexInstance, Credential, DiagnosticEvent, Model, ProbeResult, Provider } from '@/contracts/types';
import type { ApplyStatus, DesktopClient, GatewayReport, InspectResult, DiagnosticsPreview, PlatformReport } from './client';
import type { DiscoveredModel } from './client';

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw { code: 'INTERNAL', messageKey: 'error.desktopRequired',
      safeDetails: ['请使用桌面应用运行。浏览器预览不会保存数据或读取 API Key。'], retryable: false, recoveryActions: [] };
  }
  return invoke<T>(command, args);
}

/** 事务提交类命令的返回形状，与 Rust 侧 ExecuteResult 一致。 */
interface ExecuteResult { operationId: string }

export const desktopClient: DesktopClient = {
  detectInstances: explicitPath => call<CodexInstance[]>('instances_detect', { explicitPath: explicitPath ?? null }),
  gatewayStatus: () => call<GatewayReport>('gateway_status'),
  platformInfo: () => call<PlatformReport>('platform_info'),
  async listProviders(filter) {
    const providers = await call<Provider[]>('providers_list');
    const query = filter?.query?.trim().toLocaleLowerCase();
    return { items: query ? providers.filter(p => `${p.name} ${p.endpoint}`.toLocaleLowerCase().includes(query)) : providers, nextCursor: null };
  },
  saveProvider: (draft, expectedVersion) => call<Provider>('providers_save', { draft, expectedVersion }),
  listPresets: async () => [],
  listCredentials: providerId => call<Credential[]>('credentials_list', { providerId }),
  addCredential: (providerId, label, secret) => call<Credential>('credentials_add', { providerId, label, secret }),
  replaceCredential: (credentialId, secret, expectedVersion) => call<Credential>('credentials_replace', { credentialId, secret, expectedVersion }),
  selectCredential: (providerId, credentialId) => call<void>('credentials_select', { providerId, credentialId }),
  listModels: () => call<Model[]>('models_list'),
  saveModel: (draft, expectedVersion) => call<Model>('models_save', { draft, expectedVersion }),
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
  planRestore: instanceId => call<ApplyPlan>('restore_plan', { instanceId }),
  executeRestore: request => call<ExecuteResult>('restore_execute', { planId: request.planId, planHash: request.planHash, idempotencyKey: request.idempotencyKey }),
  async listDiagnostics(filter) {
    const result = await call<{ items: DiagnosticEvent[]; nextCursor: string | null }>('diagnostics_list', { level: filter?.level ?? null });
    return result;
  },
  previewDiagnostics: request => call<DiagnosticsPreview>('diagnostics_preview', { scopes: request.scopes }),
  exportDiagnostics: request => call<{ savedPath: string }>('diagnostics_export', { scopes: request.scopes }),
};
