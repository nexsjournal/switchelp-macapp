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
  ContentStatus,
  CoreError,
  Credential,
  DiagnosticEvent,
  FeedItem,
  FeedSource,
  FeedSourceDraft,
  InstallPreview,
  InstallReport,
  InstallRequest,
  Model,
  OperationEvent,
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

/**
 * 系统代理与回环地址的关系。
 *
 * 宿主到本机网关是 `http://127.0.0.1:<端口>`，而宿主自己的 HTTP 客户端会把系统代理
 * 套到这条请求上，代理又到不了用户的回环地址——用户看到的就是
 * 「502 Bad Gateway: Unknown error」。这条报告是界面唯一能说清这件事的地方。
 */
export interface SystemProxyReport {
  /** 系统里有没有开 HTTP 代理。 */
  httpEnabled: boolean;
  /** 代理地址，形如 `127.0.0.1:7890`；读不到时为 null。 */
  endpoint: string | null;
  /** 「绕过回环」是否已经在生效（已写进登录会话，之后启动的 Codex 不再被拦）。 */
  bypassApplied: boolean;
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
  systemProxy: SystemProxyReport;
}

/** 当前已生效的配置摘要。`defaultModel` 为空表示历史事务没有记录，不用当前表单值顶替。 */
/**
 * 一次重启的结果。两个字段都是**观察到**的结论，不是「命令发出去了」：
 * `quitConfirmed` 为 false 表示旧进程还在跑，本次没有重启；`launchedConfirmed`
 * 为 false 表示 Codex 已经退出但没有重新起来。界面按这两个值说准确的话。
 */
export interface CoexistState {
  enabled: boolean;
  /** bridge 有没有装好；没装好时 `bridgeDetail` 说明原因。 */
  bridgeReady: boolean;
  bridgeDetail: string | null;
  bridgePath: string | null;
  managedHome: string;
  managedConfigExists: boolean;
  /** 宿主此刻是否跑在 bridge 上；null = 无法确认。 */
  hostUnderBridge: boolean | null;
  /** 这个实例具不具备接管前提（有 CLI、没有硬阻塞）。 */
  ready: boolean;
  blockedReason: string | null;
}

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
  /** 新版本的发布说明（Markdown）。可能缺省。 */
  notes: string | null;
  releaseUrl: string | null;
  publishedAt: string | null;
  error: string | null;
}

/** 更新包下载进度。`total` 为空表示上游没给长度，进度条退化成不确定态。 */
export interface UpdateProgress {
  phase: 'download' | 'install';
  downloaded: number;
  total: number | null;
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
  /** 检查更新：只读一次更新源，不下载不安装。 */
  checkUpdate(): Promise<UpdateReport>;
  /**
   * 下载并安装更新。**成功时不会返回**：装完应用立刻重启，窗口随之消失。
   * 所以调用方只需要处理失败；进度通过 `onUpdateProgress` 订阅。
   */
  installUpdate(): Promise<void>;
  /** 订阅下载进度，返回取消订阅函数。 */
  onUpdateProgress(listener: (progress: UpdateProgress) => void): Promise<() => void>;
  /** 取出「刚更新完」的版本号（只出现一次），没有则 null。 */
  takeUpdateResult(): Promise<string | null>;
  /** 用系统浏览器打开发布页。仅允许本仓库的地址。 */
  openReleasePage(url: string): Promise<void>;

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

  /**
   * 共存模式（Bridge）的状态。
   *
   * `enabled` 是意图，`hostUnderBridge` 是事实：宿主此刻真的有没有跑在 bridge 上。
   * 后者为 `null` 表示无法确认（拿不到进程启动时间或日志），界面必须如实这么说。
   */
  coexistStatus(instanceId: string): Promise<CoexistState>;
  /** 开/关共存模式。打开时核心会先确认原生配置干净、bridge 已装好，任何一条不成就整体失败。 */
  setCoexist(instanceId: string, enabled: boolean): Promise<CoexistState>;
  /**
   * 用当前的原生配置重建托管 profile 的底子（除路由外的设置）。
   *
   * 底子是开启共存时复制的那一份，之后原生配置的改动不会自动同步；这里是那个出口。
   */
  resyncCoexist(instanceId: string): Promise<CoexistState>;

  listDiagnostics(filter?: { level?: DiagnosticEvent['level'] }): Promise<ListResult<DiagnosticEvent>>;
  previewDiagnostics(request: DiagnosticsRequest): Promise<DiagnosticsPreview>;
  exportDiagnostics(request: DiagnosticsRequest): Promise<{ savedPath: string }>;
  /** 清空本工具自己的诊断事件；不影响 Codex 历史。返回清掉的条数。 */
  clearDiagnostics(): Promise<number>;
  /* ---- 工具管理 ---- */

  /**
   * 工具清单与状态。`refresh = false` 时复用未过期的探测结论，因此普通页面加载
   * 不会每次都拉起十几个子进程。
   */
  listTools(options?: { refresh?: boolean; locale?: string }): Promise<ToolState[]>;
  /** 强制重探一个工具。 */
  probeTool(toolId: string, locale?: string): Promise<ToolState>;
  /** 支持安装技能的目标工具及其技能根目录。 */
  listSkillTargets(): Promise<SkillTarget[]>;

  /* ---- 插件中心 ---- */

  listPluginSources(): Promise<PluginSource[]>;
  addPluginSource(repo: string): Promise<PluginSource[]>;
  removePluginSource(repo: string): Promise<PluginSource[]>;
  /** 浏览一个来源的技能目录。这一步会联网，失败原因原样返回。 */
  browsePluginRepo(repo: string): Promise<RepoCatalog>;
  /** 生成安装计划。**不写文件**，界面据此展示将写入什么、哪里会冲突。 */
  previewPluginInstall(request: InstallRequest): Promise<InstallPreview>;
  installPlugin(request: InstallRequest): Promise<InstallReport>;
  listInstalledSkills(): Promise<SkillRecord[]>;
  checkSkillUpdates(): Promise<UpdateInfo[]>;
  /** 启用 / 禁用。实现是给技能目录改名，界面必须说明这一点。 */
  setSkillEnabled(skillId: string, targetTool: string, enabled: boolean): Promise<SkillRecord>;
  /** 卸载。改过的文件会保留并如实返回。 */
  uninstallSkill(skillId: string, targets: string[]): Promise<UninstallOutcome[]>;

  /* ---- 内容中心 ---- */

  listFeedSources(): Promise<FeedSource[]>;
  saveFeedSource(draft: FeedSourceDraft): Promise<FeedSource>;
  deleteFeedSource(sourceId: string): Promise<void>;
  listFeedItems(filter: { sourceId?: string; lang?: string; limit?: number; offset?: number }): Promise<FeedItem[]>;
  /** 抓取。指定 `sourceId` 只刷那一个；`force` 忽略到期时间。 */
  refreshContent(options?: { sourceId?: string; force?: boolean }): Promise<RefreshReport>;
  contentStatus(): Promise<ContentStatus>;
  /** 返回实际生效的间隔（已夹到允许区间内）。 */
  /** GitHub 令牌是否已配置。**不返回令牌本身**。 */
  contentGithubTokenStatus(): Promise<boolean>;
  setContentGithubToken(token: string | null): Promise<boolean>;

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
