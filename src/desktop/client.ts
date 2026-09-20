/**
 * 壳无关的业务契约。React 业务组件只依赖本接口；
 * Tauri invoke/listen 只出现在 transport.ts。
 * 方法名对应 docs/architecture/04-data-and-contracts.md 第 4 节 IPC 命令表。
 */
import type {
  Protocol,
  ApplyPlan,
  ApplyStage,
  CodexInstance,
  CoreError,
  Credential,
  DiagnosticEvent,
  Model,
  OperationEvent,
  ProbeResult,
  Provider,
} from '@/contracts/types';

import { t } from '@/i18n';

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
  /** `null` = 跟随供应商的协议。 */
  protocolOverride: Protocol | null;
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
  /** 是否已暂停接受新请求（在途请求不受影响）。 */
  paused: boolean;
  port: number | null;
  served: number;
  /** 已发布的目录版本；空表示尚未应用过任何配置。 */
  revisions: string[];
  tokenFingerprint: string;
  error: string | null;
}

/** 当前已生效的配置摘要。`defaultModel` 为空表示历史事务没有记录，不用当前表单值顶替。 */
/**
 * 一次重启的结果。两个字段都是**观察到**的结论，不是「命令发出去了」：
 * `quitConfirmed` 为 false 表示旧进程还在跑，本次没有重启；`launchedConfirmed`
 * 为 false 表示 Codex 已经退出但没有重新起来。界面按这两个值说准确的话。
 */
export interface HostRestart {
  appPath: string;
  quitConfirmed: boolean;
  /** 优雅退出没成、最后是发信号结束的；界面要提醒未保存内容可能丢失。 */
  quitForced: boolean;
  launchedConfirmed: boolean;
}

export interface AppliedSummary {
  operationId: string;
  instanceId: string;
  catalogRevision: string;
  defaultModel: string | null;
  aliasCount: number;
  /** 事务阶段。用契约类型而不是 string：写成 'Verified' 这种错值时编译期就能发现。 */
  stage: ApplyStage;
  appliedAt: string;
}

/** 一份配置备份。`mayContainSecrets` 为真时预览会被遮罩，且不要外发。 */
export interface BackupEntry {
  id: string;
  sourcePath: string;
  createdAt: string;
  contentHash: string;
  bytes: number;
  mayContainSecrets: boolean;
}

/** 更新检查结果。查询失败时 `error` 有值且 `latest` 为空，界面不得显示成“已是最新”。 */
export interface UpdateReport {
  current: string;
  latest: string | null;
  hasUpdate: boolean;
  releaseUrl: string | null;
  publishedAt: string | null;
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

  /** 当前已生效的配置；从未应用过时为 null。 */
  applySummary(): Promise<AppliedSummary | null>;
  /** 暂停或继续接受新推理请求；返回实际状态。 */
  setGatewayPaused(paused: boolean): Promise<boolean>;

  listBackups(): Promise<BackupEntry[]>;
  /** 手动备份一个实例的配置文件；提交前也会自动备份。 */
  createBackup(instanceId: string): Promise<BackupEntry>;
  /** 遮罩预览：原始内容不经过 IPC。 */
  previewBackup(backupId: string): Promise<string>;
  /** 恢复：会先把当前文件再备份一次，因此恢复本身可回退。 */
  restoreBackup(backupId: string): Promise<string>;
  /** 检查更新：只查询公开 Release，不下载不安装。 */
  checkUpdate(): Promise<UpdateReport>;

  /** 平台与窗口策略。界面据此设置 data-platform 与窗口相关变量。 */
  platformInfo(): Promise<PlatformReport>;

  listProviders(filter?: { query?: string }): Promise<ListResult<Provider>>;
  saveProvider(draft: ProviderDraft, expectedVersion: number): Promise<Provider>;

  listCredentials(providerId: string): Promise<Credential[]>;
  /** 秘密只在这一个调用里传入，不进入任何持久化状态。 */
  addCredential(providerId: string, label: string, secret: string): Promise<Credential>;
  replaceCredential(credentialId: string, secret: string, expectedVersion: number): Promise<Credential>;
  selectCredential(providerId: string, credentialId: string): Promise<void>;
  /** 改备注名。同一供应商下多个 Key 只有掩码尾号不同，名字是唯一的区分手段。 */
  renameCredential(credentialId: string, label: string, expectedVersion: number): Promise<Credential>;
  /** 停用 / 重新启用。停用当前正在用的那个会被拒绝——路由已经指向它。 */
  setCredentialDisabled(credentialId: string, disabled: boolean, expectedVersion: number): Promise<Credential>;

  discoverModels(providerId: string, credentialId: string): Promise<DiscoveredModel[]>;
  listModels(): Promise<Model[]>;
  saveModel(draft: ModelDraft, expectedVersion: number): Promise<Model>;
  /** 已纳入目录的模型必须先移出，删除会被拒绝。 */
  deleteModel(modelId: string, expectedVersion: number): Promise<void>;
  /** 正在使用的 Key 不能删除；删除会同时撤销系统凭据库里的条目。 */
  deleteCredential(credentialId: string): Promise<void>;
  /** 还有 Key 或模型时会拒绝，不做级联删除。 */
  deleteProvider(providerId: string): Promise<void>;

  /** 探测只读阶段默认不发真实请求；`includeGenerate` 才会产生费用与副作用。 */
  startProbe(target: { providerId: string; modelId?: string; credentialId: string }, options?: { includeGenerate?: boolean }): Promise<ProbeResult>;
  cancelProbe(probeId: string): Promise<void>;

  inspectConfig(instanceId: string): Promise<InspectResult>;
  planApply(request: PlanRequest): Promise<ApplyPlan>;
  executeApply(request: ExecutionRequest): Promise<{ operationId: string }>;
  applyStatus(operationId: string): Promise<ApplyStatus>;
  /** 只有用户确认宿主已重新加载，事务才从“等待重载”前进；不得由前端自行宣称已加载。 */
  confirmReload(operationId: string, loaded: boolean): Promise<ApplyStatus>;
  /**
   * 自动补记宿主回执：宿主进程在这次发布之后重新启动过，就认为它读过了新配置。
   *
   * 应用完成、窗口重新获得焦点、启动时各调一次。返回被补记的 operationId；空数组表示
   * 没有可确认的事务（宿主没重启过，或平台查不到启动时间），不是失败。
   */
  reconcileReload(): Promise<{ confirmedOperationIds: string[] }>;
  /**
   * 重启探测到的宿主实例（Codex / ChatGPT 桌面端）。
   *
   * Codex 只在启动时读 `config.toml`，写完配置必须重启它，模型才会出现在它的菜单里。
   * 返回的字段只说明「发出了什么命令」，不表示它已经加载了新配置。
   */
  restartHost(instanceId: string): Promise<HostRestart>;
  planRestore(instanceId: string): Promise<ApplyPlan>;
  executeRestore(request: ExecutionRequest): Promise<{ operationId: string }>;

  listDiagnostics(filter?: { level?: DiagnosticEvent['level'] }): Promise<ListResult<DiagnosticEvent>>;
  previewDiagnostics(request: DiagnosticsRequest): Promise<DiagnosticsPreview>;
  exportDiagnostics(request: DiagnosticsRequest): Promise<{ savedPath: string }>;
  /** 清空本工具自己的诊断事件；不影响 Codex 历史。返回清掉的条数。 */
  clearDiagnostics(): Promise<number>;
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
    safeDetails: [t('error.generic')],
    retryable: false,
    recoveryActions: [],
  };
}
