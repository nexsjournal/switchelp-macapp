import { CheckCell, CheckCells } from '@/components/CheckCell';
import styles from './LevelChips.module.css';

import { t } from '@/i18n';

/**
 * 思考档位（对齐参考界面：一排可勾选的档位 + 一个默认档位）。
 *
 * 两件事分开问，因为它们是两件事：
 * - **勾选**＝声明这个模型支持哪些档位。一个都不勾表示这个模型不声明思考：
 *   核心侧就是不声明（未知），既不会投影到 Codex 菜单，也不会往请求里塞参数。
 * - **默认档位**＝没有指定档位的那次调用用哪个。它必须是勾选出来的一项，
 *   所以只在勾了至少一项之后才出现。
 *
 * 档位是**从 Codex 支持的集合里选**，不是自由文本（用户原话：「正常是有自己选的那几种吧，
 * 而不是只能手输」）。手输的代价不只是难填：档位会被投影进 Codex 的模型目录，写一个
 * 它不认识的值，轻则这一档在宿主里没有标签，重则整个目录条目被拒。集合见 policy.ts。
 *
 * 已保存的档位如果不在集合里（老数据、或上游自己扩展的值）不会被抹掉：
 * 它们原样显示在末尾，仍可取消勾选。丢掉别人已声明的档位比少一个选项严重得多。
 */
export function LevelChips({ levels, defaultLevel, presets, busy = false, onChange }: {
  levels: string[];
  defaultLevel: string | null;
  /** 可选集合，从低到高。由调用方传入（policy.ts 的 REASONING_LEVEL_PRESETS）。 */
  presets: readonly { value: string; label: string }[];
  busy?: boolean;
  onChange: (next: { levels: string[]; defaultLevel: string | null }) => void;
}) {
  const order = presets.map(item => item.value);
  const extras = levels.filter(level => !order.includes(level));

  /** 落进策略的集合按档位从低到高排：界面上勾选的先后顺序不该写进策略。 */
  function commit(next: string[], pick: string | null) {
    const ordered = [...order.filter(value => next.includes(value)), ...next.filter(value => !order.includes(value))];
    onChange({ levels: ordered, defaultLevel: pick && ordered.includes(pick) ? pick : ordered[0] ?? null });
  }

  /**
   * 勾选/取消一个档位。
   *
   * 默认档位不在这里挑：没有默认时交给 `commit` 的统一规则（取已勾选里最低的一档），
   * 于是「勾了什么」和「默认是哪个」两件事各有各的、能一句话说清的规则，
   * 不依赖「先勾了哪一个」这种看不见的状态。取消的恰好是默认那一档时同理，落回最低的一档。
   */
  function toggle(value: string, checked: boolean) {
    commit(checked ? [...levels, value] : levels.filter(level => level !== value),
      defaultLevel === value && !checked ? null : defaultLevel);
  }

  return <div className={styles.levels}>
    <CheckCells>
      {presets.map(preset => <CheckCell key={preset.value} label={preset.label}
        hint={t('editor.levelPresetHint', { value: preset.value })}
        checked={levels.includes(preset.value)} disabled={busy}
        onChange={next => toggle(preset.value, next)} />)}
      {extras.map(level => <CheckCell key={level} label={level}
        hint={t('editor.levelExtraHint')} checked disabled={busy}
        onChange={next => toggle(level, next)} />)}
    </CheckCells>

    {levels.length > 0 && <div className={styles.defaultRow}>
      <span className={styles.defaultLabel}>{t('editor.levelDefault')}</span>
      <div className={styles.picks} role="group" aria-label={t('editor.levelDefault')}>
        {levels.map(level => <button key={level} type="button" disabled={busy} aria-pressed={defaultLevel === level}
          title={t('editor.levelPickHint')} className={styles.pick}
          onClick={() => commit(levels, level)}>{labelOf(level, presets)}</button>)}
      </div>
    </div>}
  </div>;
}

/** 只对已知档位翻译；已保存的自定义值原样显示——它就是发给上游的那个字符串。 */
function labelOf(value: string, presets: readonly { value: string; label: string }[]): string {
  return presets.find(preset => preset.value === value)?.label ?? value;
}
