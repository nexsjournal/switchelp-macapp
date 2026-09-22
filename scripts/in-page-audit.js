/**
 * 页面内审计函数。用法（在 mcp__node_repl__js 里）：
 *
 *   const src = await (await import('node:fs/promises')).readFile(
 *     'scripts/in-page-audit.js', 'utf8');
 *   const audit = await tab.playwright.evaluate(`(${src})(${JSON.stringify(rootSelector)})`);
 *
 * rootSelector 传 CSS 选择器：整页用 `'main'`，弹窗用 `'[role="dialog"]'`。
 * 返回的都是实测数字，报告里可以直接引用；不要凭它没报就宣称某个数字合格——
 * 它只覆盖它检查的那几类问题。
 */
(rootSel) => {
  const root = document.querySelector(rootSel);
  if (!root) return { error: 'root not found: ' + rootSel };

  // ---- 颜色工具：按 alpha 合成后的真实底色算对比度 ----
  const parse = c => {
    const m = /rgba?\(([^)]+)\)/.exec(c || '');
    if (!m) return null;
    const p = m[1].split(/[,\s/]+/).filter(Boolean).map(Number);
    return { r: p[0], g: p[1], b: p[2], a: p.length > 3 ? p[3] : 1 };
  };
  const over = (fg, bg) => ({
    r: fg.r * fg.a + bg.r * (1 - fg.a),
    g: fg.g * fg.a + bg.g * (1 - fg.a),
    b: fg.b * fg.a + bg.b * (1 - fg.a),
    a: 1,
  });
  const luminance = c => {
    const f = v => { v /= 255; return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); };
    return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b);
  };
  const contrast = (a, b) => {
    const l1 = luminance(a), l2 = luminance(b);
    return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
  };
  /** 逐层向上合成背景色，直到不透明。半透明底会让 token 名骗人，所以必须真算。 */
  const backgroundOf = el => {
    let node = el, acc = null;
    while (node && node !== document.documentElement) {
      const c = parse(getComputedStyle(node).backgroundColor);
      if (c && c.a > 0) acc = acc ? over(acc, c) : c;
      if (acc && acc.a >= 1) return acc;
      node = node.parentElement;
    }
    return acc ?? parse(getComputedStyle(document.documentElement).backgroundColor) ?? { r: 0, g: 0, b: 0, a: 1 };
  };

  const name = el => el.tagName.toLowerCase()
    + (typeof el.className === 'string' && el.className ? '.' + el.className.replace(/_[a-z0-9]+$/i, '') : '');

  /** 只跳过不参与视觉判定的东西；被 aria-hidden 或 visually-hidden 的也不算。 */
  const skip = el => {
    const s = getComputedStyle(el);
    return s.display === 'none' || s.visibility === 'hidden'
      || el.closest('[aria-hidden="true"], .visually-hidden')
      || ['svg', 'path', 'rect', 'circle', 'line', 'polyline', 'g', 'use'].includes(el.tagName);
  };
  const box = el => el.getBoundingClientRect();
  const visible = el => { const b = box(el); return b.width > 0.5 && b.height > 0.5; };

  const overlap = [], overflow = [], touching = [], clipped = [], contrast_ = [], targets = [], fonts = {};

  // ---- 1. 横向溢出：子元素越出父容器，且父容器不滚动 = 会被裁掉 ----
  for (const el of root.querySelectorAll('*')) {
    if (skip(el) || !visible(el)) continue;
    if (getComputedStyle(el).overflowX !== 'visible') continue;
    const parent = el.parentElement;
    if (!parent) continue;
    const b = box(el), pb = box(parent);
    if (b.right > pb.right + 2) overflow.push({ el: name(el), over: Math.round(b.right - pb.right), parent: name(parent) });
    if (b.left < pb.left - 2 && getComputedStyle(parent).paddingLeft === '0px') overflow.push({ el: name(el), over: -Math.round(pb.left - b.left), parent: name(parent), side: 'left' });
  }

  // ---- 2. 被祖先裁掉：祖先 overflow 不是 visible，且元素越出祖先边界 ----
  //
  // 只报**不可达**的裁切：祖先如果是可滚动的（auto / scroll），越出去的内容滚一下就能看到，
  // 那是滚动区在正常工作（弹窗正文、表格横向滚动都靠它）。真正的问题是把内容锁在
  // `overflow: hidden` 后面——那才是看不见也够不着。
  const scrollable = (value) => value === 'auto' || value === 'scroll';
  for (const el of root.querySelectorAll('*')) {
    if (skip(el) || !visible(el)) continue;
    let p = el.parentElement;
    while (p && p !== root) {
      const s = getComputedStyle(p);
      if (s.overflow !== 'visible' || s.overflowX !== 'visible') {
        const b = box(el), pb = box(p);
        const cut = Math.max(pb.top - b.top, b.bottom - pb.bottom);
        const reachable = (cut > 0 && scrollable(s.overflowY)) || (cut <= 0 && scrollable(s.overflowX));
        if (cut > 2 && !reachable) clipped.push({ el: name(el), by: name(p), cut: Math.round(cut) });
        break;
      }
      p = p.parentElement;
    }
  }

  // ---- 3. 相邻兄弟：真重叠 或 紧贴到 0 间距 ----
  const TABLE_ROWS = ['TR', 'THEAD', 'TBODY', 'TFOOT'];
  // 绝对/固定定位的元素不参与重叠判定（见 SKILL.md 的噪音清单）：它们本来就是浮层，
  // 与兄弟的矩形相交是浮层的定义（字段内叠加的显隐按钮、菜单、Toast 都属此类）。
  const floating = (el) => ['absolute', 'fixed'].includes(getComputedStyle(el).position);
  for (const parent of root.querySelectorAll('*')) {
    const kids = [...parent.children].filter(k => !skip(k) && visible(k) && !floating(k));
    for (let i = 1; i < kids.length; i++) {
      const a = box(kids[i - 1]), b = box(kids[i]);
      const v = Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top);
      const h = Math.min(a.right, b.right) - Math.max(a.left, b.left);
      if (v > 2 && h > 2) overlap.push({ a: name(kids[i - 1]), b: name(kids[i]), v: Math.round(v), h: Math.round(h) });
      else if (Math.abs(a.bottom - b.top) < 0.5 && h > 2 && a.height > 4 && b.height > 4
        && !TABLE_ROWS.includes(kids[i].tagName)) {
        // 「紧贴」要看**内容**是否也贴住了：相邻盒子之间只要有内边距或一条分隔线，
        // 视觉上就是分开的（滚动区与固定底栏、<details> 的 summary 与展开体都靠这个）。
        // 只报既没有内边距、也没有边框的那种贴死。
        const sa = getComputedStyle(kids[i - 1]), sb = getComputedStyle(kids[i]);
        const padded = parseFloat(sb.paddingTop) >= 8 || parseFloat(sa.paddingBottom) >= 8;
        const bordered = parseFloat(sb.borderTopWidth) > 0 || parseFloat(sa.borderBottomWidth) > 0;
        if (!padded && !bordered) touching.push({ a: name(kids[i - 1]), b: name(kids[i]) });
      }
    }
  }

  // ---- 4. 对比度与点击目标 ----
  const seen = new Set();
  for (const el of root.querySelectorAll('*')) {
    if (skip(el) || !visible(el)) continue;
    const s = getComputedStyle(el);
    fonts[s.fontSize] = (fonts[s.fontSize] ?? 0) + 1;

    const ownText = [...el.childNodes].filter(n => n.nodeType === 3).map(n => n.textContent.trim()).join('');
    if (ownText) {
      const fg = parse(s.color);
      if (fg) {
        const bg = backgroundOf(el);
        const cr = contrast(over(fg, bg), bg);
        const fs = parseFloat(s.fontSize);
        const bold = parseInt(s.fontWeight, 10) >= 700;
        const min = (fs >= 24 || (fs >= 18.66 && bold)) ? 3 : 4.5;
        const key = `${s.color}|${s.fontSize}|${name(el)}`;
        if (cr < min && !seen.has(key)) {
          seen.add(key);
          contrast_.push({
            text: ownText.slice(0, 24), el: name(el), color: s.color, fontSize: fs,
            background: `rgb(${Math.round(bg.r)},${Math.round(bg.g)},${Math.round(bg.b)})`,
            ratio: Number(cr.toFixed(2)), required: min,
          });
        }
      }
    }

    if (el.matches('button, a, input, select, textarea, [role="button"], [role="menuitem"], [role="tab"], [role="switch"]')) {
      // Switch 规范就是 36×20（docs/design/03-components.md），不受 32 方形约束。
      if (el.getAttribute('role') === 'switch') continue;
      // 勾选框图形本身是 18，点击区由外层（label / 点击层）撑到 32：
      // 外层够大就不算不合格（规范要求的是「点击区」而不是图形尺寸）。
      if (el.tagName === 'INPUT') {
        const wrap = el.parentElement;
        if (wrap && box(wrap).width >= 32 && box(wrap).height >= 32) continue;
      }
      const b = box(el);
      // 规范：图标点击区 ≥32×32，紧凑控件高 32。低于此值列入。
      if (b.width < 32 || b.height < 32) {
        targets.push({ el: name(el), w: Math.round(b.width), h: Math.round(b.height), label: (el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 20) });
      }
    }
  }

  return {
    root: rootSel,
    theme: document.documentElement.dataset.theme,
    viewport: { w: innerWidth, h: innerHeight },
    horizontalScroll: document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    fontsUsed: fonts,
    h1: [...root.querySelectorAll('h1')].map(h => h.textContent.trim()),
    positiveTabindex: [...root.querySelectorAll('[tabindex]')].filter(e => Number(e.getAttribute('tabindex')) > 0).length,
    iconButtonsWithoutLabel: [...root.querySelectorAll('button')].filter(b => !b.textContent.trim() && !b.getAttribute('aria-label')).length,
    columnHeadersWithoutScope: [...root.querySelectorAll('th')].filter(th => th.getAttribute('scope') !== 'col').length,
    overlap, overflow, touching, clipped,
    contrast: contrast_,
    targets: targets.slice(0, 25),
    targetCount: targets.length,
  };
}
