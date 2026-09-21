import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { vi } from 'vitest';

import { App } from '@/app/App';
import type { UpdateProgress, UpdateReport } from '@/desktop/client';
import { testClient } from '../../../tests/helpers/client';

/**
 * 应用内更新的界面契约。
 *
 * 守三件事：
 * - 入口在侧栏左上角，有更新时**顶掉副标题那一行**（不是多出一行把侧栏推下去）；
 * - 弹窗把「从哪版到哪版、会重启」说清楚再动手，失败时如实报错、不显示成成功；
 * - 安装成功没有收尾界面（应用会重启），所以「成功了没有」由更新后启动时的那条 Toast 交代。
 */

const report = (overrides: Partial<UpdateReport> = {}): UpdateReport => ({
  current: '0.2.0', latest: '0.3.0', hasUpdate: true, notes: '## 说明\n\n- 一条改动',
  releaseUrl: 'https://github.com/nexsjournal/switchelp-macapp/releases/tag/v0.3.0',
  publishedAt: '2026-09-21', error: null, ...overrides,
});

test('有更新时侧栏出现更新按钮，副标题让位', async () => {
  const client = testClient({ checkUpdate: vi.fn().mockResolvedValue(report()) });
  render(<App client={client} />);

  const trigger = await screen.findByRole('button', { name: '更新 0.3.0' });
  expect(trigger).toBeInTheDocument();
  // 两行变一行：副标题不再显示，侧栏高度不变。
  expect(screen.queryByText('Codex 配置管理')).not.toBeInTheDocument();
});

test('没有更新时只有副标题，不摆一个「已是最新」的常驻按钮', async () => {
  const client = testClient({
    checkUpdate: vi.fn().mockResolvedValue(report({ hasUpdate: false, latest: '0.2.0', notes: null })),
  });
  render(<App client={client} />);

  // 先确认首屏加载完成（副标题是同步渲染的，这里等的是检查结果落地后的稳定态）。
  expect(await screen.findByText('Codex 配置管理')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: /更新/ })).not.toBeInTheDocument();
});

test('检查更新失败时静默：不出现按钮，也不在界面上报警', async () => {
  const client = testClient({ checkUpdate: vi.fn().mockRejectedValue(new Error('offline')) });
  render(<App client={client} />);

  expect(await screen.findByText('Codex 配置管理')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: /更新/ })).not.toBeInTheDocument();
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

test('点更新按钮打开弹窗：说清版本去向、说明与重启后果', async () => {
  const user = userEvent.setup();
  const client = testClient({ checkUpdate: vi.fn().mockResolvedValue(report()) });
  render(<App client={client} />);

  await user.click(await screen.findByRole('button', { name: '更新 0.3.0' }));

  const dialog = await screen.findByRole('dialog');
  expect(within(dialog).getByText('有新版本可以安装')).toBeInTheDocument();
  expect(within(dialog).getByText('当前 0.2.0 → 新版本 0.3.0')).toBeInTheDocument();
  expect(within(dialog).getByText(/一条改动/)).toBeInTheDocument();
  expect(within(dialog).getByText(/应用会自动重启/)).toBeInTheDocument();
  // 还没点安装：这时候不该有任何安装调用。
  expect(client.installUpdate).not.toHaveBeenCalled();
});

test('下载中显示真实进度，并说明关闭窗口不会中断下载', async () => {
  const user = userEvent.setup();
  const sink: { push: ((progress: UpdateProgress) => void) | null } = { push: null };
  const client = testClient({
    checkUpdate: vi.fn().mockResolvedValue(report()),
    // 真实安装成功时不会 resolve（应用重启），这里照此保持下载中。
    installUpdate: vi.fn().mockImplementation(() => new Promise<void>(() => undefined)),
    onUpdateProgress: vi.fn().mockImplementation(async (listener: (progress: UpdateProgress) => void) => {
      sink.push = listener;
      return () => { sink.push = null; };
    }),
  });
  render(<App client={client} />);

  await user.click(await screen.findByRole('button', { name: '更新 0.3.0' }));
  await user.click(screen.getByRole('button', { name: '下载并安装' }));

  expect(client.installUpdate).toHaveBeenCalledTimes(1);
  expect(await screen.findByText('正在下载更新包…')).toBeInTheDocument();
  sink.push?.({ phase: 'download', downloaded: 2_097_152, total: 4_194_304 });
  expect(await screen.findByText('已下载 2.0 MB / 4.0 MB')).toBeInTheDocument();
  expect(screen.getByText(/不会中断下载/)).toBeInTheDocument();
});

test('上游没给长度时进度退化成不确定态，不显示假百分比', async () => {
  const user = userEvent.setup();
  const sink: { push: ((progress: UpdateProgress) => void) | null } = { push: null };
  const client = testClient({
    checkUpdate: vi.fn().mockResolvedValue(report()),
    installUpdate: vi.fn().mockImplementation(() => new Promise<void>(() => undefined)),
    onUpdateProgress: vi.fn().mockImplementation(async (listener: (progress: UpdateProgress) => void) => {
      sink.push = listener;
      return () => { sink.push = null; };
    }),
  });
  render(<App client={client} />);

  await user.click(await screen.findByRole('button', { name: '更新 0.3.0' }));
  await user.click(screen.getByRole('button', { name: '下载并安装' }));
  sink.push?.({ phase: 'download', downloaded: 1_048_576, total: null });

  expect(await screen.findByText('已下载 1.0 MB')).toBeInTheDocument();
});

test('弹窗里的「看完整说明」走系统浏览器打开发布页', async () => {
  const user = userEvent.setup();
  const client = testClient({
    checkUpdate: vi.fn().mockResolvedValue(report()),
    openReleasePage: vi.fn().mockResolvedValue(undefined),
  });
  render(<App client={client} />);

  await user.click(await screen.findByRole('button', { name: '更新 0.3.0' }));
  await user.click(await screen.findByRole('button', { name: '在 GitHub 上看完整说明' }));

  expect(client.openReleasePage).toHaveBeenCalledWith('https://github.com/nexsjournal/switchelp-macapp/releases/tag/v0.3.0');
});

test('安装失败时如实报错，给出重试与手动下载两条路', async () => {
  const user = userEvent.setup();
  const client = testClient({
    checkUpdate: vi.fn().mockResolvedValue(report()),
    installUpdate: vi.fn().mockRejectedValue({
      code: 'VALIDATION_FAILED', messageKey: 'error.updateInstallFailed',
      safeDetails: ['安装包签名校验没通过，已放弃安装'], retryable: false, recoveryActions: [],
    }),
    openReleasePage: vi.fn().mockResolvedValue(undefined),
  });
  render(<App client={client} />);

  await user.click(await screen.findByRole('button', { name: '更新 0.3.0' }));
  await user.click(screen.getByRole('button', { name: '下载并安装' }));

  const failure = await screen.findByRole('alert');
  expect(within(failure).getByText('更新没有完成。')).toBeInTheDocument();
  expect(within(failure).getByText('安装包签名校验没通过，已放弃安装')).toBeInTheDocument();
  expect(screen.getByText(/当前版本不受影响/)).toBeInTheDocument();

  await user.click(screen.getByRole('button', { name: '到发布页手动下载' }));
  expect(client.openReleasePage).toHaveBeenCalledWith('https://github.com/nexsjournal/switchelp-macapp/releases/tag/v0.3.0');

  // 重试会再调一次安装，而不是只改文案。
  await user.click(screen.getByRole('button', { name: '重试' }));
  expect(client.installUpdate).toHaveBeenCalledTimes(2);
});

test('更新成功后重启，这次启动用一条 Toast 交代结果', async () => {
  const client = testClient({
    checkUpdate: vi.fn().mockResolvedValue(report({ hasUpdate: false, latest: '0.3.0', notes: null })),
    // 标记文件读一次就删：第二次调用必须给 null，否则每次渲染都会再报一遍「已更新」。
    takeUpdateResult: vi.fn().mockResolvedValueOnce('0.3.0').mockResolvedValue(null),
  });
  // App 自带 ToastHost；这里不能再套一个 renderWithToasts——两个宿主会把同一条提示
  // 渲染成两份 DOM，断言「只提示一次」就失去意义。
  render(<App client={client} />);

  const toasts = await screen.findAllByText('已更新到 0.3.0');
  expect(toasts).toHaveLength(1);
});
