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
