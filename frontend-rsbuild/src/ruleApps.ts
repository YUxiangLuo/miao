// 规则工作台「快捷选择」数据：常见应用进程与常见站点。

export interface ProcessApp {
  label: string
  name: string
}

export interface ProcessAppCategory {
  category: string
  apps: ProcessApp[]
}

export const COMMON_PROCESS_APPS: ProcessAppCategory[] = [
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
      { label: 'Chrome', name: 'chrome' },
      { label: 'Firefox', name: 'firefox' },
      { label: 'Edge', name: 'msedge' },
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
      { label: 'Steam', name: 'steam' },
    ],
  },
]

export const COMMON_DOMAIN_SITES: string[] = [
  'openai.com',
  'google.com',
  'youtube.com',
  'github.com',
  'netflix.com',
  'bilibili.com',
  'wikipedia.org',
  'cloudflare.com',
]
