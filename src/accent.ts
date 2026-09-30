import type { ResolvedTheme } from './theme';

/**
 * 主题色（强调色）。
 *
 * 难点不在存一个色号，而在**同一支色要同时干两件相反的事**：它既当底
 * （`--action-primary-bg`，上面压 `--accent-fg` 的字），又当字（链接、侧栏当前项，
 * 压在自己的淡底上）。两个方向都要过 4.5:1，于是可用亮度被夹在一个很窄的区间里，
 * 所以**不能把用户挑的色号原样写进去**：挑一支亮黄，白字压不住；挑一支深蓝，
 * 深色主题下链接又看不见。
 *
 * 做法是**只搬亮度，不动色相**：在线性光里整体乘一个系数（等价于原色变深变浅，
 * 色相与彩度原样保留），或者往白色里掺。目标亮度不是这里新定的，就是 `tokens.css`
 * 里那套**已经实测过**的青绿各档亮度。亮度定了对比度就定了，且**与色相无关**——
 * 换任何颜色进来，对比度都还是实测过的那几个数。
 *
 * 各档都从**该主题的强调色**往下推，而不是各自直接拿用户原色去推。理由是浅色主题的
 * 淡底与描边要的是「粉彩」，得从已经压暗的那支色掺白出来；拿高亮度的原色（比如纯黄）
 * 去推淡底，会推出一块荧光黄当侧栏选中底。
 *
 * 存储位置是 localStorage 的 `gptswitch.accent`，与主题、语言一致（界面偏好，不是秘密，
 * 也不参与配置事务）。存的是**用户挑的那个色号**而不是推导结果：推导是纯函数，
 * 重新算一遍就行，以后调算法也不必迁移已存的偏好。
 */

/** `default` = 内置青绿（不写任何覆盖）；其余是用户挑的色号。 */
export type AccentPreference = 'default' | `#${string}`;

export const DEFAULT_ACCENT: AccentPreference = 'default';

const STORAGE_KEY = 'gptswitch.accent';

/** 默认档的代表色：深色主题下的那支青绿。取色器在默认状态下就以它作为初值。 */
export const DEFAULT_ACCENT_BASE = '#2dd4bf';

/**
 * 内置预设。给的是**色系**代表色，落进界面时同样要过下面那套亮度搬运，
 * 所以列表里的色号与界面上看到的色块不是同一个值——不必也不该对齐。
 *
 * 八个色相覆盖整个色轮；每种在深浅两个主题下都会落到同一组对比度上（见模块头注释）。
 */
export const ACCENT_PRESETS = [
  { id: 'blue', hex: '#2563eb', labelKey: 'settings.accentPresetBlue' },
  { id: 'cyan', hex: '#0891b2', labelKey: 'settings.accentPresetCyan' },
  { id: 'green', hex: '#16a34a', labelKey: 'settings.accentPresetGreen' },
  { id: 'amber', hex: '#ca8a04', labelKey: 'settings.accentPresetAmber' },
  { id: 'orange', hex: '#ea580c', labelKey: 'settings.accentPresetOrange' },
  { id: 'red', hex: '#dc2626', labelKey: 'settings.accentPresetRed' },
  { id: 'pink', hex: '#db2777', labelKey: 'settings.accentPresetPink' },
  { id: 'violet', hex: '#7c3aed', labelKey: 'settings.accentPresetViolet' },
] as const;

/**
 * `tokens.css` 里那套青绿的取值。它们是各档亮度的**来源**——这份表与 `tokens.css`
 * 的取值必须一致（`accent.test.ts` 会去读 CSS 逐条核对），于是色号与推导规则之间
 * 不可能各自漂移。
 */
const SHIPPED: Record<ResolvedTheme, AccentRamp> = {
  dark: { accent: '#2dd4bf', accentStrong: '#14b8a6', accentFg: '#04211d', accentSubtle: '#0c3a33', accentBorder: '#1f7d70' },
  light: { accent: '#0f766e', accentStrong: '#115e59', accentFg: '#ffffff', accentSubtle: '#e3f5f1', accentBorder: '#94d3c8' },
};

/**
 * 亮色主题的描边是**粉彩**档：比纯掺白出来的更灰一档，与既有取值对齐
 * （`#94d3c8` 就是从 `#0f766e` 掺白到同一亮度再降彩度得来的）。深色主题不需要这一步。
 */
const BORDER_DESATURATE: Record<ResolvedTheme, number> = { dark: 0, light: 0.4 };

export interface AccentRamp {
  accent: string;
  accentStrong: string;
  accentFg: string;
  accentSubtle: string;
  accentBorder: string;
}

interface Anchors {
  accent: number;
  accentStrong: number;
  accentFg: number;
  accentSubtle: number;
  accentBorder: number;
}

type Rgb = { r: number; g: number; b: number };
type Linear = [number, number, number];

function srgbToLinear(value: number): number {
  const v = value / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
}

function linearToSrgb(value: number): number {
  const v = value <= 0.0031308 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055;
  return Math.round(Math.min(1, Math.max(0, v)) * 255);
}

function linearOf(rgb: Rgb): Linear {
  return [srgbToLinear(rgb.r), srgbToLinear(rgb.g), srgbToLinear(rgb.b)];
}

/** WCAG 相对亮度：线性光下的加权和，权重就是人眼对三原色的敏感度。 */
function luminanceOf(linear: Linear): number {
  return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
}

function luminance(rgb: Rgb): number {
  return luminanceOf(linearOf(rgb));
}

/**
 * 把一支色搬到指定亮度上，色相与彩度保持不变。
 *
 * - 要变暗：线性光里整体乘一个系数。这是**保持色度**的变换，深色主题的各档全靠它。
 * - 要变亮：往白色里掺。目标比自己亮时只有掺白能到，代价是彩度会降——那正是淡底该有的样子。
 */
function withLuminance(rgb: Rgb, target: number): Rgb {
  const linear = linearOf(rgb);
  const current = luminanceOf(linear);
  const next: Linear = target <= current && current > 0
    ? [linear[0] * (target / current), linear[1] * (target / current), linear[2] * (target / current)]
    : target <= 0
      // 纯黑的输入没有色相可保，任何亮度都只能是灰：交给下面掺白那支处理，
      // 这里只接住「黑的还要更黑」这一种没有解的情况。
      ? [0, 0, 0]
      : [
        linear[0] + (1 - linear[0]) * ((target - current) / (1 - current)),
        linear[1] + (1 - linear[1]) * ((target - current) / (1 - current)),
        linear[2] + (1 - linear[2]) * ((target - current) / (1 - current)),
      ];
  return { r: linearToSrgb(next[0]), g: linearToSrgb(next[1]), b: linearToSrgb(next[2]) };
}

/** 朝「同一亮度的灰」靠拢，亮度基本不变、彩度下降。 */
function desaturate(rgb: Rgb, amount: number): Rgb {
  if (amount <= 0) return rgb;
  const gray = linearToSrgb(luminance(rgb));
  const mix = (channel: number) => Math.round(channel + (gray - channel) * amount);
  return { r: mix(rgb.r), g: mix(rgb.g), b: mix(rgb.b) };
}

const HEX = /^#?([0-9a-f]{6})$/i;

/** 收下一个色号；不是 6 位十六进制就返回 null（界面据此提示，而不是猜一个色出来）。 */
export function normalizeHex(input: string): `#${string}` | null {
  const match = HEX.exec(input.trim());
  return match ? `#${match[1]!.toLowerCase()}` : null;
}

function hexToRgb(hex: string): Rgb {
  return {
    r: Number.parseInt(hex.slice(1, 3), 16),
    g: Number.parseInt(hex.slice(3, 5), 16),
    b: Number.parseInt(hex.slice(5, 7), 16),
  };
}

function toHex(rgb: Rgb): string {
  return `#${[rgb.r, rgb.g, rgb.b].map(channel => channel.toString(16).padStart(2, '0')).join('')}`;
}

function anchorsOf(theme: ResolvedTheme): Anchors {
  const shipped = SHIPPED[theme];
  return {
    accent: luminance(hexToRgb(shipped.accent)),
    accentStrong: luminance(hexToRgb(shipped.accentStrong)),
    accentFg: luminance(hexToRgb(shipped.accentFg)),
    accentSubtle: luminance(hexToRgb(shipped.accentSubtle)),
    accentBorder: luminance(hexToRgb(shipped.accentBorder)),
  };
}

const ANCHORS: Record<ResolvedTheme, Anchors> = { dark: anchorsOf('dark'), light: anchorsOf('light') };

/**
 * 把用户原色归一到「强调色本体」那一档（深色主题的亮度），作为**淡底与描边的根**。
 *
 * 这两档不能直接从用户原色推：原色可能是纯黄那种高亮度色，直接推到淡底的亮度上会得到
 * 一块荧光底（浅色主题的侧栏当前项就是这么用的）。深色主题的强调色本身是「高亮度的浅色」，
 * 拿它当根再掺白，淡底自然是粉彩，与既有青绿的取值也只差个位数。
 */
function tintRoot(base: Rgb): Rgb {
  return withLuminance(base, ANCHORS.dark.accent);
}

/**
 * 把用户挑的色号搬进当前主题的亮度档位。色相与彩度保留，只有明暗被改写——
 * 这正是「特别亮/特别浅的颜色配白字看不清」那类问题的解法。
 */
export function deriveAccentRamp(hex: string, theme: ResolvedTheme): AccentRamp {
  const base = hexToRgb(normalizeHex(hex) ?? DEFAULT_ACCENT_BASE);
  const anchor = ANCHORS[theme];
  const tint = tintRoot(base);
  return {
    accent: toHex(withLuminance(base, anchor.accent)),
    accentStrong: toHex(withLuminance(base, anchor.accentStrong)),
    // 亮色主题的字是白：它是压在深底上对比最高的那一支，不参与推导。
    accentFg: theme === 'light' ? '#ffffff' : toHex(withLuminance(base, anchor.accentFg)),
    accentSubtle: toHex(withLuminance(tint, anchor.accentSubtle)),
    accentBorder: toHex(desaturate(withLuminance(tint, anchor.accentBorder), BORDER_DESATURATE[theme])),
  };
}

/**
 * 某个选项在当前主题下**实际**会变成的颜色。界面上的色块用它，而不是用选项自己的色号——
 * 否则用户点之前看到的和点之后看到的不是同一个色。
 */
export function accentSwatch(preference: AccentPreference, theme: ResolvedTheme): string {
  if (preference === DEFAULT_ACCENT) return SHIPPED[theme].accent;
  return deriveAccentRamp(preference, theme).accent;
}

/** 当前生效的主题。`data-theme` 由 `src/theme.ts` 写入，这里只读——两个模块不互相 import。 */
function resolvedTheme(): ResolvedTheme {
  return typeof document !== 'undefined' && document.documentElement.dataset.theme === 'light' ? 'light' : 'dark';
}

export function readAccentPreference(): AccentPreference {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === DEFAULT_ACCENT) return DEFAULT_ACCENT;
    const hex = stored ? normalizeHex(stored) : null;
    if (hex) return hex;
  } catch {
    // 存储不可用（隐私模式等）时回落到默认，不影响渲染。
  }
  return DEFAULT_ACCENT;
}

/** 覆盖哪几个变量：强调色本体，以及三个别名（主按钮底/字、焦点环）。 */
const ACCENT_VARS = {
  '--accent': 'accent',
  '--accent-strong': 'accentStrong',
  '--accent-fg': 'accentFg',
  '--accent-subtle': 'accentSubtle',
  '--accent-border': 'accentBorder',
  '--action-primary-bg': 'accent',
  '--action-primary-fg': 'accentFg',
  '--focus-ring': 'accent',
} as const satisfies Record<string, keyof AccentRamp>;

/**
 * 把主题色写到根元素。
 *
 * 默认档**不写任何覆盖**：`tokens.css` 里的原值就是它。少一层覆盖，首帧与实测过的那套
 * 配色逐字节一致，回退路径也只剩「删掉内联变量」一条。
 */
export function applyAccent(preference: AccentPreference = readAccentPreference()): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  if (preference === DEFAULT_ACCENT) {
    for (const name of Object.keys(ACCENT_VARS)) root.style.removeProperty(name);
    return;
  }
  const ramp = deriveAccentRamp(preference, resolvedTheme());
  for (const [name, key] of Object.entries(ACCENT_VARS)) root.style.setProperty(name, ramp[key]);
}

/**
 * 落定一支主题色：归一化 → 写存储 → 立刻生效。
 *
 * 返回**归一化之后**的那支，界面必须用它来更新自己那份状态：挑到默认青绿时会被归到默认档，
 * 界面若还拿着原样的色号，第一个色块会显示成没选中，而实际用的就是它。
 * 归一化规则只写在这里一处，界面不复制一份。
 *
 * 参数收 `string` 而不是 `AccentPreference`：调用点拿到的是 DOM 给的字符串（`input` 的 value、
 * 预设表里的字面量），而本函数对任何输入都有定义（收不下就归默认），不必让调用点先断言一遍。
 */
export function setAccentPreference(preference: string): AccentPreference {
  // 选到默认那支色（取色器原样确认就是它）时归到默认档：否则「默认」与「自定义成同一个色」
  // 两个状态在界面上长得一模一样，用户会以为选择没生效。
  const hex = preference === DEFAULT_ACCENT ? null : normalizeHex(preference);
  const next: AccentPreference = hex === null || hex === DEFAULT_ACCENT_BASE ? DEFAULT_ACCENT : hex;
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // 写不进去只影响“下次还记得”，当前主题色照常生效。
  }
  applyAccent(next);
  return next;
}
