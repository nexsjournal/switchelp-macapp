/**
 * 壳无关的业务契约。React 业务组件只依赖本接口；
 * Tauri invoke/listen 只出现在 transport.ts。
 * 方法名对应 docs/architecture/04-data-and-contracts.md 第 4 节 IPC 命令表。
 */
import type {
  ApplyPlan,
  CodexInstance,
  CoreError,
  Credential,
  DiagnosticEvent,
  Model,
  OperationEvent,
  ProbeResult,
  Provider,
  ProviderPreset,
} from '@/contracts/types';

export interface ListResult<T> {
  items: T[];
  nextCursor: string | null;
}

export interface ProviderDraft {
  id?: string;
  name: string;
  endpoint: string;
  protocol: Provider['protocol'];
  authKind: Provider['authKind'];
  presetId?: string | null;
  notes?: string | null;
  enabled: boolean;
}

export interface ModelDraft {
  id?: string;
  providerId: string;
  upstreamId: string;
  catalogAlias: string;
  displayName: string;
  policy: Model['policy'];
  inCatalog: boolean;
  displayNameOverridden: boolean;
}

export interface DiscoveredModel {
  upstreamId: string;
  displayName: string;
  alreadySaved: boolean;
}

export interface PlanRequest {
  instanceId: string;
  draftRevision: string;
}

export interface ExecutionRequest {
  planId: string;
  planHash: string;
  idempotencyKey: string;
}

export interface ApplyStatus {
  operationId: string;
  events: OperationEvent[];
  /** 是否仍有未完成阶段；窗口重开后先查快照再订阅。 */
  open: boolean;
}

export interface InspectResult {
  instanceId: string;
  configPath: string;
  /** 已脱敏的只读 TOML 预览。 */
  redactedPreview: string;
  managedFields: string[];
  conflicts: string[];
}

export interface DiagnosticsRequest {
  scopes: string[];
  redactionPreviewHash: string;
}

export interface DiagnosticsPreview {
  items: { name: string; included: boolean; note: string }[];
  totalBytes: number;
}

/** 本机网关状态。未启动时 `error` 必须带出原因，界面不得显示成正常。 */
export interface GatewayReport {
  running: boolean;
  port: number | null;
  served: number;
  /** 已发布的目录版本；空表示尚未应用过任何配置。 */
  revisions: string[];
  tokenFingerprint: string;
  error: string | null;
}

/** 平台与窗口策略。 */
export interface PlatformReport {
  platform: 'macos' | 'windows' | 'linux';
  titlebarHeight: number;
  leadingReserve: number;
  systemDecorations: boolean;
}

export interface DesktopClient {
  /** 只读检测，不修改任何配置。 */
  detectInstances(explicitPath?: string): Promise<CodexInstance[]>;

  /** 本机网关是否在监听。 */
  gatewayStatus(): Promise<GatewayReport>;

  /** 平台与窗口策略。界面据此设置 data-platform 与窗口相关变量。 */
  platformInfo(): Promise<PlatformReport>;

  listProviders(filter?: { query?: string }): Promise<ListResult<Provider>>;
  saveProvider(draft: ProviderDraft, expectedVersion: number): Promise<Provider>;
  listPresets(): Promise<ProviderPreset[]>;

  listCredentials(providerId: string): Promise<Credential[]>;
  /** 秘密只在这一个调用里传入，不进入任何持久化状态。 */
  addCredential(providerId: string, label: string, secret: string): Promise<Credential>;
  replaceCredential(credentialId: string, secret: string, expectedVersion: number): Promise<Credential>;
  selectCredential(providerId: string, credentialId: string): Promise<void>;

  discoverModels(providerId: string, credentialId: string): Promise<DiscoveredModel[]>;
  listModels(): Promise<Model[]>;
  saveModel(draft: ModelDraft, expectedVersion: number): Promise<Model>;

  /** 探测只读阶段默认不发真实请求；`includeGenerate` 才会产生费用与副作用。 */
  startProbe(target: { providerId: string; modelId?: string; credentialId: string }, options?: { includeGenerate?: boolean }): Promise<ProbeResult>;
  cancelProbe(probeId: string): Promise<void>;

  inspectConfig(instanceId: string): Promise<InspectResult>;
  planApply(request: PlanRequest): Promise<ApplyPlan>;
  executeApply(request: ExecutionRequest): Promise<{ operationId: string }>;
  applyStatus(operationId: string): Promise<ApplyStatus>;
  /** 只有用户确认宿主已重新加载，事务才从“等待重载”前进；不得由前端自行宣称已加载。 */
  confirmReload(operationId: string, loaded: boolean): Promise<ApplyStatus>;
  planRestore(instanceId: string): Promise<ApplyPlan>;
  executeRestore(request: ExecutionRequest): Promise<{ operationId: string }>;

  listDiagnostics(filter?: { level?: DiagnosticEvent['level'] }): Promise<ListResult<DiagnosticEvent>>;
  previewDiagnostics(request: DiagnosticsRequest): Promise<DiagnosticsPreview>;
  exportDiagnostics(request: DiagnosticsRequest): Promise<{ savedPath: string }>;
}

/** 错误归一化：后端 CoreError 与前端未知错误都收敛成同一形状。 */
export function isCoreError(value: unknown): value is CoreError {
  return (
    typeof value === 'object' &&
    value !== null &&
    'code' in value &&
    'messageKey' in value &&
    'safeDetails' in value
  );
}

export function toCoreError(value: unknown): CoreError {
  if (isCoreError(value)) return value;
  return {
    code: 'INTERNAL',
    messageKey: 'error.internal',
    safeDetails: ['操作失败，请重试或查看诊断。'],
    retryable: false,
    recoveryActions: [],
  };
}
