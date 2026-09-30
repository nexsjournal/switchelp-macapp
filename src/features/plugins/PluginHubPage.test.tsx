import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { RepoCatalog, SkillRecord, TargetPlan } from '@/contracts/types';
import { PluginHubPage, resetCatalogErrorAnnouncements } from './PluginHubPage';
import { testClient } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

/**
 * 「一次运行里同一个 messageKey 只弹一次提示」是模块级记录：不在这里清掉的话，
 * 前一个用例会把后一个用例该看见的提示吞掉（提示队列本身由 vitest.setup.ts 清理）。
 */
beforeEach(() => { resetCatalogErrorAnnouncements(); });

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

/** 核心对 403/429 的真实返回：详情就是用户截图里那句。 */
const rateLimited = {
  code: 'INTERNAL', messageKey: 'error.pluginRateLimited',
  safeDetails: ['公开接口的访问频率已用尽，稍后再试或在设置里填一个 GitHub 令牌'],
  retryable: true, recoveryActions: [],
};

const repoNotFound = {
  code: 'NOT_FOUND', messageKey: 'error.pluginRepoNotFound',
  safeDetails: ['https://api.github.com/repos/owner/nope 返回 404'], retryable: false, recoveryActions: [],
};

/**
 * 定位常驻的那一行提醒。
 *
 * 摘要与一次性提示可能写着同一句话，所以按「详情」按钮往上找它所在的 `section`，
 * 断言只针对页面上那一行，不误抓提示。
 */
function notice(): HTMLElement {
  // 展开后这个按钮改口「收起」，两种状态都要能找到。
  return screen.getByRole('button', { name: /详情|收起/ }).closest('section') as HTMLElement;
}

/**
 * 从卡片网格进详情。
 *
 * 卡片上那枚铺满整张卡的点击层按钮带着技能名的可访问名；
 * 进去之后用详情标题（h2）确认落地，免得后续断言对着一份还在的列表跑。
 */
async function enterDetail(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(await screen.findByRole('button', { name: new RegExp(name) }));
  await screen.findByRole('heading', { name });
}

/** 卡片网格（列表视图的那个 ul）。 */
function grid(): HTMLElement {
  return screen.getByRole('list', { name: '技能卡片' });
}

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
  const user = userEvent.setup();
  renderWithToasts(<PluginHubPage client={baseClient()} />);

  // 列表里只有卡片（名字＋描述）：那句警告只在详情里，进去才看得到。
  await enterDetail(user, 'frontend-design');
  expect(screen.getByText(/技能是给 AI 的执行说明/)).toBeInTheDocument();
  expect(screen.getByText(/看清它会要求 AI 执行哪些命令/)).toBeInTheDocument();
});

it('front-matter 解析不了时说明回落，且不阻止安装', async () => {
  const user = userEvent.setup();
  renderWithToasts(<PluginHubPage client={baseClient()} />);

  await enterDetail(user, 'no-front-matter');
  expect(screen.getByText(/头部没能解析/)).toBeInTheDocument();
  expect(screen.getByText(/它需要这些命令：pandoc/)).toBeInTheDocument();
  expect(screen.getByText(/另外还会写入 1 个同目录文件/)).toBeInTheDocument();
  expect(screen.getByRole('button', { name: '添加技能' })).toBeEnabled();
});

it('安装前先出计划：写哪些文件、装到哪些工具', async () => {
  const user = userEvent.setup();
  const client = baseClient();
  renderWithToasts(<PluginHubPage client={client} />);

  await enterDetail(user, 'frontend-design');
  await user.click(screen.getByRole('button', { name: '添加技能' }));

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

  await enterDetail(user, 'frontend-design');
  await user.click(screen.getByRole('button', { name: '添加技能' }));
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

  await enterDetail(user, 'frontend-design');
  await user.click(screen.getByRole('button', { name: '添加技能' }));
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

  await enterDetail(user, 'frontend-design');
  await user.click(screen.getByRole('button', { name: '添加技能' }));
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

it('打开仓库交给系统浏览器，而不是 webview 里没人接的 window.open', async () => {
  const user = userEvent.setup();
  const openExternalUrl = vi.fn().mockResolvedValue(undefined);
  const client = baseClient({ openExternalUrl });
  renderWithToasts(<PluginHubPage client={client} />);

  // 回归：以前这里调 window.open，而 Tauri 的 webview 没有浏览器新窗口，点了没反应。
  await enterDetail(user, 'frontend-design');
  await user.click(screen.getByRole('button', { name: /打开仓库/ }));
  await waitFor(() => expect(openExternalUrl).toHaveBeenCalledWith('https://github.com/anthropics/skills'));
});

describe('卡片网格与详情', () => {
  it('列表视图把所有技能都排成卡片，不再一页只看得见几条', async () => {
    renderWithToasts(<PluginHubPage client={baseClient()} />);

    // 网格一次铺开：两条技能各占一张卡（不是左列表 + 右详情）。
    await screen.findByRole('list', { name: '技能卡片' });
    expect(within(grid()).getAllByRole('listitem')).toHaveLength(2);
    expect(within(grid()).getByText('frontend-design')).toBeInTheDocument();
    // 没写描述的也有话说，不留空白。
    expect(within(grid()).getByText('这份技能没有写描述')).toBeInTheDocument();
    // 卡片上就有两个快捷操作。
    expect(within(grid()).getAllByRole('button', { name: '添加技能到工作台' })).toHaveLength(2);
    expect(within(grid()).getAllByRole('button', { name: '打开来源页' })).toHaveLength(2);
  });

  it('点卡片进详情，返回按钮回列表', async () => {
    const user = userEvent.setup();
    renderWithToasts(<PluginHubPage client={baseClient()} />);

    await enterDetail(user, 'frontend-design');
    // 详情视图：列表连同搜索框一起让位，展开 SKILL.md 原文的入口在这里。
    expect(screen.queryByRole('list', { name: '技能卡片' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '查看 SKILL.md 原文' })).toBeInTheDocument();
    expect(screen.getByText(/技能是给 AI 的执行说明/)).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: '返回技能列表' }));
    expect(grid()).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'frontend-design' })).not.toBeInTheDocument();
  });

  it('卡片上的加号走安装计划，不直接装、也不进详情', async () => {
    const user = userEvent.setup();
    const client = baseClient();
    renderWithToasts(<PluginHubPage client={client} />);

    await screen.findByRole('list', { name: '技能卡片' });
    await user.click(screen.getAllByRole('button', { name: '添加技能到工作台' })[0]!);

    // 快捷操作只是省一次点击，写文件之前的那张计划弹窗不能省。
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByRole('heading', { name: '确认安装' })).toBeInTheDocument();
    expect(client.previewPluginInstall).toHaveBeenCalledWith(expect.objectContaining({ skillDirs: ['frontend-design'] }));
    expect(client.installPlugin).not.toHaveBeenCalled();
    // 加号不是「进详情」：关掉弹窗后人还留在网格上（弹窗开着时整页被 aria-hidden，量不了）。
    await user.click(within(dialog).getByRole('button', { name: '取消' }));
    expect(grid()).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'frontend-design' })).not.toBeInTheDocument();
  });

  it('卡片上的来源图标交回系统浏览器，且不会顺带把人带进详情', async () => {
    const user = userEvent.setup();
    const openExternalUrl = vi.fn().mockResolvedValue(undefined);
    renderWithToasts(<PluginHubPage client={baseClient({ openExternalUrl })} />);

    await screen.findByRole('list', { name: '技能卡片' });
    await user.click(screen.getAllByRole('button', { name: '打开来源页' })[0]!);

    await waitFor(() => expect(openExternalUrl).toHaveBeenCalledWith('https://github.com/anthropics/skills'));
    // 回归：整张卡也是可点的（点击层铺满卡片），图标按钮压在它之上，点它不该连带跳进详情。
    expect(screen.queryByRole('heading', { name: 'frontend-design' })).not.toBeInTheDocument();
    expect(grid()).toBeInTheDocument();
  });
});

describe('技能市场', () => {
  it('搜索按关键词送到服务端，打开的是市场自己的页面', async () => {
    const user = userEvent.setup();
    const browsePluginRepo = vi.fn().mockResolvedValue({
      ...catalog, repo: 'clawhub', homepage: 'https://clawhub.ai/skills/frontend-design',
    });
    const openExternalUrl = vi.fn().mockResolvedValue(undefined);
    const client = baseClient({
      listPluginSources: vi.fn().mockResolvedValue([
        { repo: 'clawhub', label: 'ClawHub 技能市场', description: '社区技能注册表', builtin: true },
      ]),
      browsePluginRepo, openExternalUrl,
    });
    renderWithToasts(<PluginHubPage client={client} />);

    // 来源标识里没有 `/` ＝ 市场：本地筛几千条没有意义，关键词要交给对方的搜索接口（400ms 防抖）。
    await screen.findByRole('list', { name: '技能卡片' });
    await user.type(screen.getByLabelText('搜索技能'), 'pdf');
    // 第三个参数是「顺带补齐哪个技能的正文」，只有进详情时才有值；搜索这一次是 undefined。
    await waitFor(() => expect(browsePluginRepo).toHaveBeenCalledWith('clawhub', 'pdf', undefined), { timeout: 2000 });

    // 市场不是 GitHub：卡片上的来源图标回链到它自己的技能页。
    await user.click(screen.getAllByRole('button', { name: '打开来源页' })[0]!);
    await waitFor(() => expect(openExternalUrl).toHaveBeenCalledWith('https://clawhub.ai/skills/frontend-design'));

    // 详情视图同样可用，按钮也按来源改口。
    await enterDetail(user, 'frontend-design');
    expect(screen.getByRole('button', { name: '在市场里查看' })).toBeInTheDocument();
  });
});

it('仓库里没有技能时说清楚，而不是显示一个空市场', async () => {
  const user = userEvent.setup();
  const client = baseClient({
    browsePluginRepo: vi.fn().mockRejectedValue({
      code: 'NOT_FOUND', messageKey: 'error.pluginRepoHasNoSkills',
      safeDetails: ['anthropics/skills 在提交 9c1f0a7b 里没有任何 SKILL.md'], retryable: false, recoveryActions: [],
    }),
  });
  renderWithToasts(<PluginHubPage client={client} />);

  // 默认只有一行摘要，原因要点开才看。
  expect(await screen.findByRole('button', { name: '详情' })).toBeInTheDocument();
  expect(within(notice()).getByText('技能目录没能取到')).toBeInTheDocument();
  expect(screen.queryByText(/没有任何 SKILL.md/)).not.toBeInTheDocument();
  // 取不到目录不等于「仓库是空的」——空状态会让人去别处找原因。
  expect(screen.queryByText('还没有可浏览的内容')).not.toBeInTheDocument();

  await user.click(screen.getByRole('button', { name: '详情' }));
  expect(screen.getByText(/没有任何 SKILL.md/)).toBeInTheDocument();
});

it('限额失败：折叠时只有一句摘要，原因与「重试」在详情里', async () => {
  const user = userEvent.setup();
  const client = baseClient({ browsePluginRepo: vi.fn().mockRejectedValue(rateLimited) });
  renderWithToasts(<PluginHubPage client={client} />);

  expect(await screen.findByRole('button', { name: '详情' })).toBeInTheDocument();
  expect(within(notice()).getByText('GitHub 接口的访问频率已用尽')).toBeInTheDocument();
  // 折叠态只有那一行：完整原因、重试、令牌入口都不该已经占着版面。
  expect(screen.queryByText(/公开接口的访问频率已用尽/)).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '重试' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '填写令牌' })).not.toBeInTheDocument();
  // 完整原因不铺在页面上——那正是报告里「一直提示这个信息」的那条红字。
  expect(screen.queryByText(/稍后再试或在设置里填一个 GitHub 令牌/)).not.toBeInTheDocument();

  await user.click(screen.getByRole('button', { name: '详情' }));
  expect(screen.getByText(/稍后再试或在设置里填一个 GitHub 令牌/)).toBeInTheDocument();
  expect(screen.getByRole('button', { name: '重试' })).toBeInTheDocument();
  // 限额的唯一解法是令牌，这条路径必须在能给出来的地方给出来。
  expect(screen.getByRole('button', { name: '填写令牌' })).toBeInTheDocument();
});

it('不是限额的失败不给「令牌」这条路——它解决不了问题', async () => {
  const user = userEvent.setup();
  const client = baseClient({ browsePluginRepo: vi.fn().mockRejectedValue(repoNotFound) });
  renderWithToasts(<PluginHubPage client={client} />);

  await user.click(await screen.findByRole('button', { name: '详情' }));
  expect(screen.getByText(/返回 404/)).toBeInTheDocument();
  expect(screen.getByRole('button', { name: '重试' })).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '填写令牌' })).not.toBeInTheDocument();
  // 摘要说「没能取到」，不说限额那句；条子的档位也不同（danger 才带 role="alert"）。
  expect(within(notice()).getByText('技能目录没能取到')).toBeInTheDocument();
  expect(within(await screen.findByLabelText('通知')).getByRole('alert')).toBeInTheDocument();
});

it('点「填写令牌」开弹窗，存好之后自动重抓一次目录', async () => {
  const user = userEvent.setup();
  const browsePluginRepo = vi.fn().mockRejectedValueOnce(rateLimited).mockResolvedValue(catalog);
  const setContentGithubToken = vi.fn().mockResolvedValue(true);
  const client = baseClient({ browsePluginRepo, setContentGithubToken });
  renderWithToasts(<PluginHubPage client={client} />);

  await user.click(await screen.findByRole('button', { name: '详情' }));
  await user.click(screen.getByRole('button', { name: '填写令牌' }));

  const dialog = await screen.findByRole('dialog');
  await user.type(within(dialog).getByLabelText('GitHub 令牌'), 'ghp_secret');
  await user.click(within(dialog).getByRole('button', { name: '保存' }));

  await waitFor(() => expect(setContentGithubToken).toHaveBeenCalledWith('ghp_secret'));
  // 存了令牌立刻再抓一次：不自动重试等于让用户自己再点一遍。
  await waitFor(() => expect(browsePluginRepo).toHaveBeenCalledTimes(2));
  expect(await screen.findByRole('list', { name: '技能卡片' })).toBeInTheDocument();
  expect(within(grid()).getByText('frontend-design')).toBeInTheDocument();
});

it('首次失败弹一次提示；同一个原因再失败只留那一行', async () => {
  const user = userEvent.setup();
  const browsePluginRepo = vi.fn().mockRejectedValue(rateLimited);
  renderWithToasts(<PluginHubPage client={baseClient({ browsePluginRepo })} />);

  // 限额是「等等就好」：提示用会自动消失的 info（danger 才带 role="alert"）。
  const notifications = await screen.findByLabelText('通知');
  expect(within(notifications).getByText('GitHub 接口的访问频率已用尽')).toBeInTheDocument();
  expect(within(notifications).queryByRole('alert')).not.toBeInTheDocument();

  await user.click(await screen.findByRole('button', { name: '详情' }));
  await user.click(screen.getByRole('button', { name: '重试' }));

  // 重试期间那一行先消失；它带着折叠态回来＝第二次失败已经落地，而提示还只有第一条。
  await waitFor(() => expect(screen.getByRole('button', { name: '详情' })).toHaveAttribute('aria-expanded', 'false'));
  expect(browsePluginRepo).toHaveBeenCalledTimes(2);
  expect(within(notifications).getAllByText('GitHub 接口的访问频率已用尽')).toHaveLength(1);
});

it('目录取到了但里面没有技能，才说「还没有可浏览的内容」', async () => {
  const client = baseClient({ browsePluginRepo: vi.fn().mockResolvedValue({ ...catalog, skills: [] }) });
  renderWithToasts(<PluginHubPage client={client} />);

  expect(await screen.findByText('还没有可浏览的内容')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '详情' })).not.toBeInTheDocument();
});

describe('已安装列表', () => {
  it('没有装过时给出下一步，而不是空表格', async () => {
    const user = userEvent.setup();
    renderWithToasts(<PluginHubPage client={baseClient()} />);
    await user.click(await screen.findByRole('tab', { name: /已安装/ }));
    expect(screen.getByText('还没装任何技能')).toBeInTheDocument();
  });
});
