/**
 * 文案单一入口。所有 UI 文案走 locale key，Rust 错误不拼最终中文。
 * 措辞遵循 docs/design/02-icons-type-and-copy.md 第 4 节的标准文案表。
 */
export const zhCN = {
  app: {
    name: 'GPTSwitch',
    subtitle: 'Codex 配置管理',
  },
  nav: {
    overview: '概览',
    providers: '供应商',
    models: '模型',
    codexConfig: 'Codex 配置',
    diagnostics: '连接诊断',
    logs: '日志',
    settings: '设置',
  },
  mode: {
    label: '模式',
    /** 三种模式状态始终可见，不要求用户理解内部 Bridge。 */
    native: '原生模式',
    thirdParty: '第三方模式',
    awaitingReload: '等待加载',
  },
  service: {
    running: '服务运行中',
    stopped: '服务未运行',
    starting: '正在启动',
  },
  /** 标准文案：保存 / 测试 / 应用语义不合并。 */
  copy: {
    draftSaved: '已保存，尚未应用到 Codex。',
    keySwitched: '后续新请求使用「工作 Key」。进行中的请求保持原配置。',
    catalogCommitted: '模型目录已更新。Codex 重新加载后可见。',
    hostUnobservable: '配置已提交，尚未确认当前 Codex 窗口已加载。',
    /** 只有用户确认宿主已重新加载后才使用。 */
    hostLoaded: 'Codex 已重新加载并核验本次目录。',
    fullSuccess: 'Codex 已加载 3 个模型。最近一次路由验证通过。',
    textOnlyPassed: '文本请求通过。工具调用和图片输入尚未验证。',
    discoveryUnsupported: '此接口未提供模型列表。可手动填写模型 ID。',
    capabilityUnknown: '未获取到该能力信息。可填写服务商文档中的值。',
    outputCapped: '本次输出上限为 8,192 Token，受模型策略限制。',
    fileCannotBeSent: '当前 Codex 接入方式不能原生发送 PDF。',
    urlChanged: '服务地址已更改，需要重新测试模型权限。',
    conflict: '配置在预览后被修改。重新比较后再应用。',
    applyFailedRestored: '应用未完成，已恢复上次配置。',
    restoreConflict: '检测到外部修改，已保留当前文件。请选择要恢复的字段。',
    quitDependency: '退出后，使用本地网关的第三方模型将暂时不可用。',
    notInCatalog: '未选择用于 Codex',
  },
  action: {
    addProvider: '添加供应商',
    addModel: '添加模型',
    addKey: '添加 Key',
    testConnection: '测试连接',
    testModel: '测试模型',
    applyToCodex: '应用到 Codex',
    applyAndReload: '应用并重新加载',
    reload: '重新加载',
    restorePrevious: '恢复上次配置',
    openCodex: '打开 Codex',
    viewModels: '查看模型',
    viewDiff: '查看差异',
    manageAll: '管理全部',
    save: '保存',
    saveDraft: '保存草稿',
    saveAndViewDiff: '保存并查看应用差异',
    cancel: '取消',
    delete: '删除',
    copy: '复制',
    copyModelId: '复制模型 ID',
    reveal: '打开所在位置',
    retry: '重试',
    gotIt: '知道了',
    startService: '启动服务',
    unlockThenRetry: '解锁后重试',
    reenterSecret: '重新填写',
    recompare: '重新比较',
    openConfigCopy: '定位错误行、打开副本',
    laterReload: '保存并稍后重新加载',
  },
  host: {
    notInCatalog: '未选择用于 Codex',
    pendingApply: '待应用',
    awaitingReload: '已提交，等待重载',
    loaded: '已加载',
    loadUnconfirmed: '加载未确认',
  },
  stage: {
    draft: '草稿',
    validating: '校验',
    blocked: '已阻塞',
    prepared: '已准备',
    committing: '提交中',
    awaitingReload: '等待 Codex 重新加载',
    verified: '已核验',
    pending: '稍后重新加载',
    rollingBack: '正在回滚',
    restored: '已恢复上次配置',
    conflict: '存在冲突',
    failed: '失败',
  },
  /** 差异分组：只对 `reasonKey` 做展示归类，判定仍在核心。 */
  group: {
    route: '供应商路由',
    catalog: '模型目录',
    policy: '请求策略',
    restore: '恢复基线',
    other: '其他字段',
  },
  reason: {
    defaultModel: '应用后作为默认模型',
    providerRoute: '指向本机网关 provider',
    catalog: '指向编译后的目录文件',
    contextOverride: '全局上下文覆盖',
    reasoningDefault: '默认思考档位',
    gatewayProvider: '本机网关 provider 定义',
    restore: '恢复基线值',
    other: '受管字段变更',
  },
  /** 目录编译警告。核心拼成 `warning.xxx：详情`，界面只显示这里的可读文案。 */
  warning: {
    contextUnknown: '上下文未声明',
    reasoningDefaultNotProjected: '默认思考档位未进入菜单',
    reasoningNotSelectableInHost: '思考档位在 Codex 中不可切换',
    other: '目录提示',
  },
  credential: {
    saved: '已保存，未检测',
    verified: '已验证',
    authFailed: '认证失败',
    scopeLimited: '权限待判定',
    keystoreLocked: '系统凭据库暂不可用',
    missing: '安全记录不存在',
    disabled: '已停用',
  },
  compat: {
    unverified: '未验证',
    experimental: '实验',
    stable: '稳定',
    unsupported: '不支持',
  },
  protocol: {
    responses: 'Responses',
    chatCompletions: 'Chat Completions',
  },
  auth: {
    apiKey: 'API Key',
    localNoAuth: '本地无认证',
  },
  source: {
    provider: '服务商返回',
    officialDocs: '官方文档',
    registry: '注册表',
    user: '用户填写',
    probe: '测试确认',
  },
  capability: {
    text: '文本',
    image: '图片',
    audio: '音频',
    video: '视频',
    pdf: 'PDF',
    document: '其他文件',
  },
  error: {
    validation: '填写内容不符合要求',
    notFound: '未找到目标',
    internal: '内部错误',
    configChanged: '配置已被其他程序修改',
    configParseFailed: '配置文件无法解析',
    keystoreLocked: '系统凭据库暂不可用',
    credentialMissing: '此 Key 的安全记录不存在',
    modelPermissionDenied: '当前 Key 没有该模型权限',
    capabilityUnsupported: '当前接入方式不支持此能力',
    catalogSchemaMismatch: '目录与当前 Codex 版本不匹配',
    desktopReloadRequired: '配置已提交，等待 Codex 重新加载',
    routeMismatch: '实际请求路由与选择不一致',
    continuationBound: '此续接仍绑定原凭据',
    portInUse: '本地服务端口被占用',
    unauthorized: '本机网关拒绝了该请求',
    requestTooLarge: '请求体超过运输保护上限',
    unknownCatalogRevision: '该目录版本未发布',
    unknownAlias: '该目录版本不包含此模型',
    instanceMismatch: '令牌与请求目标实例不一致',
  },
  empty: {
    noInstanceTitle: '未检测到 Codex',
    noInstanceBody: '请先安装受支持的 Codex，或手动指定应用与配置目录。',
    noProviderTitle: '还没有供应商',
    noProviderBody: '添加供应商和模型，测试并应用后，再确认 Codex 模型选择器已加载。',
    noModelTitle: '还没有模型',
    noModelBody: '可以从 /models 获取，也可以直接手动填写模型 ID。',
    noKeyTitle: '还没有 API Key',
    noKeyBody: '添加 Key 后即可测试连接。没有 Key 时不会反复自动探测。',
    noLogTitle: '还没有日志',
    noLogBody: '执行测试或应用后，这里会显示脱敏后的诊断事件。',
  },
} as const;

export type Locale = typeof zhCN;

/** 取文案；缺失时回落到 key 本身，避免界面出现空白。 */
export function t(path: string, locale: Locale = zhCN): string {
  const segments = path.split('.');
  let current: unknown = locale;
  for (const segment of segments) {
    if (typeof current !== 'object' || current === null) return path;
    current = (current as Record<string, unknown>)[segment];
  }
  return typeof current === 'string' ? current : path;
}
