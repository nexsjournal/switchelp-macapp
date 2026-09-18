import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SettingsPage } from './SettingsPage';
import { instance, testClient } from '../../../tests/helpers/client';

const gateway = { running: true, paused: false, port: 18765, served: 12, revisions: ['rev_a'], tokenFingerprint: '3f9a1c04', error: null };

test('展示真实网关状态，未启动时给原因而不是假装正常', async () => {
  const stopped = testClient({ detectInstances: vi.fn().mockResolvedValue([]) });
  const { rerender } = render(<SettingsPage client={stopped} gateway={gateway} onNavigate={() => {}} />);

  expect(await screen.findByText('运行中')).toBeInTheDocument();
  expect(screen.getByText('127.0.0.1:18765')).toBeInTheDocument();
  expect(screen.getByText('3f9a1c04')).toBeInTheDocument();

  rerender(<SettingsPage client={stopped} gateway={{ ...gateway, running: false, port: null, error: '端口被占用' }} onNavigate={() => {}} />);
  expect(screen.getByText('未启动')).toBeInTheDocument();
  expect(screen.getByText('端口被占用')).toBeInTheDocument();
});

test('检测到的实例展示配置路径与冲突工具', async () => {
  const client = testClient({ detectInstances: vi.fn().mockResolvedValue([
    { ...instance, conflictingManagers: ['other-tool'] },
  ]) });
  render(<SettingsPage client={client} gateway={gateway} onNavigate={() => {}} />);

  expect(await screen.findByText(instance.configFile)).toBeInTheDocument();
  expect(screen.getByText(/其他工具：other-tool/)).toBeInTheDocument();
  expect(screen.getByText('配置存在')).toBeInTheDocument();
});

test('未实现的能力明说未实现，不放看不到效果的开关', async () => {
  render(<SettingsPage client={testClient()} gateway={gateway} onNavigate={() => {}} />);

  expect(await screen.findByText('备份与更新')).toBeInTheDocument();
  expect(screen.getAllByText('未实现').length).toBeGreaterThanOrEqual(2);
  // 危险操作只做入口，不在这里直接执行。
  expect(screen.getByRole('button', { name: '还原 Codex 配置' })).toBeInTheDocument();
});

test('危险操作的入口会跳到对应页面执行', async () => {
  const user = userEvent.setup();
  const onNavigate = vi.fn();
  render(<SettingsPage client={testClient()} gateway={gateway} onNavigate={onNavigate} />);

  await user.click(await screen.findByRole('button', { name: '还原 Codex 配置' }));
  expect(onNavigate).toHaveBeenCalledWith('codexConfig');
  await user.click(screen.getByRole('button', { name: '查看日志' }));
  expect(onNavigate).toHaveBeenCalledWith('logs');
});

test('暂停开关取自后端返回值，不在前端自行翻转', async () => {
  const user = userEvent.setup();
  const setGatewayPaused = vi.fn().mockResolvedValue(true);
  render(<SettingsPage client={testClient({ setGatewayPaused })} gateway={gateway} onNavigate={() => {}} />);

  await user.click(await screen.findByRole('button', { name: '暂停新请求' }));
  expect(setGatewayPaused).toHaveBeenCalledWith(true);
  // 以后端返回为准：按钮文案随之变化。
  expect(await screen.findByRole('button', { name: '继续接受新请求' })).toBeInTheDocument();
});
