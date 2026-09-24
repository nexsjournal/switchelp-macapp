import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { ToolState } from '@/contracts/types';
import { ToolsPage } from './ToolsPage';
import { categoryLabel, catalogLocale, describeProbedAt, statusBadge } from './toolsPolicy';
import { testClient } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

function tool(overrides: Partial<ToolState> & { id: string }): ToolState {
  return {
    displayName: overrides.id,
    category: 'cliCode',
    description: '描述',
    status: 'ready',
    installed: null,
    website: null,
    docs: null,
    modelConfig: true,
    skillTarget: false,
    agentUsage: { tags: [], nonInteractive: [] },
    versionProbeTail: '',
    authProbeTail: null,
    notes: [],
    probedAt: Math.floor(Date.now() / 1000),
    cacheSeconds: 300,
    ...overrides,
  };
}

const ready = tool({
  id: 'codex',
  displayName: 'Codex CLI',
  status: 'ready',
  installed: {
    path: '/opt/homebrew/bin/codex', pathSource: 'path', version: 'codex-cli 0.154.0',
    configPath: '/Users/me/.codex/config.toml', configExists: true,
    skillsPath: '/Users/me/.codex/skills', skillsCount: 3,
  },
  agentUsage: { tags: ['编码 agent'], nonInteractive: ['codex exec "<任务>"'] },
  versionProbeTail: 'codex-cli 0.154.0',
});

const unverified = tool({
  id: 'aider',
  displayName: 'Aider',
  status: 'unverified',
  installed: {
    path: '/Users/me/.local/bin/aider', pathSource: 'candidate', version: null,
    configPath: null, configExists: false, skillsPath: null, skillsCount: 0,
  },
  versionProbeTail: 'aider: command timed out after 10s',
  notes: ['版本探针超时，已终止该进程'],
});

const missing = tool({ id: 'ffmpeg', displayName: 'FFmpeg', status: 'notInstalled', category: 'utility', modelConfig: false });

describe('工具状态映射', () => {
  it('未验证不是绿色——它需要人看一眼', () => {
    expect(statusBadge('ready').tone).toBe('success');
    expect(statusBadge('unverified').tone).not.toBe('success');
    expect(statusBadge('unverified').labelKey).toBe('tools.status.unverified');
    // 未知取值一律按未验证处理，绝不默认成已就绪。
    expect(statusBadge('something-new' as never).tone).toBe('warning');
  });

  it('未授权是琥珀而不是错误色：它是可操作的提示', () => {
    expect(statusBadge('needsLogin').tone).toBe('warning');
  });

  it('界面语言到清单语言只有一处转换', () => {
    expect(catalogLocale('zh-CN')).toBe('zh-Hans');
    expect(catalogLocale('en')).toBe('en');
  });

  it('分类标签来自枚举，未知取值原样暴露以便发现', () => {
    expect(categoryLabel('cliCode')).toBe('tools.category.cliCode');
    expect(categoryLabel('nope' as never)).toBe('nope');
  });

  it('探测时间说成人话', () => {
    const now = 1_000_000;
    expect(describeProbedAt(now - 30, now, 'zh-CN')).toContain('秒');
    expect(describeProbedAt(now - 7200, now, 'zh-CN')).toContain('小时');
  });
});

it('列出工具，并让每个状态对得上一条观察', async () => {
  const client = testClient({ listTools: vi.fn().mockResolvedValue([ready, unverified, missing]) });
  renderWithToasts(<ToolsPage client={client} />);

  expect(await screen.findByText('Codex CLI')).toBeInTheDocument();
  // 断言收在列表里：状态词典（折叠的 <details>）里也有这几个词，不限定范围会撞上。
  const list = screen.getByRole('list');
  expect(within(list).getByText('已就绪')).toBeInTheDocument();
  expect(within(list).getByText('未验证')).toBeInTheDocument();
  expect(within(list).getByText('未安装')).toBeInTheDocument();
  // 概览数字来自真实清单，不是编的。
  expect(screen.getByText(/检测到 2 个已安装，其中 1 个已就绪；清单共 3 个/)).toBeInTheDocument();
});

it('不展开也能读到它是什么、装在哪、版本多少', async () => {
  const client = testClient({ listTools: vi.fn().mockResolvedValue([ready, missing]) });
  renderWithToasts(<ToolsPage client={client} />);

  const list = await screen.findByRole('list');
  // 行内事实：路径 + 版本；没装的那条给「未找到可执行文件」而不是空白。
  expect(within(list).getByText(/\/opt\/homebrew\/bin\/codex · codex-cli 0.154.0/)).toBeInTheDocument();
  expect(within(list).getByText('未找到可执行文件')).toBeInTheDocument();
});

it('状态词典解释「已安装」和「未验证」的区别——这一页最容易被误读的地方', async () => {
  const user = userEvent.setup();
  const client = testClient({ listTools: vi.fn().mockResolvedValue([ready, unverified]) });
  renderWithToasts(<ToolsPage client={client} />);

  await user.click(await screen.findByText('这些状态是怎么判定的'));
  const term = screen.getByText('这些状态是怎么判定的').closest('details')!;
  expect(within(term).getByText(/检测没有通过（超时或返回异常）/)).toBeInTheDocument();
  expect(within(term).getByText(/可执行文件存在、版本检测正常/)).toBeInTheDocument();
});

it('展开后能看到探针原文与「为什么不是已就绪」', async () => {
  const user = userEvent.setup();
  const client = testClient({ listTools: vi.fn().mockResolvedValue([unverified]) });
  renderWithToasts(<ToolsPage client={client} />);

  await user.click(await screen.findByRole('button', { name: /Aider/ }));
  expect(screen.getByText('版本探针原文')).toBeInTheDocument();
  expect(screen.getByText(/command timed out/)).toBeInTheDocument();
  expect(screen.getByText('版本探针超时，已终止该进程')).toBeInTheDocument();
  // 没有做过登录判定时不能渲染一个空的「登录探针」区块。
  expect(screen.queryByText('登录探针原文')).not.toBeInTheDocument();
});

it('「只重新检测这个」只重探一个工具，不整页重扫', async () => {
  const user = userEvent.setup();
  const probeTool = vi.fn().mockResolvedValue({ ...ready, versionProbeTail: 'codex-cli 0.155.0' });
  const listTools = vi.fn().mockResolvedValue([ready, missing]);
  renderWithToasts(<ToolsPage client={testClient({ listTools, probeTool })} />);

  await user.click(await screen.findByRole('button', { name: /Codex CLI/ }));
  await user.click(screen.getByRole('button', { name: '只重新检测这个' }));

  expect(probeTool).toHaveBeenCalledWith('codex', 'zh-Hans');
  expect(listTools).toHaveBeenCalledTimes(1);
  expect(await screen.findByText('codex-cli 0.155.0')).toBeInTheDocument();
});

it('清单不可用时说明这是安装包的问题，而不是显示空表', async () => {
  const client = testClient({
    listTools: vi.fn().mockRejectedValue({
      code: 'CATALOG_SCHEMA_MISMATCH', messageKey: 'error.toolCatalogUnavailable',
      safeDetails: ['工具清单不是合法 JSON'], retryable: false, recoveryActions: [],
    }),
  });
  renderWithToasts(<ToolsPage client={client} />);

  expect(await screen.findByText('工具清单不可用')).toBeInTheDocument();
  expect(screen.getByText(/不是你的机器的问题/)).toBeInTheDocument();
  expect(screen.queryByText('已就绪')).not.toBeInTheDocument();
});

it('搜索按名称与路径过滤，空结果给可执行的下一步', async () => {
  const user = userEvent.setup();
  const client = testClient({ listTools: vi.fn().mockResolvedValue([ready, missing]) });
  renderWithToasts(<ToolsPage client={client} />);

  const search = await screen.findByLabelText('搜索工具');
  await user.type(search, 'opt/homebrew');
  const list = screen.getByRole('list');
  expect(within(list).getByText('Codex CLI')).toBeInTheDocument();
  expect(within(list).queryByText('FFmpeg')).not.toBeInTheDocument();

  await user.clear(search);
  await user.type(search, '不存在的工具');
  expect(screen.getByText('没有匹配的工具')).toBeInTheDocument();
});

it('本版不放安装入口：做不到的事不出现在界面上', async () => {
  const client = testClient({ listTools: vi.fn().mockResolvedValue([missing]) });
  renderWithToasts(<ToolsPage client={client} />);

  await screen.findByText('FFmpeg');
  expect(screen.queryByRole('button', { name: /安装/ })).not.toBeInTheDocument();
  // 页脚那句「替你装第三方工具不在本版范围内」已经按用户要求撤掉：它解释的是一颗根本不
  // 存在的按钮，长期挂在页面底部只是噪音。这里不再断言它，但仍然守住「没有安装入口」。
  expect(screen.queryByText(/不在本版范围内/)).not.toBeInTheDocument();
});
