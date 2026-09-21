import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { RepoCatalog, SkillRecord, TargetPlan } from '@/contracts/types';
import { PluginHubPage } from './PluginHubPage';
import { testClient } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

const catalog: RepoCatalog = {
  repo: 'anthropics/skills',
  commit: '9c1f0a7b2e4d5f6a',
  fetchedAt: 1_789_956_000,
  truncated: false,
  skills: [
    {
      dirName: 'frontend-design', sourcePath: 'skills/frontend-design',
      document: { id: 'frontend-design', title: null, description: '界面规范', requiresBins: [], body: '# 前端设计\n正文', frontMatterParsed: true },
      files: [{ path: 'SKILL.md', bytes: 100, text: '# 前端设计\n正文' }],
    },
    {
      dirName: 'no-front-matter', sourcePath: 'skills/no-front-matter',
      document: { id: 'no-front-matter', title: null, description: null, requiresBins: ['pandoc'], body: '正文', frontMatterParsed: false },
      files: [
        { path: 'SKILL.md', bytes: 10, text: '正文' },
        { path: 'references/notes.md', bytes: 5, text: '笔记' },
      ],
    },
  ],
};

const targetCodex: TargetPlan = {
  toolId: 'codex', displayName: 'Codex CLI', root: '/Users/me/.codex/skills', dir: '/Users/me/.codex/skills/frontend-design',
  dirName: 'frontend-design', action: 'create',
  files: [{ path: 'SKILL.md', bytes: 100, sha256: 'x' }], conflictDetail: null, foreignFiles: [],
};

const targetClaude: TargetPlan = { ...targetCodex, toolId: 'claude-code', displayName: 'Claude Code', root: '/Users/me/.claude/skills', dir: '/Users/me/.claude/skills/frontend-design' };

function baseClient(overrides = {}) {
  return testClient({
    listPluginSources: vi.fn().mockResolvedValue([
      { repo: 'anthropics/skills', label: 'Anthropic 官方技能集合', description: '官方技能', builtin: true },
    ]),
    listSkillTargets: vi.fn().mockResolvedValue([
      { toolId: 'codex', displayName: 'Codex CLI', root: '/Users/me/.codex/skills' },
      { toolId: 'claude-code', displayName: 'Claude Code', root: '/Users/me/.claude/skills' },
    ]),
    browsePluginRepo: vi.fn().mockResolvedValue(catalog),
    previewPluginInstall: vi.fn().mockResolvedValue({
      repo: catalog.repo, commit: catalog.commit,
      skills: [{
        skillId: 'frontend-design', dirName: 'frontend-design', sourcePath: 'skills/frontend-design',
        description: '界面规范', requiresBins: [], targets: [targetCodex, targetClaude],
      }],
    }),
    installPlugin: vi.fn().mockResolvedValue({ repo: catalog.repo, commit: catalog.commit, installed: [], skipped: [], failed: [] }),
    listInstalledSkills: vi.fn().mockResolvedValue([]),
    checkSkillUpdates: vi.fn().mockResolvedValue([]),
    ...overrides,
  });
}

it('技能被明确说明是给 AI 的指令，不只是普通内容', async () => {
  renderWithToasts(<PluginHubPage client={baseClient()} />);

  // 名字同时出现在列表与详情里，这里按详情标题断言。
  expect(await screen.findByRole('heading', { name: 'frontend-design' })).toBeInTheDocument();
  expect(screen.getByText(/技能是给 AI 的执行说明/)).toBeInTheDocument();
  expect(screen.getByText(/看清它会要求 AI 执行哪些命令/)).toBeInTheDocument();
});

it('front-matter 解析不了时说明回落，且不阻止安装', async () => {
  const user = userEvent.setup();
  renderWithToasts(<PluginHubPage client={baseClient()} />);

  await user.click(await screen.findByRole('button', { name: /no-front-matter/ }));
  expect(screen.getByText(/头部没能解析/)).toBeInTheDocument();
  expect(screen.getByText(/它需要这些命令：pandoc/)).toBeInTheDocument();
  expect(screen.getByText(/另外还会写入 1 个同目录文件/)).toBeInTheDocument();
  expect(screen.getByRole('button', { name: '添加技能' })).toBeEnabled();
});

it('安装前先出计划：写哪些文件、装到哪些工具', async () => {
  const user = userEvent.setup();
  const client = baseClient();
  renderWithToasts(<PluginHubPage client={client} />);

  await user.click(await screen.findByRole('button', { name: '添加技能' }));

  const dialog = await screen.findByRole('dialog');
  expect(within(dialog).getByRole('heading', { name: '确认安装' })).toBeInTheDocument();
  expect(within(dialog).getByText(/不执行仓库里的任何脚本/)).toBeInTheDocument();
  expect(within(dialog).getByText('/Users/me/.codex/skills/frontend-design')).toBeInTheDocument();
  expect(within(dialog).getByText('/Users/me/.claude/skills/frontend-design')).toBeInTheDocument();
  expect(within(dialog).getAllByText('写入 1 个文件')).toHaveLength(2);
  expect(client.previewPluginInstall).toHaveBeenCalledWith(expect.objectContaining({ skillDirs: ['frontend-design'], targets: ['codex', 'claude-code'] }));
});

it('目标目录不是我们装的时候，默认跳过并且不给「覆盖」这个选项', async () => {
  const user = userEvent.setup();
  const conflict: TargetPlan = {
    ...targetCodex, action: 'conflict', dirName: 'frontend-design',
    conflictDetail: '/Users/me/.codex/skills/frontend-design 已存在，且含有 2 个不是本工具写入的文件',
    foreignFiles: ['notes.md', 'draft.md'],
  };
  const client = baseClient({
    previewPluginInstall: vi.fn().mockResolvedValue({
      repo: catalog.repo, commit: catalog.commit,
      skills: [{ skillId: 'frontend-design', dirName: 'frontend-design', sourcePath: 'skills/frontend-design', description: null, requiresBins: [], targets: [conflict] }],
    }),
  });
  renderWithToasts(<PluginHubPage client={client} />);

  await user.click(await screen.findByRole('button', { name: '添加技能' }));
  const dialog = await screen.findByRole('dialog');

  expect(within(dialog).getByText('冲突')).toBeInTheDocument();
  expect(within(dialog).getByText(/不是本工具写入的文件/)).toBeInTheDocument();
  // 默认选中「跳过」——冲突默认不动别人的目录。
  expect(within(dialog).getByRole('radio', { name: '跳过这个目标' })).toBeChecked();
  expect(within(dialog).getByRole('radio', { name: /保留两者/ })).not.toBeChecked();
  // 只提供两个选择，没有「覆盖」。
  expect(within(dialog).getAllByRole('radio')).toHaveLength(2);
});

it('选择保留两者后，安装请求带上冲突处置', async () => {
  const user = userEvent.setup();
  const conflict: TargetPlan = { ...targetCodex, action: 'conflict', conflictDetail: '已存在', foreignFiles: ['a.md'] };
  const installPlugin = vi.fn().mockResolvedValue({ repo: catalog.repo, commit: catalog.commit, installed: [], skipped: [], failed: [] });
  const client = baseClient({
    previewPluginInstall: vi.fn().mockResolvedValue({
      repo: catalog.repo, commit: catalog.commit,
      skills: [{ skillId: 'frontend-design', dirName: 'frontend-design', sourcePath: 'skills/frontend-design', description: null, requiresBins: [], targets: [conflict] }],
    }),
    installPlugin,
  });
  renderWithToasts(<PluginHubPage client={client} />);

  await user.click(await screen.findByRole('button', { name: '添加技能' }));
  const dialog = await screen.findByRole('dialog');
  await user.click(within(dialog).getByRole('radio', { name: /保留两者/ }));
  await user.click(within(dialog).getByRole('button', { name: '确认安装' }));

  await waitFor(() => expect(installPlugin).toHaveBeenCalledWith(expect.objectContaining({
    conflictChoices: { 'codex::frontend-design': 'keepBoth' },
  })));
});

it('部分完成如实分开报：装了哪些、跳过哪些、失败哪些', async () => {
  const user = userEvent.setup();
  const client = baseClient({
    installPlugin: vi.fn().mockResolvedValue({
      repo: catalog.repo, commit: catalog.commit,
      installed: [{
        skillId: 'frontend-design', dirName: 'frontend-design', targetTool: 'codex', targetDisplayName: 'Codex CLI',
        sourceRepo: catalog.repo, sourceCommit: catalog.commit, sourcePath: 'skills/frontend-design',
        installedPath: '/Users/me/.codex/skills/frontend-design', enabled: true, installedAt: 1, files: [],
      }],
      skipped: [{ toolId: 'claude-code', dirName: 'frontend-design', reason: '目标目录已存在且不是本工具安装的' }],
      failed: [{ toolId: 'cursor', dirName: 'frontend-design', message: '目标工具的技能目录不存在' }],
    }),
  });
  renderWithToasts(<PluginHubPage client={client} />);

  await user.click(await screen.findByRole('button', { name: '添加技能' }));
  await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: '确认安装' }));

  expect(await screen.findByText('安装结果')).toBeInTheDocument();
  expect(screen.getByText('frontend-design 已装到 Codex CLI')).toBeInTheDocument();
  expect(screen.getByText(/frontend-design 未装到 claude-code/)).toBeInTheDocument();
  expect(screen.getByText(/frontend-design 装到 cursor 时失败/)).toBeInTheDocument();
});

it('卸载确认列出将被删除的文件，并说明改过的会保留', async () => {
  const user = userEvent.setup();
  const record: SkillRecord = {
    skillId: 'frontend-design', dirName: 'frontend-design', targetTool: 'codex', targetDisplayName: 'Codex CLI',
    sourceRepo: catalog.repo, sourceCommit: catalog.commit, sourcePath: 'skills/frontend-design',
    installedPath: '/Users/me/.codex/skills/frontend-design', enabled: true, installedAt: 1,
    files: [{ path: 'SKILL.md', sha256: 'a', bytes: 10 }, { path: 'references/notes.md', sha256: 'b', bytes: 5 }],
  };
  const uninstallSkill = vi.fn().mockResolvedValue([{
    dir: record.installedPath, removedFiles: ['SKILL.md'], keptModified: ['references/notes.md'],
    missingFiles: [], foreignFiles: [], removedDir: false,
  }]);
  const client = baseClient({
    listInstalledSkills: vi.fn().mockResolvedValue([record]),
    uninstallSkill,
  });
  renderWithToasts(<PluginHubPage client={client} />);

  await user.click(await screen.findByRole('tab', { name: /已安装/ }));
  await user.click(await screen.findByRole('button', { name: '卸载' }));

  const dialog = await screen.findByRole('dialog');
  expect(within(dialog).getByText('frontend-design/SKILL.md')).toBeInTheDocument();
  expect(within(dialog).getByText('frontend-design/references/notes.md')).toBeInTheDocument();
  expect(within(dialog).getByText(/逐个核对文件指纹/)).toBeInTheDocument();

  await user.click(within(dialog).getByRole('button', { name: '卸载' }));
  await waitFor(() => expect(uninstallSkill).toHaveBeenCalledWith('frontend-design', ['codex']));
});

it('停用后的技能如实显示，并说明改名是本工具的做法', async () => {
  const user = userEvent.setup();
  const record: SkillRecord = {
    skillId: 'frontend-design', dirName: 'frontend-design.disabled', targetTool: 'codex', targetDisplayName: 'Codex CLI',
    sourceRepo: catalog.repo, sourceCommit: catalog.commit, sourcePath: 'skills/frontend-design',
    installedPath: '/Users/me/.codex/skills/frontend-design.disabled', enabled: false, installedAt: 1, files: [],
  };
  const client = baseClient({ listInstalledSkills: vi.fn().mockResolvedValue([record]) });
  renderWithToasts(<PluginHubPage client={client} />);

  await user.click(await screen.findByRole('tab', { name: /已安装/ }));
  expect(await screen.findByText('已停用')).toBeInTheDocument();
  expect(screen.getByText(/不是该工具的官方开关/)).toBeInTheDocument();
});

it('来源可以添加，写法不对时给出可读原因', async () => {
  const user = userEvent.setup();
  const addPluginSource = vi.fn()
    .mockRejectedValueOnce({
      code: 'VALIDATION_FAILED', messageKey: 'error.pluginRepoInvalid',
      safeDetails: ['来源要写成 owner/repo，收到的是：not-a-repo'], retryable: false, recoveryActions: [],
    })
    .mockResolvedValueOnce([
      { repo: 'anthropics/skills', label: 'Anthropic 官方技能集合', description: '', builtin: true },
      { repo: 'owner/repo', label: 'owner/repo', description: '', builtin: false },
    ]);
  const client = baseClient({ addPluginSource });
  renderWithToasts(<PluginHubPage client={client} />);

  const input = await screen.findByLabelText('owner/repo 或仓库地址');
  await user.type(input, 'not-a-repo');
  await user.click(screen.getByRole('button', { name: '添加来源' }));
  expect(await screen.findByText(/来源要写成 owner\/repo/)).toBeInTheDocument();

  await user.clear(input);
  await user.type(input, 'owner/repo');
  await user.click(screen.getByRole('button', { name: '添加来源' }));
  await waitFor(() => expect(addPluginSource).toHaveBeenLastCalledWith('owner/repo'));
});

it('仓库里没有技能时说清楚，而不是显示一个空市场', async () => {
  const client = baseClient({
    browsePluginRepo: vi.fn().mockRejectedValue({
      code: 'NOT_FOUND', messageKey: 'error.pluginRepoHasNoSkills',
      safeDetails: ['anthropics/skills 在提交 9c1f0a7b 里没有任何 SKILL.md'], retryable: false, recoveryActions: [],
    }),
  });
  renderWithToasts(<PluginHubPage client={client} />);

  expect(await screen.findByText(/没有任何 SKILL.md/)).toBeInTheDocument();
});

describe('已安装列表', () => {
  it('没有装过时给出下一步，而不是空表格', async () => {
    const user = userEvent.setup();
    renderWithToasts(<PluginHubPage client={baseClient()} />);
    await user.click(await screen.findByRole('tab', { name: /已安装/ }));
    expect(screen.getByText('还没装任何技能')).toBeInTheDocument();
  });
});
