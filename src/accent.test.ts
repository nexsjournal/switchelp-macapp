import { readFileSync } from 'node:fs';

import {
  ACCENT_PRESETS,
  DEFAULT_ACCENT,
  DEFAULT_ACCENT_BASE,
  applyAccent,
  deriveAccentRamp,
  readAccentPreference,
  setAccentPreference,
  type AccentRamp,
} from './accent';
import type { ResolvedTheme } from './theme';

/**
 * 主题色的不变量。
 *
 * 这一组断言守的不是「函数返回了什么」，而是产品对用户的那条承诺：**换任何一支色，
 * 界面上的对比度都还是 tokens.css 里那套青绿实测过的那几个数**。推导规则本身可以改，
 * `tokens.css` 里的取值也可以调，但两者之间的关系（下面第一节）与最终的对比度
 * （第二节）必须始终成立——否则「选什么色都看得清」就成了没依据的说法。
 *
 * 相对亮度与对比度公式在这里**自己写了一份**（第二节用），不从 `accent.ts` 里借：
 * 那边也没有导出亮度函数，而且借过来就等于让同一套口径同时当运动员和裁判，
 * 亮度算错了会连断言一起算错，测了个寂寞。这一节是本文件的主要价值。
 */

const THEMES: ResolvedTheme[] = ['dark', 'light'];

const SAMPLE_HEXES = [
  ...ACCENT_PRESETS.map(preset => preset.hex),
  // 取色器能选到的边界：纯黄（亮到白字压不住）、纯白与纯黑（没有色相可保）、
  // 中灰（彩度为零）、品红（色相在色轮另一侧）。前三个正是这套算法存在的理由。
  '#ffff00', '#ffffff', '#000000', '#808080', '#ff00ff',
  // 比目标档位还深的原色：推导要走「变亮」那一支。掺白那一支会把它们洗成中性灰
  // （实测 #3b0a12 会变成 #c0bdbd），这三支专门守「变亮也得认得出色相」。
  '#3b0a12', '#0a2a10', '#1a0033',
];

/* ------------------------------------------------------------------ *
 * 独立实现的 WCAG 对比度
 * ------------------------------------------------------------------ */

/** sRGB 通道值（0-255）→ 线性光。分段函数来自 WCAG 2.x 对 sRGB 的定义。 */
function srgbToLinear(value: number): number {
  const v = value / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
}

function channels(hex: string): [number, number, number] {
  return [1, 3, 5].map(offset => Number.parseInt(hex.slice(offset, offset + 2), 16)) as [number, number, number];
}

/** WCAG 相对亮度：线性光下 0.2126 / 0.7152 / 0.0722 的加权和（人眼对三原色的敏感度）。 */
function luminance(hex: string): number {
  const [r, g, b] = channels(hex);
  return 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b);
}

/** 两个色号的对比度，与谁深谁浅无关。 */
function contrast(a: string, b: string): number {
  const first = luminance(a);
  const second = luminance(b);
  const [high, low] = first >= second ? [first, second] : [second, first];
  return (high + 0.05) / (low + 0.05);
}

function channelDiff(a: string, b: string): number {
  const left = channels(a);
  const right = channels(b);
  return Math.max(...left.map((value, index) => Math.abs(value - right[index]!)));
}

/** 色相角（0-360）。彩度为零的灰没有色相，返回 null。 */
function hue(hex: string): number | null {
  const [r, g, b] = channels(hex);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const span = max - min;
  if (span === 0) return null;
  const raw = max === r ? ((g - b) / span) % 6 : max === g ? (b - r) / span + 2 : (r - g) / span + 4;
  return (raw * 60 + 360) % 360;
}

/** 两个色相角的角距，取短边。 */
function hueDistance(a: number, b: number): number {
  const diff = Math.abs(a - b) % 360;
  return Math.min(diff, 360 - diff);
}

/**
 * WCAG AA 正文标准 4.5:1。
 *
 * 余量只留 0.02：档位亮度是浮点搬出来的，贴线的那几处（浅色最紧 4.531）会因为末位舍入
 * 在 4.5 上下抖一点点。余量放大到 0.05 就盖得住一次真实回归了——真出现一支色掉到 4.47，
 * 断言照样绿。
 */
const AA = 4.5;
const AA_SLACK = 0.02;

interface Measurement {
  label: string;
  ratio: number;
}

/** 把一项保证在全部取样色号上跑一遍。失败时给出「哪支色、差多少」，而不是只说没满足。 */
function expectAtLeastAA(measurements: Measurement[]): void {
  const weak = measurements
    .filter(row => row.ratio < AA - AA_SLACK)
    .map(row => `${row.label} 实测 ${row.ratio.toFixed(3)}`);
  expect(weak).toEqual([]);
}

/* ------------------------------------------------------------------ *
 * 从 tokens.css 读那套实测过的取值
 * ------------------------------------------------------------------ */

/**
 * 与推导结果对照的是 `accent.ts` 里那份手抄的 `SHIPPED` 表，它自称「与 tokens.css 一致」，
 * 但抄写这个动作本身没有任何机制保证两边不漂移。下面就是那个机制：默认代表色跑一遍推导，
 * 结果必须落在 CSS 里那套实测过的取值上。改算法忘改 CSS（或反过来）时，这条会先响。
 *
 * 先把注释剥掉：强调色那几段注释里满是说明文字与示例色号，不剥会把注释里的数字当成声明。
 *
 * 路径按仓库根写（vitest 的工作目录），与 `i18n.test.ts` 扫源码的方式一致：
 * 那里也是拿相对路径读源文件。
 */
const TOKENS_CSS = readFileSync('src/styles/tokens.css', 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');

/** 按选择器关键字取出一个规则块的内容。 */
function cssBlock(selector: string): string {
  const blocks = [...TOKENS_CSS.matchAll(/([^{}]+)\{([^{}]*)\}/g)].filter(match => match[1]!.includes(selector));
  expect(blocks, `tokens.css 里应当有且只有一个 ${selector} 块`).toHaveLength(1);
  return blocks[0]![2]!;
}

const SELECTOR: Record<ResolvedTheme, string> = { dark: "[data-theme='dark']", light: "[data-theme='light']" };
const CSS_BLOCK: Record<ResolvedTheme, string> = { dark: cssBlock(SELECTOR.dark), light: cssBlock(SELECTOR.light) };

/** 从规则块里读一条色值声明。 */
function declaration(body: string, name: string): string {
  const match = new RegExp(`(?:^|[;{\\s])${name}\\s*:\\s*(#[0-9a-fA-F]{6})\\s*;`).exec(body);
  expect(match, `tokens.css 里找不到 ${name} 的色值声明`).not.toBeNull();
  return match![1]!.toLowerCase();
}

/** 变量名 → 推导结果里的档位。别名三个也列在这里，它们是同一支色的不同用途。 */
const TIER_VARS = {
  '--accent': 'accent',
  '--accent-strong': 'accentStrong',
  '--accent-fg': 'accentFg',
  '--accent-subtle': 'accentSubtle',
  '--accent-border': 'accentBorder',
} as const satisfies Record<string, keyof AccentRamp>;

const SHIPPED_TOKENS = Object.fromEntries(
  THEMES.map(theme => [
    theme,
    Object.fromEntries(
      Object.entries(TIER_VARS).map(([name, tier]) => [tier, declaration(CSS_BLOCK[theme], name)]),
    ) as Record<keyof AccentRamp, string>,
  ]),
) as Record<ResolvedTheme, Record<keyof AccentRamp, string>>;

/**
 * 强调色当字时压得**最紧**的那一层常规底：
 * - 浅色主题的强调色是深色，底越暗对比越低，最暗的常规底是 `--bg-selected`（侧栏当前项、选中行）；
 * - 深色主题反过来，强调色是浅色，最亮的常规底是 `--bg-elevated`（浮起卡片、弹层）。
 *
 * 从 CSS 里读而不是写死：这两个灰阶一旦调亮，这条保证就得连同实测数字一起重算，
 * 不能悄悄失效（下面有一条专门钉住这两个值）。
 */
const TIGHTEST_BACKDROP: Record<ResolvedTheme, string> = { dark: '--bg-elevated', light: '--bg-selected' };

/* ------------------------------------------------------------------ *
 * 一、推导规则与 token 源不能各自漂移
 * ------------------------------------------------------------------ */

describe('与 tokens.css 的一致性', () => {
  test('那两层最紧的底就是注释里写的那两个值，底一变就得回来重算', () => {
    // 这两个值同时出现在 tokens.css 的注释里与本节断言里：不钉住的话，
    // 灰阶悄悄调亮、而对比度断言还按旧底算，就会在真机上出现刚好看不清的文字。
    expect(declaration(CSS_BLOCK.dark, '--bg-elevated')).toBe('#1d2127');
    expect(declaration(CSS_BLOCK.light, '--bg-selected')).toBe('#eaeaef');
  });

  for (const theme of THEMES) {
    test(`${theme} 主题：默认档推出来的五个档位与 tokens.css 的取值逐通道对齐`, () => {
      const derived = deriveAccentRamp(DEFAULT_ACCENT_BASE, theme);
      for (const [name, tier] of Object.entries(TIER_VARS)) {
        const diff = channelDiff(derived[tier], SHIPPED_TOKENS[theme][tier]);
        // 20 是容差，不是目标：它是「人工调过」的额度。
        // 深色 --accent-strong 差 18 是**已知且有意**的——那一档是手工加过彩度的
        // （#14b8a6 比纯搬亮度得到的更饱和），其余各档实测 0-9。
        // 这条断言的目的是抓住「换了 token 名 / 改了算法 / 抄错了数」这类漂移，
        // 不是要求推导结果与人工取值逐字节相等。
        expect(
          diff,
          `${theme} 的 ${name}：推导 ${derived[tier]}，tokens.css ${SHIPPED_TOKENS[theme][tier]}，最大通道差 ${diff}`,
        ).toBeLessThanOrEqual(20);
      }
    });
  }

  test('强调色的三个别名与本体同值，改本体不会漏掉按钮底与焦点环', () => {
    const aliases = {
      '--action-primary-bg': '--accent',
      '--action-primary-fg': '--accent-fg',
      '--focus-ring': '--accent',
    } as const;
    for (const theme of THEMES) {
      const body = CSS_BLOCK[theme];
      for (const [alias, source] of Object.entries(aliases)) {
        expect(declaration(body, alias), `${theme}: ${alias} 应当就是 ${source}`).toBe(declaration(body, source));
      }
    }
  });
});

/* ------------------------------------------------------------------ *
 * 二、对比度保证（本文件的核心）
 * ------------------------------------------------------------------ */

describe('对比度保证：选任何颜色都不会出现看不清的字', () => {
  for (const theme of THEMES) {
    test(`${theme} 主题：字压在强调底上（主按钮、开关滑块、更新胶囊）都过 AA`, () => {
      expectAtLeastAA(
        SAMPLE_HEXES.map(hex => {
          const ramp = deriveAccentRamp(hex, theme);
          return { label: `${hex} → ${ramp.accent} 上压 ${ramp.accentFg}`, ratio: contrast(ramp.accentFg, ramp.accent) };
        }),
      );
    });

    test(`${theme} 主题：强调色当字压在自己的淡底上（侧栏当前项、选中 chip）都过 AA`, () => {
      expectAtLeastAA(
        SAMPLE_HEXES.map(hex => {
          const ramp = deriveAccentRamp(hex, theme);
          return { label: `${hex} → ${ramp.accent} 压在 ${ramp.accentSubtle}`, ratio: contrast(ramp.accent, ramp.accentSubtle) };
        }),
      );
    });

    test(`${theme} 主题：强调色当字压在本主题最亮的底上（最紧的一处）都过 AA`, () => {
      const backdrop = declaration(CSS_BLOCK[theme], TIGHTEST_BACKDROP[theme]);
      expectAtLeastAA(
        SAMPLE_HEXES.map(hex => {
          const { accent } = deriveAccentRamp(hex, theme);
          return { label: `${hex} → ${accent} 压在 ${TIGHTEST_BACKDROP[theme]} ${backdrop}`, ratio: contrast(accent, backdrop) };
        }),
      );
    });
  }
});

/* ------------------------------------------------------------------ *
 * 三、推导的纯函数性质
 * ------------------------------------------------------------------ */

describe('推导的纯函数性质', () => {
  test('同一输入算两次结果相同，中间穿插别的调用也不受影响', () => {
    for (const theme of THEMES) {
      for (const hex of SAMPLE_HEXES) {
        const first = deriveAccentRamp(hex, theme);
        // 穿插一次别的主题 + 别的色：以后若为性能加入「记住上一次结果」的缓存，
        // 键只用主题或只用色号的那种写错会在这里现形。
        deriveAccentRamp(hex === DEFAULT_ACCENT_BASE ? '#ff00ff' : DEFAULT_ACCENT_BASE, theme === 'dark' ? 'light' : 'dark');
        expect(deriveAccentRamp(hex, theme), `${hex} / ${theme}`).toEqual(first);
      }
    }
  });

  test('五个档位永远是合法的 #rrggbb，包括色号不合法时', () => {
    const HEX6 = /^#[0-9a-f]{6}$/;
    for (const theme of THEMES) {
      for (const hex of [...SAMPLE_HEXES, 'not-a-color']) {
        for (const [tier, value] of Object.entries(deriveAccentRamp(hex, theme))) {
          expect(value, `${hex} / ${theme} / ${tier}`).toMatch(HEX6);
        }
      }
    }
  });

  test('色号不合法时回落到默认代表色，而不是抛异常', () => {
    const fallback = deriveAccentRamp(DEFAULT_ACCENT_BASE, 'dark');
    for (const bad of ['not-a-color', '#12345', '#1234567', '#gggggg', 'rgb(1, 2, 3)', '']) {
      expect(() => deriveAccentRamp(bad, 'dark'), `${bad} 不应抛错`).not.toThrow();
      expect(deriveAccentRamp(bad, 'dark'), `${bad} 应回落到 ${DEFAULT_ACCENT_BASE}`).toEqual(fallback);
    }
  });

  test('黄色进来还是黄：只搬亮度，不能变成灰或别的色系', () => {
    for (const theme of THEMES) {
      const { accent } = deriveAccentRamp('#ffff00', theme);
      const [r, g, b] = channels(accent);
      // 黄 = 红绿高、蓝低。变成灰（r≈g≈b，蓝不再低）或补色（蓝反而最高）都会在这里失败。
      expect(b, `${theme} 主题下 #ffff00 → ${accent}，蓝通道 ${b} 相对 ${Math.min(r, g)} 不够低`).toBeLessThanOrEqual(
        Math.min(r, g) - 60,
      );
      // 红绿相近才是黄，偏成红或绿就是换了色系。
      expect(Math.abs(r - g), `${theme} 主题下 #ffff00 → ${accent}，红绿相差 ${Math.abs(r - g)}`).toBeLessThanOrEqual(24);
    }
  });

  test('预设色号在两套主题下都保住原色相，色系不漂移', () => {
    for (const theme of THEMES) {
      for (const { id, hex } of ACCENT_PRESETS) {
        const { accent } = deriveAccentRamp(hex, theme);
        const moved = hueDistance(hue(hex)!, hue(accent)!);
        // 额度只留 3°：实测十六个组合最大 0.5°（抬明度那一支走的是固定色相线，变暗那一支
        // 是线性光缩放，两者都保持色度，剩下的偏移只来自 8 位取整）。放到 15° 就太松了——
        // 「红变橙」那种真的漂移也能溜过去。
        expect(moved, `${theme} 主题 ${id}：${hex} → ${accent}，色相偏移 ${moved.toFixed(1)}°`).toBeLessThanOrEqual(3);
      }
    }
  });

  test('比目标档位还深的原色：变亮之后仍认得出色相，不能被洗成灰', () => {
    /*
     * 掺白那一支会把深色洗成中性灰——`#3b0a12`（深玫红）在深色主题下会变成 `#c0bdbd`，
     * 彩度从 49 掉到 3，看着就是一块灰。强调色本体因此改走「抬明度、保持相对饱和度」
     * 那一支，这条断言守的就是那支：色相几乎不动，彩度必须留住。
     */
    const deep = [
      { hex: '#3b0a12', minChroma: 60, note: '深玫红' },
      { hex: '#0a2a10', minChroma: 85, note: '深绿' },
      { hex: '#1a0033', minChroma: 65, note: '深紫' },
    ];
    for (const theme of THEMES) {
      for (const { hex, minChroma, note } of deep) {
        const { accent } = deriveAccentRamp(hex, theme);
        const [r, g, b] = channels(accent);
        const spread = Math.max(r, g, b) - Math.min(r, g, b);
        expect(spread, `${theme} 主题 ${note} ${hex} → ${accent}，彩度只剩 ${spread}`).toBeGreaterThanOrEqual(minChroma);
        expect(hueDistance(hue(hex)!, hue(accent)!), `${theme} 主题 ${note} ${hex} → ${accent} 色相漂移`).toBeLessThanOrEqual(3);
      }
    }
  });
});

/* ------------------------------------------------------------------ *
 * 四、持久化
 * ------------------------------------------------------------------ */

describe('偏好持久化', () => {
  test('存取往返：写进去的色号原样读回来，大小写归一成小写', () => {
    setAccentPreference('#2563EB');
    // 键名是与用户机器上的既有数据之间的契约：改名等于把所有人选过的色丢掉。
    expect(localStorage.getItem('gptswitch.accent')).toBe('#2563eb');
    expect(readAccentPreference()).toBe('#2563eb');
  });

  test('选到默认那支色就归默认档，不存成一条「自定义」', () => {
    // 否则「默认」与「自定义成同一个色」两个状态在界面上长得一模一样，用户会以为没生效。
    setAccentPreference(DEFAULT_ACCENT_BASE);
    expect(localStorage.getItem('gptswitch.accent')).toBe('default');
    expect(readAccentPreference()).toBe(DEFAULT_ACCENT);
  });

  test('存储里是垃圾值或别的键的取值时回落到默认，而不是抛错', () => {
    localStorage.setItem('gptswitch.theme', 'light');
    localStorage.setItem('gptswitch.accent', 'light'); // 主题键的取值，不是色号
    expect(readAccentPreference()).toBe(DEFAULT_ACCENT);

    localStorage.setItem('gptswitch.accent', 'garbage');
    expect(readAccentPreference()).toBe(DEFAULT_ACCENT);

    localStorage.setItem('gptswitch.accent', '#12345'); // 少一位
    expect(readAccentPreference()).toBe(DEFAULT_ACCENT);

    localStorage.setItem('gptswitch.accent', 'default'); // 默认档自身那一档
    expect(readAccentPreference()).toBe(DEFAULT_ACCENT);

    localStorage.removeItem('gptswitch.accent'); // 从没选过
    expect(readAccentPreference()).toBe(DEFAULT_ACCENT);
  });

  test('存储不可用时回落默认，写不进去也不影响当前主题色生效', () => {
    const read = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
    expect(readAccentPreference()).toBe(DEFAULT_ACCENT);
    read.mockRestore();

    const write = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('blocked'); });
    document.documentElement.dataset.theme = 'dark';
    expect(() => setAccentPreference('#2563eb')).not.toThrow();
    // 写不进去只影响「下次还记得」，这一趟该上的色还得上。
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe(deriveAccentRamp('#2563eb', 'dark').accent);
    write.mockRestore();
  });
});

/* ------------------------------------------------------------------ *
 * 五、写到根元素
 * ------------------------------------------------------------------ */

/**
 * `applyAccent` 该写哪些变量、各取哪一档。这里再写一遍是有意的：
 * 别名写错档（比如焦点环拿成 accent-subtle）不会崩、只会变成一支别的颜色，
 * 只有把「哪个变量 = 哪一档」显式断言出来才看得见。
 */
const VAR_SOURCES = {
  ...TIER_VARS,
  '--action-primary-bg': 'accent',
  '--action-primary-fg': 'accentFg',
  '--focus-ring': 'accent',
} as const satisfies Record<string, keyof AccentRamp>;

describe('写到根元素', () => {
  beforeEach(() => {
    localStorage.clear();
    // 内联变量与 data-theme 都会影响 applyAccent 的结果，每条用例从干净的根元素开始。
    document.documentElement.removeAttribute('style');
    delete document.documentElement.dataset.theme;
  });

  for (const theme of THEMES) {
    test(`${theme} 主题下 applyAccent 写的八个变量与推导结果一致`, () => {
      document.documentElement.dataset.theme = theme;
      applyAccent('#2563eb');
      const ramp = deriveAccentRamp('#2563eb', theme);
      for (const [name, tier] of Object.entries(VAR_SOURCES)) {
        expect(document.documentElement.style.getPropertyValue(name), `${theme} 的 ${name}`).toBe(ramp[tier]);
      }
    });
  }

  test('默认档不写覆盖：applyAccent(default) 之后内联变量全部被移除', () => {
    document.documentElement.dataset.theme = 'dark';
    applyAccent('#2563eb');
    expect(document.documentElement.style.getPropertyValue('--accent')).not.toBe('');

    applyAccent(DEFAULT_ACCENT);
    // 默认档靠「删掉内联变量」而不是「不写」：少一层覆盖，首帧与实测过的 tokens.css 逐字节一致，
    // 回退路径也只有一条。只删一部分（例如漏了别名）会在这里失败。
    for (const name of Object.keys(VAR_SOURCES)) {
      expect(document.documentElement.style.getPropertyValue(name), `${name} 应当被移除`).toBe('');
    }
  });
});
