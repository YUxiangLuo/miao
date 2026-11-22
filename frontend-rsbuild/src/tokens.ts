// 供 JS 逻辑使用的基础常量（虚拟滚动行高、折叠计算宽度、断点、动效时长等）。

export const ICON = { xs: 13, sm: 15, md: 16, lg: 18 }
export const LOGO_SIZE = { topbar: 30, onboarding: 44 }

// 桌面/移动端分界宽度
export const CONNECTIONS_MODAL_MIN_WIDTH = 841

// 主题存储键及 PWA theme-color 兜底值（运行时优先读取 CSS --bg）
export const THEME_KEY = 'harbor-theme'
export const THEME_META = { light: '#F7F6F3', dark: '#111113' }

export const ARRIVE_MS = 480
export const SCAN_STAGGER_CAP = 8

export const FLIP_MS = 260
export const FLIP_EASING = 'cubic-bezier(0.22, 1, 0.36, 1)'

export const PATH_ICON_CELL = 48
export const HOME_SITE_CARD_WIDTH = 176
export const HOME_SITE_CARD_GAP = 10
export const HOME_CONNECTIONS_MORE_WIDTH = 56

export const CONNECTION_ROW_HEIGHT = 60
export const CONNECTION_ROW_GAP = 6
