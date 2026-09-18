import { Cpu, Info, Network, Palette, ScrollText, ShieldAlert, Trash2, Wrench } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import type { CodexInstance } from '@/contracts/types';
import { type DesktopClient, type GatewayReport, toCoreError } from '@/desktop/client';
import styles from './SettingsPage.module.css';

/**
 * 设置页（设计 P09）。
 *
 * 原则：**能读到的真实状态就显示真值，做不到的就明说未实现**，
 * 不放一个看起来能点、实际不生效的开关。危险操作单独成组，不与日常项混在一起。
 */
export function SettingsPage({ client, gateway, onNavigate }: {
  client: DesktopClient;
  gateway: GatewayReport | null;
  onNavigate: (page: 'codexConfig' | 'logs' | 'diagnostics') => void;
}) {
  const [instances, setInstances] = useState<CodexInstance[]>([]);
  const [error, setError] = useState('');
  /** 暂停状态取自后端，不在前端自己翻转，避免与托盘菜单不一致。 */
  const [paused, setPaused] = useState(gateway?.paused ?? false);
  useEffect(() => { setPaused(gateway?.paused ?? false); }, [gateway?.paused]);

  const togglePaused = useCallback(async () => {
    setError('');
    try { setPaused(await client.setGatewayPaused(!paused)); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '切换暂停状态失败。'); }
  }, [client, paused]);

  useEffect(() => {
    let current = true;
    client.detectInstances()
      .then(found => { if (current) setInstances(found); })
      .catch(thrown => { if (current) setError(toCoreError(thrown).safeDetails.join('；') || '实例检测失败。'); });
    return () => { current = false; };
  }, [client]);

  return <div className={styles.page}>
    {error && <div className="error-message" role="alert">{error}</div>}

    <section className={styles.card}>
      <h2><Palette size={17} />外观与语言</h2>
      <dl className={styles.rows}>
        <dt>主题</dt><dd>暗色（固定）<span className="text-muted">参考截图以深色为主；浅色主题未实现</span></dd>
        <dt>语言</dt><dd>简体中文<span className="text-muted">文案层已支持多语言结构，切换入口未实现</span></dd>
        <dt>动效</dt><dd>跟随系统<span className="text-muted">已响应系统的“减少动态效果”设置</span></dd>
      </dl>
    </section>

    <section className={styles.card}>
      <h2><Network size={17} />本机网关</h2>
      <dl className={styles.rows}>
        <dt>状态</dt><dd className={gateway?.running ? styles.online : styles.offline}>
          {gateway?.running ? '运行中' : '未启动'}
          {gateway?.error && <span className="text-muted">{gateway.error}</span>}
        </dd>
        <dt>监听</dt><dd className="text-mono">{gateway?.running ? `127.0.0.1:${gateway.port}` : '—'}<span className="text-muted">只绑本机回环，不监听外部地址</span></dd>
        <dt>已发布目录</dt><dd>{gateway?.revisions.length ? `${gateway.revisions.length} 个版本` : '尚未发布'}<span className="text-muted">应用成功后发布；旧版本在仍有引用时保留</span></dd>
        <dt>已处理请求</dt><dd className="text-mono">{gateway?.served ?? 0}<span className="text-muted">本次启动以来</span></dd>
        <dt>接受新请求</dt><dd>{paused ? '已暂停' : '正常'}
          <span className="text-muted">暂停只拦新请求，在途请求继续跑完</span></dd>
        <dt>令牌指纹</dt><dd className="text-mono">{gateway?.tokenFingerprint || '—'}<span className="text-muted">每次启动重新生成；只暴露指纹便于核对，不暴露令牌</span></dd>
      </dl>
      <div className={styles.actions}>
        <button onClick={() => void togglePaused()} disabled={!gateway?.running}>
          {paused ? '继续接受新请求' : '暂停新请求'}
        </button>
      </div>
    </section>

    <section className={styles.card}>
      <h2><Cpu size={17} />Codex 实例</h2>
      {instances.length === 0
        ? <p className="text-muted">未检测到 Codex 实例。安装后到 Codex 配置页重新检测。</p>
        : <ul className={styles.instances}>{instances.map(instance => <li key={instance.id}>
          <div><strong>{instance.appPath ?? '未检测到应用路径'}</strong>
            <span className="text-mono text-muted break-anywhere">{instance.configFile}</span></div>
          <div className={styles.tags}>
            <span className="badge">{instance.compatibility === 'unverified' ? '兼容性未验证' : instance.compatibility}</span>
            {instance.configExists ? <span className="badge">配置存在</span> : <span className="badge warning">配置不存在</span>}
            {instance.conflictingManagers.length > 0 && <span className="badge warning">其他工具：{instance.conflictingManagers.join('、')}</span>}
          </div>
        </li>)}</ul>}
    </section>

    <section className={styles.card}>
      <h2><ScrollText size={17} />日志与诊断</h2>
      <dl className={styles.rows}>
        <dt>保留策略</dt><dd>7 天或 20 MiB，先到为准<span className="text-muted">后端固定策略，界面暂不提供调整</span></dd>
        <dt>收集范围</dt><dd>白名单字段 + 值级脱敏<span className="text-muted">不收集 prompt、completion、工具参数、文件内容与任何密钥</span></dd>
      </dl>
      <div className={styles.actions}>
        <button onClick={() => onNavigate('diagnostics')}>连接诊断</button>
        <button onClick={() => onNavigate('logs')}>查看日志</button>
      </div>
    </section>

    <section className={styles.card}>
      <h2><Wrench size={17} />备份与更新</h2>
      <dl className={styles.rows}>
        <dt>配置备份</dt><dd>未实现<span className="text-muted">当前依赖“还原上次配置”与原子替换保证可回退；定时备份与版本列表尚未实现</span></dd>
        <dt>应用更新</dt><dd>未实现<span className="text-muted">尚未接签名与更新通道</span></dd>
      </dl>
    </section>

    <section className={styles.card}>
      <h2><Info size={17} />关于</h2>
      <dl className={styles.rows}>
        <dt>版本</dt><dd className="text-mono">{__APP_VERSION__}</dd>
        <dt>仓库</dt><dd><a href="https://github.com/nexsjournal/gptswitch-macapp" rel="noreferrer noopener" target="_blank" className="text-mono">github.com/nexsjournal/gptswitch-macapp</a></dd>
        <dt>许可证</dt><dd>未附带开源许可证<span className="text-muted">默认保留所有权利</span></dd>
      </dl>
    </section>

    <section className={`${styles.card} ${styles.dangerZone}`}>
      <h2><ShieldAlert size={17} />危险操作</h2>
      <p className="text-muted">这些操作会改动 Codex 配置或删除本工具的数据，与其他设置分开列出。</p>
      <div className={styles.actions}>
        <button onClick={() => onNavigate('codexConfig')}>还原 Codex 配置</button>
        <button onClick={() => onNavigate('logs')}><Trash2 size={16} />清空诊断日志</button>
      </div>
      <p className={styles.note}>还原与清空都要在对应页面确认后才执行；这里只做入口，不重复实现第二套逻辑。</p>
    </section>
  </div>;
}
