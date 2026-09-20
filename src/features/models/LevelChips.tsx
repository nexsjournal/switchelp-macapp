import { useState } from 'react';
import { Plus, X } from 'lucide-react';
import styles from './LevelChips.module.css';

import { t } from '@/i18n';

/**
 * 思考档位（对齐参考界面：默认只有一个「+」）。
 *
 * 一个档位都不加 ＝ 这个模型不声明思考：核心侧就是不声明（未知），
 * 既不会投影到 Codex 菜单，也不会往请求里塞参数。加一个档位才代表「这个模型有档位」，
 * 点某个档位把它设为默认。所以默认状态下界面上只有加号，不摆一排没填的空控件。
 */
export function LevelChips({ levels, defaultLevel, busy = false, onChange }: {
  levels: string[];
  defaultLevel: string | null;
  busy?: boolean;
  onChange: (next: { levels: string[]; defaultLevel: string | null }) => void;
}) {
  const [adding, setAdding] = useState(false);

  function add(value: string) {
    const level = value.trim();
    setAdding(false);
    if (!level || levels.includes(level)) return;
    onChange({ levels: [...levels, level], defaultLevel: defaultLevel ?? level });
  }

  function remove(level: string) {
    const next = levels.filter(item => item !== level);
    onChange({ levels: next, defaultLevel: defaultLevel === level ? next[0] ?? null : defaultLevel });
  }

  return <div className={styles.levels}>
    {levels.map(level => <span key={level} className={styles.level}>
      <button type="button" className={styles.pick} disabled={busy} aria-pressed={defaultLevel === level}
        title={t('editor.levelPickHint')} onClick={() => onChange({ levels, defaultLevel: level })}>{level}</button>
      <button type="button" className={styles.remove} disabled={busy} aria-label={t('editor.removeLevel', { level })}
        onClick={() => remove(level)}><X size={12} /></button>
    </span>)}
    {adding
      ? <input autoFocus className={styles.input} maxLength={32} aria-label={t('editor.addLevel')}
          onBlur={event => add(event.target.value)}
          onKeyDown={event => {
            if (event.key === 'Enter') { event.preventDefault(); add(event.currentTarget.value); }
            // Escape 只关掉这个输入，不该冒泡出去把整页或多一层弹窗也关了。
            if (event.key === 'Escape') { event.stopPropagation(); setAdding(false); }
          }} />
      : <button type="button" className={styles.add} disabled={busy} aria-label={t('editor.addLevel')}
          onClick={() => setAdding(true)}><Plus size={14} /></button>}
  </div>;
}
