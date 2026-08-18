// 规则工作台「快捷选择」数据：常见应用进程与常见站点。
// 进程名按平台适配：Windows 的可执行文件带 .exe 后缀。

export const COMMON_PROCESS_APPS = [
  {
    category: '下载工具',
    apps: [
      { label: 'qBittorrent', name: 'qbittorrent' },
      { label: 'aria2', name: 'aria2c' },
      { label: 'Transmission', name: 'transmission' },
      { label: '迅雷', name: 'thunder' },
    ],
  },
  {
    category: 'AI 工具',
    apps: [
      { label: 'Codex CLI', name: 'codex' },
      { label: 'Claude Code', name: 'claude' },
      { label: 'Cursor', name: 'cursor' },
      { label: 'aider', name: 'aider' },
    ],
  },
  {
    category: '浏览器',
    apps: [
      { label: 'Chrome', name: 'chrome', windowsName: 'chrome.exe' },
      { label: 'Firefox', name: 'firefox', windowsName: 'firefox.exe' },
      { label: 'Edge', name: 'msedge', windowsName: 'msedge.exe' },
    ],
  },
  {
    category: '开发 / 运维',
    apps: [
      { label: 'Docker', name: 'docker' },
      { label: 'git', name: 'git' },
      { label: 'ssh', name: 'ssh' },
      { label: 'curl', name: 'curl' },
      { label: 'wget', name: 'wget' },
      { label: 'Node.js', name: 'node' },
      { label: 'Python', name: 'python' },
      { label: 'nginx', name: 'nginx' },
    ],
  },
  {
    category: '游戏 / 娱乐',
    apps: [
      { label: 'Steam', name: 'steam', windowsName: 'steam.exe' },
      { label: 'Epic Games', name: 'epicgameslauncher', windowsName: 'EpicGamesLauncher.exe' },
    ],
  },
]

export const COMMON_DOMAIN_SITES = [
  'openai.com',
  'google.com',
  'youtube.com',
  'github.com',
  'netflix.com',
  'bilibili.com',
  'wikipedia.org',
  'cloudflare.com',
]

// 进程名的平台适配：优先显式 windowsName，否则 Linux 名 + .exe
export function processNameFor(app, platform = 'linux') {
  if (platform === 'windows') {
    return app.windowsName || `${app.name}.exe`
  }
  return app.name
}
