import { useState } from 'react';
import { Eye, EyeOff } from 'lucide-react';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { showToast } from '@/components/Toast';
import { t } from '@/i18n';
import styles from './GithubTokenDialog.module.css';

/**
 * 「设置 GitHub 令牌」弹窗（内容中心与插件中心共用一份令牌）。
 *
 * 从前这个入口是一个 `window.prompt`：WKWebView 要实现 `WKUIDelegate` 的输入面板才会显示，
 * 而 wry 没有实现，于是真机上它不弹任何界面、直接返回 null——用户点了按钮什么都不会发生。
 * 令牌是敏感值，所以输入框、显隐按钮与提示文案的写法照供应商表单里的密钥字段来。
 *
 * 提交空值＝清除（原代码里 `value.trim() || null` 就是这个语义）；已配置时底栏另有
 * 一个显式的「清除令牌」，省得人靠猜「留空」才知道怎么移除。
 *
 * 令牌在核心层是**一份**（`content::GITHUB_TOKEN_REF`）：内容抓取与插件目录共用，
 * 所以这个弹窗不属于任何一页——插件中心在限额报错的详情里也会直接开它。
 */
export function GithubTokenDialog({ client, configured, onSaved, onClose }: {
  client: DesktopClient;
  /** 当前是否已配置：只决定要不要给出「清除令牌」那条路径。 */
  configured: boolean;
  onSaved: (configured: boolean) => void;
  onClose: () => void;
}) {
  const [token, setToken] = useState('');
  const [visible, setVisible] = useState(false);
  const [busy, setBusy] = useState(false);

  const submit = async (value: string | null) => {
    setBusy(true);
    try {
      const next = await client.setContentGithubToken(value);
      onSaved(next);
      showToast(next ? t('github.token.saved') : t('github.token.cleared'));
      onClose();
    } catch (cause) {
      // 失败不关弹窗：人还在这里，改一下就能重试。
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    } finally { setBusy(false); }
  };

  return (
    <Dialog width="narrow" title={t('github.token.title')} busy={busy} onClose={onClose}
      footer={<footer className="form-footer">
        <div className="actions">
          {configured && <button type="button" onClick={() => void submit(null)} disabled={busy}>{t('github.token.clear')}</button>}
        </div>
        <div className="actions">
          <button type="button" onClick={onClose} disabled={busy}>{t('action.cancel')}</button>
          <button className="primary" type="button" disabled={busy} onClick={() => void submit(token.trim() || null)}>
            {busy ? t('key.saving') : t('action.save')}
          </button>
        </div>
      </footer>}>
      <div className="form-fields">
        <label><span className="field-label">{t('github.token.label')}</span>
          <span className={styles.secret}>
            <input type={visible ? 'text' : 'password'} value={token} maxLength={4096} spellCheck={false}
              autoComplete="new-password" aria-label={t('github.token.label')} className={styles.secretInput}
              placeholder={t('github.token.prompt')} autoFocus
              onChange={event => setToken(event.target.value)} />
            <button type="button" className="icon-button" aria-label={visible ? t('key.hide') : t('key.reveal')}
              onClick={() => setVisible(current => !current)}>{visible ? <EyeOff size={16} /> : <Eye size={16} />}</button>
          </span></label>
        <p className={styles.scope}>{t('github.token.scope')}</p>
      </div>
    </Dialog>
  );
}
