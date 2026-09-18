/**
 * 产品标记。
 *
 * 字形与 `assets/app-logo.svg` 同一段路径，但不带底色——底色和前景交给容器与
 * `currentColor`，侧栏那块方片才能跟着主题反相（打包图标里它始终是黑底白字）。
 * viewBox 收到字形本身的外接框，否则 1024 画布里那圈留白会让标记显得偏小。
 */
export function AppLogo({ size = 20 }: { size?: number }) {
  return <svg width={size} height={size} viewBox="170 170 684 684" fill="currentColor" aria-hidden="true" focusable="false">
    <path d="M180.854 765.456L331.469 614.841L413.165 636.732L322.356 727.541L451.871 762.244L632.036 582.079L250.461 479.836L550.238 180.06L843.146 258.544L692.531 409.159L610.835 387.268L701.644 296.459L572.129 261.756L391.963 441.921L773.539 544.164L473.762 843.941L180.854 765.456Z" />
  </svg>;
}
