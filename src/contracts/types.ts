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
  /** 上游自己执行的服务端内置工具（`web_search` 等）。未声明即不转发给上游。 */
  builtinTools: Support;
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

/* ---- 工具管理 ---- */

export type ToolCategory = 'cliCode' | 'utility' | 'runtime';

/**
 * 工具状态。全部来自一次真实探测，没有推断值：
 * `unverified` 是「找到了文件但探针没通过」，它需要人看一眼，**不等于**已就绪。
 */
export type ToolStatus = 'ready' | 'needsLogin' | 'installed' | 'unverified' | 'notInstalled' | 'unsupportedPlatform';

export type ToolPathSource = 'path' | 'candidate';

export interface InstalledTool {
  path: string;
  pathSource: ToolPathSource;
  version?: string | null;
  configPath?: string | null;
  configExists: boolean;
  skillsPath?: string | null;
  skillsCount: number;
}

export interface ToolAgentUsage {
  tags: string[];
  nonInteractive: string[];
}

export interface ToolState {
  id: string;
  displayName: string;
  category: ToolCategory;
  description: string;
  status: ToolStatus;
  installed?: InstalledTool | null;
  website?: string | null;
  docs?: string | null;
  modelConfig: boolean;
  skillTarget: boolean;
  agentUsage: ToolAgentUsage;
  versionProbeTail: string;
  /** `null` = 没做过登录判定；空字符串 = 查了但没输出。两者含义不同。 */
  authProbeTail?: string | null;
  notes: string[];
  probedAt: number;
  cacheSeconds: number;
}

export interface SkillTarget {
  toolId: string;
  displayName: string;
  root: string;
}

/* ---- 插件中心 ---- */

export interface PluginSource {
  repo: string;
  label: string;
  description: string;
  builtin: boolean;
}

export interface SkillDocument {
  id: string;
  title?: string | null;
  description?: string | null;
  requiresBins: string[];
  body: string;
  frontMatterParsed: boolean;
}

export interface RepoFile {
  path: string;
  bytes: number;
  text: string;
}

export interface RepoSkill {
  dirName: string;
  sourcePath: string;
  document: SkillDocument;
  files: RepoFile[];
}

export interface RepoCatalog {
  repo: string;
  commit: string;
  skills: RepoSkill[];
  fetchedAt: number;
  truncated: boolean;
}

export interface FileFingerprint {
  path: string;
  sha256: string;
  bytes: number;
}

/** `create` 新建；`update` 更新我们自己的安装；`conflict` 目标目录不是我们装的。 */
export type PlannedAction = 'create' | 'update' | 'conflict';

export interface PlannedFile {
  path: string;
  bytes: number;
  sha256: string;
}

export interface TargetPlan {
  toolId: string;
  displayName: string;
  root: string;
  dir: string;
  dirName: string;
  action: PlannedAction;
  files: PlannedFile[];
  conflictDetail?: string | null;
  foreignFiles: string[];
}

export interface SkillPlan {
  skillId: string;
  dirName: string;
  sourcePath: string;
  description?: string | null;
  requiresBins: string[];
  targets: TargetPlan[];
}

export interface InstallPreview {
  repo: string;
  commit: string;
  skills: SkillPlan[];
}

/** 冲突处置只有这两个：**没有覆盖**，因为目标目录里的东西不是我们的。 */
export type ConflictChoice = 'skip' | 'keepBoth';

export interface InstallRequest {
  repo: string;
  gitRef?: string | null;
  skillDirs: string[];
  targets: string[];
  conflictChoices: Record<string, ConflictChoice>;
}

export interface SkillRecord {
  skillId: string;
  dirName: string;
  targetTool: string;
  targetDisplayName: string;
  sourceRepo: string;
  sourceCommit: string;
  sourcePath: string;
  installedPath: string;
  enabled: boolean;
  installedAt: number;
  files: FileFingerprint[];
}

export interface SkippedTarget {
  toolId: string;
  dirName: string;
  reason: string;
}

export interface FailedTarget {
  toolId: string;
  dirName: string;
  message: string;
}

/** 部分完成是常态，所以三个列表一起返回。 */
export interface InstallReport {
  repo: string;
  commit: string;
  installed: SkillRecord[];
  skipped: SkippedTarget[];
  failed: FailedTarget[];
}

export interface UpdateInfo {
  skillId: string;
  targetTool: string;
  currentCommit: string;
  latestCommit: string;
}

export interface UninstallOutcome {
  dir: string;
  removedFiles: string[];
  keptModified: string[];
  missingFiles: string[];
  foreignFiles: string[];
  removedDir: boolean;
}

/* ---- 内容中心 ---- */

export type FeedKind = 'rss' | 'githubSearch';

export interface FeedSource {
  id: string;
  kind: FeedKind;
  url: string;
  label: string;
  lang: string;
  enabled: boolean;
  etag?: string | null;
  lastModified?: string | null;
  lastOkAt?: number | null;
  lastError?: string | null;
  failStreak: number;
  nextFetchAt: number;
  builtin: boolean;
}

export interface FeedSourceDraft {
  id?: string | null;
  kind: FeedKind;
  url: string;
  label: string;
  lang: string;
  enabled: boolean;
}

export interface FeedItem {
  url: string;
  sourceId: string;
  sourceLabel: string;
  title: string;
  summary: string;
  publishedAt: number;
  firstSeenAt: number;
  lang: string;
  stars?: number | null;
  repo?: string | null;
}

export interface FeedFailure {
  sourceId: string;
  label: string;
  message: string;
  failStreak: number;
}

export interface RefreshReport {
  attempted: string[];
  succeeded: string[];
  notModified: string[];
  failed: FeedFailure[];
  skipped: string[];
  newItems: number;
  nextFetchAt: number;
}

export interface ContentStatus {
  lastOkAt?: number | null;
  nextFetchAt: number;
  /** 每天自动抓取的本地小时数（升序），例如 [6, 18]。时刻表由核心持有，界面照它显示。 */
  scheduleHours: number[];
  failing: FeedFailure[];
  totalItems: number;
}

/**
 * 用量统计（用量页）。全部来自 Codex 本地会话记录，不联网、不新增采集。
 *
 * 口径提醒：`cachedTokens ⊂ inputTokens`、`reasoningTokens ⊂ outputTokens`，所以四个
 * 指标之间**不可相加**当作总量，界面必须说明这种包含关系。
 *
 * 注意 `totalTokens` 也不等于 `inputTokens + outputTokens`：本机 32366 个 token_count
 * 事件里有 2878 个（约 9%）两者不等，差异多为 ±1~7（上游自身的记账口径所致）。
 * 所以不要在任何地方假设这几个字段可以互相推算。
 */
export interface UsageTotals {
  inputTokens: number;
  /** 输入中被缓存命中的部分（含在 inputTokens 内）。 */
  cachedTokens: number;
  cacheWriteTokens: number;
  outputTokens: number;
  /** 输出中的推理部分（含在 outputTokens 内）。 */
  reasoningTokens: number;
  totalTokens: number;
}

export interface UsageDay {
  /** 本地日期，`YYYY-MM-DD`。 */
  date: string;
  sessions: number;
  totals: UsageTotals;
}

export interface UsageModelRow {
  model: string;
  sessions: number;
  totals: UsageTotals;
}

export interface UsageProviderRow {
  provider: string;
  sessions: number;
  totals: UsageTotals;
}

/** 计划额度窗口。只有官方计划账号的 Codex 会话会写入，第三方供应商的会话没有。 */
export interface UsagePlanWindow {
  planType: string;
  usedPercent: number;
  windowMinutes: number;
  /** Unix 秒。 */
  resetsAt: number;
}

export interface UsageReport {
  /** 实际扫描的根目录，界面上如实显示，便于核对数字来源。 */
  sourceDirectory: string;
  /** 本次请求的时间范围（天）。 */
  rangeDays: number;
  scannedFiles: number;
  unreadableFiles: number;
  sessions: number;
  totals: UsageTotals;
  /** 范围内逐日零填充，按日期升序。 */
  daily: UsageDay[];
  /** 按总 Token 降序。 */
  byModel: UsageModelRow[];
  byProvider: UsageProviderRow[];
  planWindow: UsagePlanWindow | null;
}
