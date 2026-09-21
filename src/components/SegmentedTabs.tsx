import styles from './SegmentedTabs.module.css';

export interface SegmentedTab {
  id: string;
  label: string;
  /** 可选的计数徽章，例如「已安装 2」。 */
  count?: number;
}

/**
 * 分段切换器（一组互斥视图）。
 *
 * 为什么是一个共用组件：以前插件中心与内容中心各写了一遍下划线式页签，两处的
 * 圆角、内边距和选中样式都不一样；更糟的是下划线压在全局 `button` 的圆角上，
 * 选中态看起来像「下面两个角是圆的」。统一成一个胶囊分段控件之后：
 *
 * - 选中态复用侧栏导航的选中样式（`--accent-subtle` 底 + `--accent` 字 + `--accent-border` 边），
 *   同一套「这里是当前的」在全应用里只有一种画法；
 * - 高度由 token 推出来：容器 3px 内边距 + 32 的控件高度 + 2px 边框 = 40，和标准控件等高，
 *   放进任何一行控件里都不用额外对齐。
 *
 * 语义仍是 `tablist` / `tab`：它真的是互斥视图切换，不是装饰性按钮组。
 */
export function SegmentedTabs({ tabs, active, onChange, ariaLabel }: {
  tabs: SegmentedTab[];
  active: string;
  onChange: (id: string) => void;
  ariaLabel: string;
}) {
  return (
    <div className={styles.tabs} role="tablist" aria-label={ariaLabel}>
      {tabs.map(tab => (
        <button
          key={tab.id}
          type="button"
          role="tab"
          aria-selected={tab.id === active}
          className={tab.id === active ? styles.active : undefined}
          onClick={() => onChange(tab.id)}
        >
          {tab.label}
          {typeof tab.count === 'number' && tab.count > 0 && <span className={styles.count}>{tab.count}</span>}
        </button>
      ))}
    </div>
  );
}
