import { useId } from 'react';

/**
 * 工具清单里 v1 没有、v5 有的两家品牌图标，SVG 路径本地内联。
 *
 * **来源与许可**：路径取自 @lobehub/icons v5.21.0
 * （https://github.com/lobehub/lobe-icons，MIT）。
 * 只搬这两个品牌的 SVG 数据、**不把 v5 加进依赖**：v5 的 peerDependencies 要求
 * React 19 + antd 6 + @lobehub/ui 5，而本项目跑在 React 18 的 v1 这条版本线上；
 * 为两个图标升整条依赖线不划算，所以只取同一份 MIT 素材里需要的那几条路径。
 *
 * **opencode 是单色**：v5 里这个品牌只有 `Mono`（没有 Color 变体）——它本来就是
 * 一色的方形终端框，currentColor 继承文字色。
 * **openclaw 是彩色**：v5 里官方有 `Color` 版（红色爪痕 + 青色眼珠），按
 * 「有官方彩色的用彩色」沿用；渐变端点与眼珠色原样搬过来，不自己调色。
 *
 * 图标只做识别（aria-hidden 的装饰位），工具名是旁边的正文文字。
 */

/** opencode 的方形终端框（v5 中该品牌只有单色版，单路径）。 */
export function OpenCodeIcon({ size = 18 }: { size?: number }) {
  return (
    <svg
      width={size} height={size} viewBox="0 0 24 24" fill="currentColor" fillRule="evenodd"
      xmlns="http://www.w3.org/2000/svg" aria-hidden="true" focusable="false"
    >
      <path d="M16 6H8v12h8V6zm4 16H4V2h16v20z" />
    </svg>
  );
}

/** openclaw 的爪痕标记（v5 中该品牌官方有彩色版）。 */
export function OpenClawIcon({ size = 18 }: { size?: number }) {
  /*
   * 彩色版靠渐变填色，渐变必须带 id；同一页出现两个实例时 id 不能撞车。
   * React 的 useId 自带冒号，而 SVG 的 url(#…) 引用对冒号的宽容度各家引擎不一，去掉更稳。
   */
  const uid = useId().replace(/:/g, '');
  const body = `${uid}-body`;
  const leftClaw = `${uid}-left`;
  const rightClaw = `${uid}-right`;
  return (
    <svg
      width={size} height={size} viewBox="0 0 24 24"
      xmlns="http://www.w3.org/2000/svg" aria-hidden="true" focusable="false"
    >
      <path
        d="M12 2.568c-6.33 0-9.495 5.275-9.495 9.495 0 4.22 3.165 8.44 6.33 9.494v2.11h2.11v-2.11s1.055.422 2.11 0v2.11h2.11v-2.11c3.165-1.055 6.33-5.274 6.33-9.494S18.33 2.568 12 2.568z"
        fill={`url(#${body})`}
      />
      <path
        d="M3.56 9.953C.396 8.898-.66 11.008.396 13.118c1.055 2.11 3.164 1.055 4.22-1.055.632-1.477 0-2.11-1.056-2.11z"
        fill={`url(#${leftClaw})`}
      />
      <path
        d="M20.44 9.953c3.164-1.055 4.22 1.055 3.164 3.165-1.055 2.11-3.164 1.055-4.22-1.055-.632-1.477 0-2.11 1.056-2.11z"
        fill={`url(#${rightClaw})`}
      />
      <path
        d="M5.507 1.875c.476-.285 1.036-.233 1.615.037.577.27 1.223.774 1.937 1.488a.316.316 0 01-.447.447c-.693-.693-1.279-1.138-1.757-1.361-.475-.222-.795-.205-1.022-.069a.317.317 0 01-.326-.542zM16.877 1.913c.58-.27 1.14-.323 1.616-.038a.317.317 0 01-.326.542c-.227-.136-.547-.153-1.022.069-.478.223-1.064.668-1.756 1.361a.316.316 0 11-.448-.447c.714-.714 1.36-1.218 1.936-1.487z"
        fill="#FF4D4D"
      />
      <path
        d="M8.835 9.109a1.266 1.266 0 100-2.532 1.266 1.266 0 000 2.532zM15.165 9.109a1.266 1.266 0 100-2.532 1.266 1.266 0 000 2.532z"
        fill="#050810"
      />
      <path
        d="M9.046 8.16a.527.527 0 100-1.056.527.527 0 000 1.055zM15.376 8.16a.527.527 0 100-1.055.527.527 0 000 1.054z"
        fill="#00E5CC"
      />
      <defs>
        <linearGradient gradientUnits="userSpaceOnUse" id={body} x1="-.659" x2="27.023" y1=".458" y2="22.855">
          <stop stopColor="#FF4D4D" />
          <stop offset="1" stopColor="#991B1B" />
        </linearGradient>
        <linearGradient gradientUnits="userSpaceOnUse" id={leftClaw} x1="0" x2="4.311" y1="9.672" y2="14.949">
          <stop stopColor="#FF4D4D" />
          <stop offset="1" stopColor="#991B1B" />
        </linearGradient>
        <linearGradient gradientUnits="userSpaceOnUse" id={rightClaw} x1="19.385" x2="24.399" y1="9.953" y2="14.462">
          <stop stopColor="#FF4D4D" />
          <stop offset="1" stopColor="#991B1B" />
        </linearGradient>
      </defs>
    </svg>
  );
}
