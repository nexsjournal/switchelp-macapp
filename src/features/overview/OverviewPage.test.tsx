import { render, screen, within } from '@testing-library/react';
import { OverviewPage } from './OverviewPage';
import { provider } from '../../../tests/helpers/client';

const model = {
  id: 'm_1', providerId: 'p_test', upstreamId: 'vendor/a', catalogAlias: 'gs/m_1', displayName: '代码模型',
  lifecycle: 'saved' as const, hostState: 'pending_apply' as const, inCatalog: true,
  policy: { contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
    reasoning: { support: 'unknown' as const, control: 'none' as const, allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
    inputs: [], tools: { functionTools: 'unknown' as const, parallelTools: 'unknown' as const, customTools: 'unknown' as const, verification: 'declared' as const } },
  displayNameLayer: { discovered: null, userValue: null, overridden: false },
  capabilityRevision: 1, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
};

const gateway = { running: true, paused: false, port: 18765, served: 3, revisions: ['rev_a'], tokenFingerprint: 'x', error: null };

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
