/**
 * IPC 契约 DTO。字段命名与 switch-core 的 serde camelCase 输出一致，
 * 单一真相源是 Rust 侧类型（见 docs/architecture/04-data-and-contracts.md）。
 */

export type ProviderId = string;
export type CredentialId = string;
export type ModelId = string;
export type InstanceId = string;
export type RevisionId = string;
export type OperationId = string;
export type PlanId = string;
export type ProbeId = string;

/* ---- 错误契约 ---- */

export type ErrorCode =
  | 'CONFIG_CHANGED'
  | 'CONFIG_PARSE_FAILED'
  | 'KEYSTORE_LOCKED'
  | 'CREDENTIAL_MISSING'
  | 'MODEL_PERMISSION_DENIED'
  | 'CAPABILITY_UNSUPPORTED'
  | 'CATALOG_SCHEMA_MISMATCH'
  | 'DESKTOP_RELOAD_REQUIRED'
  | 'ROUTE_MISMATCH'
  | 'CONTINUATION_BOUND'
  | 'PORT_IN_USE'
  | 'UNAUTHORIZED'
  | 'REQUEST_TOO_LARGE'
  | 'VALIDATION_FAILED'
  | 'NOT_FOUND'
  | 'CONFLICT'
  | 'INTERNAL';

export interface RecoveryAction {
  action: string;
  messageKey: string;
}

export interface CoreError {
  code: ErrorCode;
  messageKey: string;
  safeDetails: string[];
  retryable: boolean;
  recoveryActions: RecoveryAction[];
  operationId?: string | null;
}

/* ---- 供应商 ---- */

export type Protocol = 'responses' | 'chat_completions';
export type AuthKind = 'api_key' | 'none';

export interface Provider {
  id: ProviderId;
  name: string;
  endpoint: string;
  protocol: Protocol;
  authKind: AuthKind;
  presetId?: string | null;
  activeCredentialId?: CredentialId | null;
  enabled: boolean;
  notes?: string | null;
  version: number;
  createdAt: string;
  updatedAt: string;
}

export interface ProviderPreset {
  id: string;
  name: string;
  baseUrl: string;
  protocol: Protocol;
  notesKey?: string | null;
}

/* ---- 凭据 ---- */

export type CredentialStatus =
  | 'saved'
  | 'verified'
  | 'auth_failed'
  | 'scope_limited'
  | 'keystore_locked'
  | 'missing'
  | 'disabled';

export interface Credential {
  id: CredentialId;
  providerId: ProviderId;
  label: string;
  secretRef: string;
  secretVersion: number;
  maskedSuffix: string;
  status: CredentialStatus;
  scope?: string | null;
  lastVerifiedAt?: string | null;
  version: number;
  createdAt: string;
}

/* ---- 能力 ---- */

export type Support = 'supported' | 'unsupported' | 'unknown';
export type EvidenceSource = 'provider' | 'official_docs' | 'registry' | 'user' | 'probe';
export type Verification = 'declared' | 'verified' | 'failed' | 'stale';
export type InputKind = 'text' | 'image' | 'audio' | 'video' | 'pdf' | 'document';
export type InputPath = 'native' | 'converted' | 'tool_read' | 'blocked';

export interface CapabilityValue<T> {
  value: T | null;
  source: EvidenceSource;
  sourceRef?: string | null;
  observedAt: string;
  verification: Verification;
}

export interface InputCapability {
  kind: InputKind;
  upstream: Support;
  gateway: Support;
  host: Support;
  effectivePath: InputPath;
  mimeTypes: string[];
  maxBytes?: number | null;
  conversionId?: string | null;
  verification: Verification;
  blockedReasonKey?: string | null;
}

export interface ToolCapability {
  functionTools: Support;
  parallelTools: Support;
  customTools: Support;
  verification: Verification;
}

/* ---- 思考 ---- */

export type ReasoningControl = 'effort' | 'toggle' | 'budget' | 'none';

export interface ReasoningPolicy {
  support: Support;
  control: ReasoningControl;
  allowedValues: string[];
  defaultValue: string | null;
  budgetTokens?: number | null;
  mappingId?: string | null;
}

/* ---- 模型 ---- */

export type ModelLifecycle = 'draft' | 'saved' | 'disabled';
export type HostState =
  | 'not_in_catalog'
  | 'pending_apply'
  | 'awaiting_reload'
  | 'loaded'
  | 'load_unconfirmed';

export interface LayeredField<T> {
  discovered: T | null;
  userValue: T | null;
  overridden: boolean;
}

export interface ModelPolicy {
  contextLimit: number | null;
  outputLimit: number | null;
  compactLimit: number | null;
  reasoning: ReasoningPolicy;
  inputs: InputCapability[];
  tools: ToolCapability;
}

export interface Model {
  id: ModelId;
  providerId: ProviderId;
  upstreamId: string;
  catalogAlias: string;
  displayName: string;
  protocolOverride?: Protocol | null;
  lifecycle: ModelLifecycle;
  hostState: HostState;
  inCatalog: boolean;
  policy: ModelPolicy;
  displayNameLayer: LayeredField<string>;
  capabilityRevision: number;
  version: number;
  createdAt: string;
  updatedAt: string;
}

/* ---- 应用事务 ---- */

export type ApplyStage =
  | 'draft'
  | 'validating'
  | 'blocked'
  | 'prepared'
  | 'committing'
  | 'awaiting_reload'
  | 'verified'
  | 'pending'
  | 'rolling_back'
  | 'restored'
  | 'conflict'
  | 'failed';

export type ReloadScope = 'none' | 'next_task_only' | 'host_reload';

export interface FieldChange {
  keyPath: string;
  before: string | null;
  after: string | null;
  reasonKey: string;
}

export interface DiffGroup {
  groupKey: string;
  changes: FieldChange[];
}

export interface ApplyPlan {
  id: PlanId;
  instanceId: InstanceId;
  revisionId: RevisionId;
  planHash: string;
  expectedConfigHash: string;
  /** 计划生成时配置文件是否存在；缺失与出现都属于身份变化。 */
  expectedConfigExists: boolean;
  configPath: string;
  createdAtUnix: number;
  ttlSecs: number;
  changes: FieldChange[];
  reloadScope: ReloadScope;
  catalogRevision: string;
  catalogAliases: string[];
  warnings: string[];
  touchesActiveTasks: boolean;
}

export interface OperationEvent {
  schemaVersion: number;
  operationId: string;
  sequence: number;
  phase: ApplyStage;
  revisionId?: string | null;
  messageKey: string;
  safeArgs: Record<string, string>;
  cancellable: boolean;
  timestamp: string;
}

/* ---- 实例检测 ---- */

export type StartupMode = 'managed' | 'unmanaged' | 'not_running';
export type CompatibilityStatus = 'unverified' | 'experimental' | 'stable' | 'unsupported';

export interface VersionFingerprint {
  desktopVersion?: string | null;
  cliVersion?: string | null;
  schemaHash?: string | null;
}

export interface CodexInstance {
  id: InstanceId;
  appPath?: string | null;
  cliPath?: string | null;
  desktopVersion?: string | null;
  cliVersion?: string | null;
  configRoot: string;
  configFile: string;
  configExists: boolean;
  startupMode: StartupMode;
  compatibility: CompatibilityStatus;
  fingerprint: VersionFingerprint;
  conflictingManagers: string[];
  blockedReasonKey?: string | null;
}

/* ---- 探测结果 ---- */

export interface ProbeStageResult {
  stageKey: string;
  status: 'passed' | 'failed' | 'skipped' | 'running';
  elapsedMs?: number | null;
  messageKey: string;
  errorCode?: ErrorCode | null;
}

export interface ProbeResult {
  id: ProbeId;
  targetLabel: string;
  stages: ProbeStageResult[];
  startedAt: string;
  expiresAt?: string | null;
  /** 用户是否中途取消；被取消时剩余阶段标记为 skipped。 */
  cancelled?: boolean;
  /** 是否真的向供应商发起过生成请求（有费用）。 */
  generated?: boolean;
}

/* ---- 日志 ---- */

export type LogLevel = 'info' | 'warning' | 'error';

export interface DiagnosticEvent {
  timestamp: string;
  level: LogLevel;
  categoryKey: string;
  targetLabel: string;
  resultKey: string;
  elapsedMs?: number | null;
  safeMetadata: Record<string, string>;
}
