import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SettingsPage } from './SettingsPage';
import { instance, testClient } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

const gateway = { running: true, paused: false, port: 18765, served: 12, revisions: ['rev_a'], tokenFingerprint: '3f9a1c04', error: null, systemProxy: { httpEnabled: false, endpoint: null, bypassApplied: false } };

test('展示真实网关状态，未启动时给原因而不是假装正常', async () => {
  const stopped = testClient({ detectInstances: vi.fn().mockResolvedValue([]) });
  const { rerender } = renderWithToasts(<SettingsPage client={stopped} gateway={gateway} onNavigate={() => {}} onReopenOnboarding={() => {}} />);

  expect(await screen.findByText('运行中')).toBeInTheDocument();
  // 只展示用户能据以判断的状态，不摆内部实现细节（监听地址、令牌指纹）。
  expect(screen.queryByText('127.0.0.1:18765')).not.toBeInTheDocument();
  expect(screen.queryByText('3f9a1c04')).not.toBeInTheDocument();
  expect(screen.getByText('已发布目录')).toBeInTheDocument();

  rerender(<SettingsPage client={stopped} gateway={{ ...gateway, running: false, port: null, error: '端口被占用' }} onNavigate={() => {}} onReopenOnboarding={() => {}} />);
  expect(screen.getByText('未启动')).toBeInTheDocument();
  expect(screen.getByText('端口被占用')).toBeInTheDocument();
});

test('检测到的实例展示配置路径与冲突工具', async () => {
  const client = testClient({ detectInstances: vi.fn().mockResolvedValue([
    { ...instance, conflictingManagers: ['other-tool'] },
  ]) });
  renderWithToasts(<SettingsPage client={client} gateway={gateway} onNavigate={() => {}} onReopenOnboarding={() => {}} />);

  expect(await screen.findByText(instance.configFile)).toBeInTheDocument();
  expect(screen.getByText(/其他工具：other-tool/)).toBeInTheDocument();
  expect(screen.getByText('配置存在')).toBeInTheDocument();
});

test('备份与更新是真实控制，不是占位说明', async () => {
  renderWithToasts(<SettingsPage client={testClient()} gateway={gateway} onNavigate={() => {}} onReopenOnboarding={() => {}} />);

  expect(await screen.findByText('备份与更新')).toBeInTheDocument();
  expect(screen.getByText(/每次提交前/)).toBeInTheDocument();
  expect(screen.getByText(/最近 20 份/)).toBeInTheDocument();
  expect(await screen.findByRole('button', { name: '检查更新' })).toBeInTheDocument();
  expect(screen.getByRole('button', { name: '立即备份配置' })).toBeInTheDocument();
  // 危险操作只做入口，不在这里直接执行。
  expect(screen.getByRole('button', { name: '还原 Codex 配置' })).toBeInTheDocument();
});

test('更新查询失败时不显示成“已是最新”', async () => {
  const user = userEvent.setup();
  const checkUpdate = vi.fn().mockResolvedValue({ current: '0.1.0', latest: null, hasUpdate: false,
    notes: null, releaseUrl: null, publishedAt: null, error: '无法查询发布信息：timeout' });
  renderWithToasts(<SettingsPage client={testClient({ checkUpdate })} gateway={gateway} onNavigate={() => {}} onReopenOnboarding={() => {}} />);

  await user.click(await screen.findByRole('button', { name: '检查更新' }));

  expect(await screen.findByText(/查询失败：无法查询发布信息/)).toBeInTheDocument();
  // 说明文案里本来就有“已是最新”四个字，所以这里断言的是状态行本身（带括号版本号）。
  expect(screen.queryByText(/已是最新（/)).not.toBeInTheDocument();
});

test('有新版本时指向侧栏的更新入口，发布页走系统浏览器', async () => {
  const user = userEvent.setup();
  const checkUpdate = vi.fn().mockResolvedValue({ current: '0.1.0', latest: '0.2.0', hasUpdate: true,
    notes: null, releaseUrl: 'https://example.test/releases/v0.2.0', publishedAt: '2026-09-18T00:00:00Z', error: null });
  // 设置页不装更新（那在侧栏），这里只确认它把用户指对了地方，并把「打不开浏览器」如实报出来。
  const openReleasePage = vi.fn().mockRejectedValue({ code: 'INTERNAL', messageKey: 'error.updateInstallFailed',
    safeDetails: ['无法打开浏览器'], retryable: true, recoveryActions: [] });
  renderWithToasts(<SettingsPage client={testClient({ checkUpdate, openReleasePage })} gateway={gateway} onNavigate={() => {}} onReopenOnboarding={() => {}} />);

  await user.click(await screen.findByRole('button', { name: '检查更新' }));

  expect(await screen.findByText(/有新版本 0.2.0/)).toBeInTheDocument();
  expect(screen.getByText(/点侧栏左上角的更新按钮/)).toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: '打开发布页' }));
  expect(openReleasePage).toHaveBeenCalledWith('https://example.test/releases/v0.2.0');
});

test('备份列表标注可能含密钥，恢复要走确认', async () => {
  const user = userEvent.setup();
  const restoreBackup = vi.fn().mockResolvedValue('/Users/example/.codex/config.toml');
  renderWithToasts(<SettingsPage client={testClient({
    listBackups: vi.fn().mockResolvedValue([{ id: 'b_1', sourcePath: '/Users/example/.codex/config.toml',
      createdAt: '2026-09-18T03:20:00Z', contentHash: 'a1b2c3d4e5f6', bytes: 412, mayContainSecrets: true }]),
    restoreBackup,
  })} gateway={gateway} onNavigate={() => {}} onReopenOnboarding={() => {}} />);

  expect(await screen.findByText('可能含密钥')).toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: '恢复' }));

  const dialog = await screen.findByRole('dialog');
  expect(within(dialog).getByText(/覆盖前会自动再备份一次当前文件/)).toBeInTheDocument();
  expect(restoreBackup).not.toHaveBeenCalled();

  await user.click(within(dialog).getByRole('button', { name: '恢复这份备份' }));
  expect(restoreBackup).toHaveBeenCalledWith('b_1');
  expect(await screen.findByText(/操作记录未回退/)).toBeInTheDocument();
});

test('危险操作的入口会跳到对应页面执行', async () => {
  const user = userEvent.setup();
  const onNavigate = vi.fn();
  renderWithToasts(<SettingsPage client={testClient()} gateway={gateway} onNavigate={onNavigate} onReopenOnboarding={() => {}} />);

  await user.click(await screen.findByRole('button', { name: '还原 Codex 配置' }));
  expect(onNavigate).toHaveBeenCalledWith('codexConfig');
  await user.click(screen.getByRole('button', { name: '查看日志' }));
  expect(onNavigate).toHaveBeenCalledWith('logs');
});

test('暂停开关取自后端返回值，不在前端自行翻转', async () => {
  const user = userEvent.setup();
  const setGatewayPaused = vi.fn().mockResolvedValue(true);
  renderWithToasts(<SettingsPage client={testClient({ setGatewayPaused })} gateway={gateway} onNavigate={() => {}} onReopenOnboarding={() => {}} />);

  await user.click(await screen.findByRole('button', { name: '暂停新请求' }));
  expect(setGatewayPaused).toHaveBeenCalledWith(true);
  // 以后端返回为准：按钮文案随之变化。
  expect(await screen.findByRole('button', { name: '继续接受新请求' })).toBeInTheDocument();
});
