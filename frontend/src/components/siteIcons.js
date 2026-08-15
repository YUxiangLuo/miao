const BRANDS = [
  {
    id: 'gemini',
    label: 'Gemini',
    suffixes: ['gemini.google.com', 'aistudio.google.com'],
    background: '#1a73e8',
    color: '#ffffff',
    path: 'M12 2.2 14.7 9.3 22 12l-7.3 2.7L12 21.8l-2.7-7.1L2 12l7.3-2.7L12 2.2zm0 4.4-1.4 3.7L6.9 12l3.7 1.4L12 17.1l1.4-3.7L17.1 12l-3.7-1.4L12 6.6z',
  },
  {
    id: 'google',
    label: 'Google',
    suffixes: ['google.com', 'gstatic.com', 'googleapis.com'],
    background: '#ffffff',
    color: '#4285F4',
    path: 'M12 11.1v2.8h6.3c-.3 1.7-1.9 4.1-6.3 4.1A6.9 6.9 0 1 1 12 5.1c1.9 0 3.2.8 3.9 1.5l2-2C16.4 3.2 14.4 2.2 12 2.2A9.8 9.8 0 1 0 12 22c5.7 0 9.5-4 9.5-9.6 0-.6-.1-1.1-.2-1.6H12z',
  },
  {
    id: 'openai',
    label: 'OpenAI',
    suffixes: ['openai.com', 'chatgpt.com'],
    background: '#10a37f',
    color: '#ffffff',
    path: 'M21.2 10.1a5.4 5.4 0 0 0-.5-4.4 5.5 5.5 0 0 0-5.9-2.6 5.5 5.5 0 0 0-9.3 1.9 5.4 5.4 0 0 0-3.6 2.6 5.5 5.5 0 0 0 .7 6.4 5.4 5.4 0 0 0 .4 4.4 5.5 5.5 0 0 0 5.9 2.6A5.4 5.4 0 0 0 13.1 22a5.5 5.5 0 0 0 5.2-3.8 5.4 5.4 0 0 0 3.6-2.6 5.5 5.5 0 0 0-.7-5.5zm-8 11.2a4 4 0 0 1-2.6-.9l.1-.1 4.3-2.5a.7.7 0 0 0 .4-.6v-6.1l1.8 1.1v5a4.1 4.1 0 0 1-4 4.1zM4.5 16.6a4 4 0 0 1-.5-2.7l4.4 2.5a.7.7 0 0 0 .7 0l5.3-3v2.1L9.4 18.2a4.1 4.1 0 0 1-4.9-1.6zM3.4 8a4 4 0 0 1 2.1-1.8v5.1a.7.7 0 0 0 .4.6l5.2 3-1.8 1.1-4.4-2.5A4.1 4.1 0 0 1 3.4 8zm15 3.5-5.3-3.1 1.8-1.1 4.4 2.5a4.1 4.1 0 0 1-.6 7.3v-5.1a.7.7 0 0 0-.3-.6zm1.8-2.7-4.3-2.5a.7.7 0 0 0-.7 0L9.4 9.3V7.2l4.4-2.5a4.1 4.1 0 0 1 6.4 4.1zM8.8 12.5l-1.8-1V6.6a4.1 4.1 0 0 1 6.7-3.1L9.1 6.1a.7.7 0 0 0-.3.6zm1-2.1 2.3-1.4 2.4 1.4v2.7l-2.4 1.4-2.3-1.4z',
  },
  {
    id: 'anthropic',
    label: 'Anthropic',
    suffixes: ['anthropic.com', 'claude.ai'],
    background: '#d4a27f',
    color: '#1a1a1a',
    path: 'M16.8 4 12 16.2 7.2 4H3l7.6 16.8h2.8L21 4z',
  },
  {
    id: 'xai',
    label: 'xAI',
    suffixes: ['x.ai', 'grok.com'],
    background: '#111111',
    color: '#ffffff',
    path: 'M12 2.4 13.6 9 20.4 8.2 15.4 12l5 4.2-6.8-.6L12 21.6 10.4 15.6 3.6 16.2 8.6 12 3.6 7.8l6.8.8z',
  },
  {
    id: 'x',
    label: 'X',
    suffixes: ['x.com', 'twitter.com', 'twimg.com'],
    background: '#111111',
    color: '#ffffff',
    path: 'M14.2 10.3 21.5 2h-2.2l-6 6.9L8.4 2H2.5l7.7 11.1L2.5 22h2.2l6.4-7.4L15.6 22h5.9zM5.6 3.5h2.2l10.6 17h-2.2z',
  },
  {
    id: 'groq',
    label: 'Groq',
    suffixes: ['groq.com'],
    background: '#f55036',
    color: '#ffffff',
    path: 'M12 3.2A8.8 8.8 0 1 0 20.8 12 8.8 8.8 0 0 0 12 3.2zm0 14.4A5.6 5.6 0 1 1 17.6 12 5.6 5.6 0 0 1 12 17.6zm3.2-8.4h1.8v6.4h-1.8z',
  },
  {
    id: 'deepseek',
    label: 'DeepSeek',
    suffixes: ['deepseek.com'],
    background: '#4d6bfe',
    color: '#ffffff',
    path: 'M5 6.5c3.6-2.4 8.2-2.2 12 .6 1.6 1.2 2.7 2.8 3.2 4.6-2.8-2.2-6.4-3.1-10-2.4C7.6 10 5.6 11.6 4.4 13.8 3.6 11.1 3.7 8.2 5 6.5zm1.6 11c3.4 2.1 7.8 2 11.1-.4-3.2 0-6.3-1.1-8.7-3.2-1.2-1-2.1-2.3-2.6-3.7-.6 2.3-.4 4.9.2 7.3z',
  },
  {
    id: 'perplexity',
    label: 'Perplexity',
    suffixes: ['perplexity.ai'],
    background: '#20808d',
    color: '#ffffff',
    path: 'M12 2.4 5.2 6.3v4.8L12 7.2l6.8 3.9V6.3zm0 6.6L5.2 13v4.7L12 21.6l6.8-3.9V13zm0 2.4 4.4 2.5v.1L12 16.5l-4.4-2.5z',
  },
  {
    id: 'huggingface',
    label: 'Hugging Face',
    suffixes: ['huggingface.co'],
    background: '#ffd21e',
    color: '#1a1a1a',
    path: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zm-3.2 7.2a1.1 1.1 0 1 1 0 2.2 1.1 1.1 0 0 1 0-2.2zm6.4 0a1.1 1.1 0 1 1 0 2.2 1.1 1.1 0 0 1 0-2.2zM8.2 14.4c.8 1.6 2.2 2.6 3.8 2.6s3-.1 3.8-2.6c.1-.3-.4-.6-.7-.4-1 .7-2 .1-3.1.1s-2.1.6-3.1-.1c-.3-.2-.8.1-.7.4z',
  },
  {
    id: 'openrouter',
    label: 'OpenRouter',
    suffixes: ['openrouter.ai'],
    background: '#6566f1',
    color: '#ffffff',
    path: 'M6.5 5.2 3 12l3.5 6.8h3.2L6.4 12l3.3-6.8zm7.8 0L11 12l3.3 6.8h3.2L14.1 12l3.4-6.8z',
  },
  {
    id: 'mistral',
    label: 'Mistral',
    suffixes: ['mistral.ai'],
    background: '#fa520f',
    color: '#ffffff',
    path: 'M4 18.5 8.2 5.5h3.1L7 18.5zm5.4 0 4.2-13h3.1l-4.3 13zm5.4 0 4.2-13H22L17.8 18.5z',
  },
  {
    id: 'github',
    label: 'GitHub',
    suffixes: ['github.com'],
    background: '#181717',
    color: '#ffffff',
    path: 'M12 2C6.5 2 2 6.6 2 12.2c0 4.5 2.9 8.3 6.9 9.6.5.1.7-.2.7-.5v-1.7c-2.8.6-3.4-1.4-3.4-1.4-.5-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 0 1.6 1 1.6 1 .9 1.6 2.4 1.1 3 .9.1-.7.4-1.1.6-1.4-2.3-.3-4.6-1.2-4.6-5.2 0-1.1.4-2.1 1.1-2.8-.1-.3-.5-1.4.1-2.9 0 0 .9-.3 2.9 1.1a9.8 9.8 0 0 1 5.2 0c2-1.4 2.9-1.1 2.9-1.1.6 1.5.2 2.6.1 2.9.7.7 1.1 1.7 1.1 2.8 0 4-2.4 4.9-4.6 5.2.4.3.7.9.7 1.9v2.8c0 .3.2.6.7.5 4-1.3 6.9-5.1 6.9-9.6C22 6.6 17.5 2 12 2z',
  },
  {
    id: 'gitlab',
    label: 'GitLab',
    suffixes: ['gitlab.com'],
    background: '#fc6d26',
    color: '#ffffff',
    path: 'm12 21.2 4.2-12.9H7.8zm8.4-12.9h-3.1L15.1 2.8a.3.3 0 0 0-.6 0L12 9.3 9.5 2.8a.3.3 0 0 0-.6 0L6.7 8.3H3.6a.4.4 0 0 0-.2.7L12 21.2l8.6-12.2a.4.4 0 0 0-.2-.7z',
  },
  {
    id: 'cursor',
    label: 'Cursor',
    suffixes: ['cursor.com', 'cursor.sh'],
    background: '#111111',
    color: '#ffffff',
    path: 'M5 4.5 19 12 5 19.5V14l7.2-2L5 10z',
  },
  {
    id: 'npm',
    label: 'npm',
    suffixes: ['npmjs.com', 'registry.npmjs.org'],
    background: '#cb3837',
    color: '#ffffff',
    path: 'M4 4h16v16H4zm3.2 3.2v9.6h4.8V9.6h3.2v7.2h1.6V7.2z',
  },
  {
    id: 'pypi',
    label: 'PyPI',
    suffixes: ['pypi.org'],
    background: '#3775a9',
    color: '#ffffff',
    path: 'M12 3c4.4 0 6.2 1.5 6.2 4.4v2.6H9.3c-3 0-4.5 1.6-4.5 4.4 0 2.7 1.5 4.2 4.5 4.2h1.4V16H9.4c-1.7 0-2.4-.6-2.4-2s.7-2.1 2.4-2.1h10.4v-4c0-3.3-2.2-5-6.8-5zm-1.1 2.2a1.2 1.2 0 1 1 0 2.4 1.2 1.2 0 0 1 0-2.4zm2.4 6.4v2.8h5.4c3 0 4.5-1.5 4.5-4.3 0-2.7-1.5-4.2-4.5-4.2h-1.6v2.8h1.5c1.7 0 2.4.6 2.4 2s-.7 2-2.4 2zm1.2 5.2a1.2 1.2 0 1 1 0 2.4 1.2 1.2 0 0 1 0-2.4z',
  },
  {
    id: 'crates',
    label: 'crates.io',
    suffixes: ['crates.io'],
    background: '#f74c00',
    color: '#ffffff',
    path: 'M4.8 7.4 12 3.4l7.2 4v9.2L12 20.6l-7.2-4zm2.4 1.4v6.4L12 18l4.8-2.8V8.8L12 6z',
  },
  {
    id: 'docker',
    label: 'Docker',
    suffixes: ['docker.com'],
    background: '#2496ed',
    color: '#ffffff',
    path: 'M4.4 11.2h2.1v2.1H4.4zm2.5 0h2.1v2.1H6.9zm2.6 0h2.1v2.1H9.5zm2.5 0h2.1v2.1h-2.1zM6.9 8.8h2.1v2.1H6.9zm2.6 0h2.1v2.1H9.5zm2.5 0h2.1v2.1h-2.1zm0-2.4h2.1v2.1h-2.1zm4.4 5.4c.3-.2.8-.5 1.4-.4.1-.8.5-1.4 1.1-1.8l-.8-.8c-.9.7-1.5 1.8-1.5 3.2 0 .1 0 .2.1.3h-8c-.8 0-1.8.4-2.4 1.1A4 4 0 0 0 4 16.4c0 .2 0 .4.1.6h13.3c1.8 0 3.2-1.2 3.2-3s-1.2-2.8-2.8-3z',
  },
  {
    id: 'cloudflare',
    label: 'Cloudflare',
    suffixes: ['cloudflare.com'],
    background: '#f38020',
    color: '#ffffff',
    path: 'M16.6 9.4c-.3-2.4-2.4-4.2-4.9-4.2-1.9 0-3.6 1.1-4.4 2.7A3.8 3.8 0 0 0 4 11.8c0 .3 0 .6.1.8h11.8c.8 0 1.5-.6 1.6-1.4.4.1.8.3 1.2.3 1.5 0 2.7-1.2 2.7-2.7 0-1.4-1.1-2.6-2.5-2.7zM5.3 14.3l1.5 4.4h1.7l.6-1.7h1.8l.5 1.7h1.8l-1.6-4.4zm2.6 1.2.5 1.5H7.4zm5.3-.1 1.2 3.3h1.7l2.4-3.3h-1.9l-1.1 1.7-.5-1.7z',
  },
  {
    id: 'vercel',
    label: 'Vercel',
    suffixes: ['vercel.com'],
    background: '#111111',
    color: '#ffffff',
    path: 'm12 4.5 9 15H3z',
  },
  {
    id: 'stackoverflow',
    label: 'Stack Overflow',
    suffixes: ['stackoverflow.com'],
    background: '#f48024',
    color: '#ffffff',
    path: 'M6.4 14.6h9.1v1.6H6.4zm.2-3.4 8.9 1.9.3-1.6-8.9-1.9zm1.2-3.2 8.2 3.8.7-1.5-8.2-3.8zM10 5l7.1 5.6 1-1.3L11 3.7zm8.3 12.8H5.6V13H4v6.4h15.9V13h-1.6z',
  },
  {
    id: 'discord',
    label: 'Discord',
    suffixes: ['discord.com'],
    background: '#5865f2',
    color: '#ffffff',
    path: 'M18.6 5.6A16 16 0 0 0 14.6 4l-.3.6a14.5 14.5 0 0 1 3.5 1.4 12.8 12.8 0 0 0-11.6 0A13 13 0 0 1 9.6 4.6L9.3 4a16 16 0 0 0-4 1.6C2.6 9.2 1.9 12.7 2.1 16.1a16.3 16.3 0 0 0 5 2.5l.6-1.1a10.6 10.6 0 0 1-1.7-.8l.4-.3a11.6 11.6 0 0 0 10.2 0l.4.3c-.5.3-1.1.6-1.7.8l.6 1.1a16.2 16.2 0 0 0 5-2.5c.3-4-.4-7.4-2.3-10.5zM9.1 14.3c-.8 0-1.5-.8-1.5-1.7s.7-1.7 1.5-1.7 1.5.8 1.5 1.7-.6 1.7-1.5 1.7zm5.8 0c-.8 0-1.5-.8-1.5-1.7s.7-1.7 1.5-1.7 1.5.8 1.5 1.7-.7 1.7-1.5 1.7z',
  },
  {
    id: 'youtube',
    label: 'YouTube',
    suffixes: ['youtube.com', 'ytimg.com', 'googlevideo.com'],
    background: '#ff0000',
    color: '#ffffff',
    path: 'M23 12.2s0-3.2-.4-4.6c-.2-.9-.9-1.6-1.8-1.8C19.2 5.4 12 5.4 12 5.4s-7.2 0-8.8.4c-.9.2-1.6.9-1.8 1.8C1 9 1 12.2 1 12.2s0 3.2.4 4.6c.2.9.9 1.6 1.8 1.8 1.6.4 8.8.4 8.8.4s7.2 0 8.8-.4c.9-.2 1.6-.9 1.8-1.8.4-1.4.4-4.6.4-4.6zM9.8 15.6V8.8l6.2 3.4z',
  },
  {
    id: 'kimi',
    label: 'Kimi',
    suffixes: ['kimi.ai', 'moonshot.cn'],
    background: '#1a1a1a',
    color: '#f4d19b',
    path: 'M15.2 4.6a7.2 7.2 0 1 0 4.2 10.4 8.4 8.4 0 0 1-4.2-10.4z',
  },
  {
    id: 'bilibili',
    label: 'Bilibili',
    suffixes: ['bilibili.com', 'hdslb.com', 'bilivideo.cn', 'bilivideo.com'],
    background: '#fb7299',
    color: '#ffffff',
    path: 'M5.2 7.2 3.4 5.4l1.4-1.4 2.2 2.2h10l2.2-2.2 1.4 1.4-1.8 1.8H19a2 2 0 0 1 2 2v8.2a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9.2a2 2 0 0 1 2-2zm.6 2.4v7.6h12.4V9.6zm3.2 1.6 2.4 1.8-2.4 1.8zm5.2 0v3.6l2.4-1.8z',
  },
  {
    id: 'reddit',
    label: 'Reddit',
    suffixes: ['reddit.com'],
    background: '#ff4500',
    color: '#ffffff',
    path: 'M14.4 3.4c.1.6.4 1.1.9 1.4 1.6.3 2.8 1.1 3.6 2.2a2.2 2.2 0 1 1 1.4 3.9c0 .3 0 .6-.1.8-1 4.4-5.2 7.5-10.2 7.5S.1 16.1.1 11.7c0-.3 0-.6.1-.8A2.2 2.2 0 1 1 1.6 7c.8-1.1 2-1.9 3.6-2.2.5-.3.8-.8.9-1.4l2.1.4c0 .4-.1.8-.3 1.1h4.2c-.2-.3-.3-.7-.3-1.1zM8.1 11.4a1.4 1.4 0 1 0 0 2.8 1.4 1.4 0 0 0 0-2.8zm7.8 0a1.4 1.4 0 1 0 0 2.8 1.4 1.4 0 0 0 0-2.8zM8.6 16.2c.8.8 1.9 1.2 3.4 1.2s2.6-.4 3.4-1.2c.2-.2 0-.5-.2-.5-1 .7-2.1 1-3.2 1s-2.2-.3-3.2-1c-.2 0-.4.3-.2.5z',
  },
]

const BRAND_LOOKUPS = BRANDS
  .flatMap((brand) => brand.suffixes.map((suffix) => ({ ...brand, suffix })))
  .sort((a, b) => b.suffix.length - a.suffix.length)

export function stripHostPort(value) {
  const trimmed = String(value || '').trim()
  if (!trimmed) return ''

  if (trimmed.startsWith('[')) {
    const end = trimmed.indexOf(']')
    if (end > 1) return trimmed.slice(1, end)
  }

  const colonCount = trimmed.split(':').length - 1
  if (colonCount === 1) {
    return trimmed.slice(0, trimmed.lastIndexOf(':'))
  }

  return trimmed.replace(/\.$/, '')
}

export function connectionDomain(connection) {
  const metadata = connection?.metadata || {}
  const raw = metadata.host
    || metadata.sniffHost
    || metadata.remoteDestination
    || metadata.destinationIP
    || metadata.destination
    || ''
  return stripHostPort(raw) || 'unknown'
}

export function normalizeDomain(domain) {
  return String(domain || '')
    .trim()
    .toLowerCase()
    .replace(/\.$/, '')
    .replace(/^www\./, '')
}

function isIpAddress(value) {
  if (/^\d{1,3}(?:\.\d{1,3}){3}$/.test(value)) return true
  return value.includes(':')
}

function letterFromDomain(domain) {
  const normalized = normalizeDomain(domain)
  const source = normalized === 'unknown' ? domain : normalized
  const match = String(source || '').match(/[a-z0-9]/i)
  return (match ? match[0] : '?').toUpperCase()
}

export function iconForDomain(domain) {
  const normalized = normalizeDomain(domain)
  if (!normalized || normalized === 'unknown' || isIpAddress(normalized)) {
    return {
      id: 'letter',
      label: domain || 'unknown',
      letter: letterFromDomain(domain),
    }
  }

  const brand = BRAND_LOOKUPS.find(({ suffix }) => (
    normalized === suffix || normalized.endsWith(`.${suffix}`)
  ))

  if (!brand) {
    return {
      id: 'letter',
      label: domain,
      letter: letterFromDomain(domain),
    }
  }

  return {
    id: brand.id,
    label: brand.label,
    background: brand.background,
    color: brand.color,
    path: brand.path,
    viewBox: '0 0 24 24',
  }
}
