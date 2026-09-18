import { Cpu, Info, Network, Palette, ScrollText, ShieldAlert, Trash2, Wrench } from 'lucide-react';
import { Dialog } from '@/components/Dialog';
import { useCallback, useEffect, useState } from 'react';
import type { CodexInstance } from '@/contracts/types';
import { type BackupEntry, type DesktopClient, type GatewayReport, type UpdateReport, toCoreError } from '@/desktop/client';
import { applyTheme, readThemePreference, setThemePreference, type ThemePreference } from '@/theme';
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
  const [busy, setBusy] = useState('');
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [previewText, setPreviewText] = useState('');
  const [update, setUpdate] = useState<UpdateReport | null>(null);
  const [restore, setRestore] = useState<BackupEntry | null>(null);
  const [notice, setNotice] = useState('');
  /** 暂停状态取自后端，不在前端自己翻转，避免与托盘菜单不一致。 */
  const [preference, setPreference] = useState<ThemePreference>(() => readThemePreference());
  const [resolved, setResolved] = useState(() => applyTheme(readThemePreference()));
  const applyPreference = (next: ThemePreference) => { setPreference(next); setResolved(setThemePreference(next)); };
  const [paused, setPaused] = useState(gateway?.paused ?? false);
  useEffect(() => { setPaused(gateway?.paused ?? false); }, [gateway?.paused]);

  const togglePaused = useCallback(async () => {
    setError('');
    try { setPaused(await client.setGatewayPaused(!paused)); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '切换暂停状态失败。'); }
  }, [client, paused]);

  const loadBackups = useCallback(async () => {
    try { setBackups(await client.listBackups()); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '读取备份列表失败。'); }
  }, [client]);

  useEffect(() => { void loadBackups(); }, [loadBackups]);

  async function run(label: string, work: () => Promise<void>) {
    setBusy(label); setError(''); setNotice('');
    try { await work(); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '操作失败。'); }
    finally { setBusy(''); }
  }

  const checkUpdate = () => run('update', async () => { setUpdate(await client.checkUpdate()); });

  const createBackup = () => run('backup', async () => {
    await client.createBackup(instances[0]!.id);
    setNotice('已创建配置备份。');
    await loadBackups();
  });

  const showPreview = (entry: BackupEntry) => run('preview', async () => { setPreviewText(await client.previewBackup(entry.id)); });

  const restoreBackup = (entry: BackupEntry) => run('restore', async () => {
    const target = await client.restoreBackup(entry.id);
    setRestore(null);
    setNotice(`已恢复 ${target}；恢复前的状态也已自动备份。事务记录未回退，可到 Codex 配置页重新生成差异。`);
    await loadBackups();
  });

  useEffect(() => {
    let current = true;
    client.detectInstances()
      .then(found => { if (current) setInstances(found); })
      .catch(thrown => { if (current) setError(toCoreError(thrown).safeDetails.join('；') || '实例检测失败。'); });
    return () => { current = false; };
  }, [client]);

  return <div className={styles.page}>
    {error && <div className="error-message" role="alert">{error}</div>}
    {notice && <div className={styles.notice} role="status">{notice}</div>}

    <section className={styles.card}>
      <h2><Palette size={17} />外观与语言</h2>
      <dl className={styles.rows}>
        <dt>主题</dt><dd>
          <select aria-label="主题" value={preference} onChange={event => applyPreference(event.target.value as ThemePreference)}>
            <option value="dark">暗色</option>
            <option value="light">亮色</option>
            <option value="system">跟随系统</option>
          </select>
          <span className="text-muted">当前生效：{resolved === 'dark' ? '暗色' : '亮色'}；跟随系统时系统切换会立即生效</span>
        </dd>
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
        <dt>自动备份</dt><dd>每次提交前<span className="text-muted">写 Codex 配置之前先留一份原样副本；备份失败就不提交</span></dd>
        <dt>保留策略</dt><dd>最近 20 份<span className="text-muted">超出后从最旧的开始清理，只清理本工具自己的备份</span></dd>
        <dt>更新</dt><dd>
          {update?.error ? <span className={styles.offline}>查询失败：{update.error}<small>查询不到不会显示成“已是最新”</small></span>
            : update?.hasUpdate ? <span className={styles.online}>有新版本 {update.latest}<small>当前 {update.current}；本工具不自动下载安装</small></span>
            : update ? <span>已是最新（{update.current}）</span>
            : <span className="text-muted">尚未检查</span>}
        </dd>
      </dl>
      <div className={styles.actions}>
        <button onClick={() => void checkUpdate()} disabled={busy === 'update'}>{busy === 'update' ? '检查中…' : '检查更新'}</button>
        {update?.hasUpdate && update.releaseUrl && <a className={styles.linkButton} href={update.releaseUrl} target="_blank" rel="noreferrer noopener">打开发布页</a>}
        <button onClick={() => void createBackup()} disabled={busy === 'backup' || !instances.length}>立即备份配置</button>
      </div>

      <h3 className={styles.subHeading}>最近的备份</h3>
      {backups.length === 0
        ? <p className="text-muted">还没有备份。首次应用配置时会自动创建一份。</p>
        : <ul className={styles.backups}>{backups.slice(0, 8).map(item => <li key={item.id}>
          <div>
            <strong className="text-mono">{item.createdAt}</strong>
            <span className="text-muted text-mono break-anywhere">{item.contentHash.slice(0, 12)} · {item.bytes} 字节</span>
          </div>
          {item.mayContainSecrets && <span className="badge warning">可能含密钥</span>}
          <div className="actions">
            <button onClick={() => void showPreview(item)}>遮罩预览</button>
            <button className="danger" onClick={() => setRestore(item)}>恢复</button>
          </div>
        </li>)}</ul>}
      {previewText && <textarea className={styles.summary} readOnly aria-label="备份遮罩预览" rows={6} value={previewText} />}
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

    {restore && <Dialog title="恢复这份备份" busy={busy === 'restore'}
      description={`将把 ${restore.sourcePath} 覆盖为 ${restore.createdAt} 的备份内容。覆盖前会自动再备份一次当前文件，所以这一步本身也可以回退。`}
      onClose={() => setRestore(null)}>
      <div className="form-fields"><div className="form-footer">
        <span>本工具的事务记录不会跟着回退。</span>
        <div className="actions">
          <button onClick={() => setRestore(null)} disabled={busy === 'restore'}>取消</button>
          <button className="danger" autoFocus disabled={busy === 'restore'} onClick={() => void restoreBackup(restore)}>
            {busy === 'restore' ? '恢复中…' : '恢复这份备份'}
          </button>
        </div>
      </div></div>
    </Dialog>}
  </div>;
}
