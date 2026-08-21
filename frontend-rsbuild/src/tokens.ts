// 设计 token 的 JS 镜像：CSS var 无法进入 JS 的角落统一从这里取数。
// 本文件与 styles/tokens.css、styles/responsive.css 双向绑定——改任何一边，
// 必须同步另一边（注释互指）。

// lucide 图标尺寸四档（对应 tokens.css 音阶约定：只取这四档）
export const ICON = { xs: 12, sm: 14, md: 16, lg: 18 }

// LogoIcon 使用场景尺寸
export const LOGO_SIZE = { topbar: 64, onboarding: 40 }

// 与 styles/responsive.css 的 840px 断点对齐：小于该宽度视为移动端。
// （@media 无法引用 CSS var，断点在 CSS 侧为硬编码，这里做 JS 镜像）
export const CONNECTIONS_MODAL_MIN_WIDTH = 841

// 主题：localStorage 键 + PWA theme-color（与 tokens.css 的 --bg 双主题值同步）
export const THEME_KEY = 'miao-theme'
export const THEME_META = { dark: '#1a1d22', light: '#ffffff' }

// 节点切换到位脉冲时长（与 tokens.css 的 --anim-arrive 同步）：
// 动画结束 ≈ React 移除 .arrive class，交棒给 .active 的 tileGlow 呼吸。
export const ARRIVE_MS = 900

// 站点卡入场交错序号 --i 的上限（与 tokens.css 的 --stagger-step 配合）。
export const STAGGER_CAP = 12

// 节点 tile 扫描入场交错序号 --i 的上限（与 tokens.css 的 --stagger-step-scan 配合）：
// 末位 tile 96ms 内起步，大订阅下整列 ~0.3s 扫完，不会等数秒才出现。
export const SCAN_STAGGER_CAP = 8

// 连接行 FLIP 归位时长（WAAPI 只进 JS；与 tokens.css 的 --dur-slow 同步，改任何一边必须同步）
export const FLIP_MS = 200

// 通道卡 favicon 单格宽（tokens.css 的 --size-path-icon-lg 48px + .path-icons 的
// gap --space-2 10px）：JS 侧用它算「单行放得下几个」，改任何一边必须同步。
export const PATH_ICON_CELL = 58

// 首页活跃链接条带：站点卡 144px、卡间距 10px、溢出入口 48px。
// 与 styles/tokens.css、styles/layout.css 对应几何保持同步。
export const HOME_SITE_CARD_WIDTH = 144
export const HOME_SITE_CARD_GAP = 10
export const HOME_CONNECTIONS_MORE_WIDTH = 48
