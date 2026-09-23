import { render, screen, within } from '@testing-library/react';
import { OverviewPage } from './OverviewPage';
import { provider } from '../../../tests/helpers/client';

const model = {
  id: 'm_1', providerId: 'p_test', upstreamId: 'vendor/a', catalogAlias: 'gs/m_1', displayName: '代码模型',
  lifecycle: 'saved' as const, hostState: 'pending_apply' as const, inCatalog: true,
  policy: { contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
    reasoning: { support: 'unknown' as const, control: 'none' as const, allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
    inputs: [], tools: { functionTools: 'unknown' as const, parallelTools: 'unknown' as const, customTools: 'unknown' as const, builtinTools: 'unknown' as const, verification: 'declared' as const } },
  displayNameLayer: { discovered: null, userValue: null, overridden: false },
  capabilityRevision: 1, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
};

const gateway = { running: true, paused: false, port: 18765, served: 3, revisions: ['rev_a'], tokenFingerprint: 'x', error: null, systemProxy: { httpEnabled: false, endpoint: null, bypassApplied: false } };

function renderOverview(overrides: Partial<Parameters<typeof OverviewPage>[0]> = {}) {
  return render(<OverviewPage
    providers={[provider]} models={[model]} credentialsByProvider={{ p_test: [] }}
    gateway={gateway} summary={null} pendingCount={1} awaitingHostOnly={false}
    onNavigate={() => {}} onAddProvider={() => {}} {...overrides} />);
}

test('没有已发布配置时说明尚未应用，不假装有默认路由', () => {
  renderOverview();

  expect(screen.getByText(/还没有已生效的配置/)).toBeInTheDocument();
  expect(screen.getByText('尚未应用')).toBeInTheDocument();
});

test('没有测试记录时“模型调用”显示未测试，而不是通过', () => {
  renderOverview();

  expect(screen.getByText('未测试')).toBeInTheDocument();
  expect(screen.queryByText('已通过')).not.toBeInTheDocument();
});

test('已发布配置显示默认路由与后续请求使用的 Key，并说明边界', () => {
  renderOverview({
    credentialsByProvider: { p_test: [{ id: 'k_1', providerId: 'p_test', label: '日常', secretRef: 'r', secretVersion: 1,
      maskedSuffix: '••••4f2a', status: 'verified', scope: null, lastVerifiedAt: new Date().toISOString(), version: 1, createdAt: '2026-09-18T00:00:00Z' }] },
    providers: [{ ...provider, activeCredentialId: 'k_1' }],
    summary: { operationId: 'op_1', instanceId: 'i', catalogRevision: 'rev_a', defaultModel: 'gs/m_1', aliasCount: 2, stage: 'verified', appliedAt: '2026-09-18T00:00:00Z' },
  });

  // 当前配置卡与待应用列表都会出现这个模型名，断言至少一次。
  expect(screen.getAllByText('代码模型').length).toBeGreaterThan(0);
  expect(screen.getByText(/日常 ••••4f2a/)).toBeInTheDocument();
  expect(screen.getByText(/不代表 Codex 每个会话正在使用的模型/)).toBeInTheDocument();
  // 同一句话也出现在顶部的接入进度里（那是同一事实的两处口径），所以限定到状态卡里断言。
  const statusCard = screen.getByRole('heading', { name: '连接状态' }).closest('section')!;
  expect(within(statusCard).getByText('已核验加载')).toBeInTheDocument();
});

test('网关未启动时状态卡显示原因，并提供诊断入口', () => {
  renderOverview({ gateway: { ...gateway, running: false, port: null, error: '端口 18765 被占用' } });

  expect(screen.getByText('未启动')).toBeInTheDocument();
  expect(screen.getByText('网关未启动')).toBeInTheDocument();
  expect(screen.getByRole('alert')).toHaveTextContent('端口 18765 被占用');
});

test('没有供应商时给一张添加引导，而不是空的卡片网格', () => {
  renderOverview({ providers: [], models: [], pendingCount: 0 });

  expect(screen.getByText('添加第一个供应商')).toBeInTheDocument();
  expect(screen.queryByText('当前配置')).not.toBeInTheDocument();
});

test('有未应用改动时常驻应用栏提示数量', () => {
  renderOverview({ pendingCount: 2 });

  const bar = screen.getByRole('region', { name: '待应用的修改' });
  expect(bar).toHaveTextContent('2 个模型待应用');
});

/**
 * 系统代理那一条要能回答「Codex 为什么报 502」，而且只能讲机制和我们做过的事：
 * 用户可能已经重启过 Codex，断言「你现在正失败」就成了假话。
 */
test('开着系统代理时状态卡说明回环被代理走，并区分绕过是否已生效', () => {
  const hijacked = { ...gateway, systemProxy: { httpEnabled: true, endpoint: '127.0.0.1:7890', bypassApplied: true } };
  renderOverview({ gateway: { ...hijacked, systemProxy: { ...hijacked.systemProxy, bypassApplied: false } } });

  const statusCard = screen.getByRole('heading', { name: '连接状态' }).closest('section')!;
  expect(within(statusCard).getByText('系统代理')).toBeInTheDocument();
  expect(within(statusCard).getByText(/127\.0\.0\.1:7890 会连回环地址一起代理/)).toBeInTheDocument();
  // 绕过还没写进会话时说清楚要靠本工具重启，不冒充已经解决。
  expect(within(statusCard).getByText(/用本工具重启 Codex 会带上绕过/)).toBeInTheDocument();
});

test('没有系统代理时状态卡只说没开启，不渲染绕过的话术', () => {
  renderOverview();

  const statusCard = screen.getByRole('heading', { name: '连接状态' }).closest('section')!;
  expect(within(statusCard).getByText('未开启，本机网关直连。')).toBeInTheDocument();
  expect(within(statusCard).queryByText(/会连回环地址一起代理/)).not.toBeInTheDocument();
});

/** 观察结果还没回来（网关报告为 null）时那一条不出现：不知道就不说。 */
test('网关报告缺失时不显示系统代理那一条', () => {
  renderOverview({ gateway: null });

  expect(screen.queryByText('系统代理')).not.toBeInTheDocument();
});
