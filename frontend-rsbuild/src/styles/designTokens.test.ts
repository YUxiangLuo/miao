import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from '@rstest/core'

const STYLES_DIR = resolve(process.cwd(), 'src/styles')
const CSS_FILES = ['theme.css', 'shell.css', 'components.css', 'views.css'] as const

function parseRootTokens(css: string): Record<string, string> {
  const rootMatch = /:root\s*\{([\s\S]*?)\n\}/.exec(css)
  if (!rootMatch) throw new Error('Missing :root block in theme.css')
  const map: Record<string, string> = {}
  const propRe = /--([a-z0-9-]+)\s*:\s*([^;]+);/gi
  let match: RegExpExecArray | null
  while ((match = propRe.exec(rootMatch[1])) !== null) {
    map[`--${match[1]}`] = match[2].trim()
  }
  return map
}

function hexToRgb(hex: string): [number, number, number] {
  const clean = hex.replace(/^#/, '').trim()
  const full = clean.length === 3
    ? clean.split('').map((c) => c + c).join('')
    : clean
  if (!/^[0-9a-f]{6}$/i.test(full)) {
    throw new Error(`Expected 6-digit hex color, got "${hex}"`)
  }
  return [
    Number.parseInt(full.slice(0, 2), 16),
    Number.parseInt(full.slice(2, 4), 16),
    Number.parseInt(full.slice(4, 6), 16),
  ]
}

function relativeLuminance([r, g, b]: [number, number, number]): number {
  const channel = (v: number) => {
    const s = v / 255
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

function contrastRatio(fgHex: string, bgHex: string): number {
  const l1 = relativeLuminance(hexToRgb(fgHex))
  const l2 = relativeLuminance(hexToRgb(bgHex))
  const lighter = Math.max(l1, l2)
  const darker = Math.min(l1, l2)
  return (lighter + 0.05) / (darker + 0.05)
}

describe('Harbor Industrial Light Design System Contracts', () => {
  const themeCss = readFileSync(resolve(STYLES_DIR, 'theme.css'), 'utf8')
  const tokens = parseRootTokens(themeCss)

  it('eliminates all dark-mode blocks and enforces single Industrial Light color-scheme', () => {
    expect(themeCss).toContain('color-scheme: light;')
    for (const file of CSS_FILES) {
      const content = readFileSync(resolve(STYLES_DIR, file), 'utf8')
      expect(content).not.toMatch(/\[data-theme\s*=\s*["']dark["']\]/i)
      expect(content).not.toMatch(/prefers-color-scheme\s*:\s*dark/i)
    }
  })

  it('satisfies WCAG 2.1 AA (>= 4.5:1) text contrast across all light surfaces and soft semantic badges', () => {
    const lightSurfaces = [
      '--color-canvas',
      '--color-surface',
      '--color-surface-subtle',
      '--color-surface-hover',
      '--color-surface-active',
    ] as const

    const textTokens = [
      '--color-foreground',
      '--color-muted-foreground',
      '--color-subtle-foreground',
    ] as const

    for (const surfaceKey of lightSurfaces) {
      const bg = tokens[surfaceKey]
      for (const textKey of textTokens) {
        const fg = tokens[textKey]
        const ratio = contrastRatio(fg, bg)
        expect(ratio).toBeGreaterThanOrEqual(4.5)
      }
    }

    // Semantic foregrounds on standard surfaces and their matching soft tinted surfaces
    const semanticPairs: ReadonlyArray<[string, string]> = [
      ['--color-primary', '--color-surface'],
      ['--color-primary', '--color-canvas'],
      ['--color-primary', '--color-primary-soft'],
      ['--color-primary-foreground', '--color-primary'],
      ['--color-success', '--color-surface'],
      ['--color-success', '--color-success-soft'],
      ['--color-warning', '--color-surface'],
      ['--color-warning', '--color-warning-soft'],
      ['--color-destructive', '--color-surface'],
      ['--color-destructive', '--color-destructive-soft'],
      ['--color-destructive-foreground', '--color-destructive'],
    ]

    for (const [fgKey, bgKey] of semanticPairs) {
      const ratio = contrastRatio(tokens[fgKey], tokens[bgKey])
      expect(ratio).toBeGreaterThanOrEqual(4.5)
    }
  })

  it('satisfies WCAG 1.4.11 (>= 3.0:1) non-text contrast for interactive control borders and focus rings', () => {
    const bgKeys = ['--color-canvas', '--color-surface', '--color-surface-subtle'] as const
    const controlKeys = [
      '--color-border-control',
      '--color-border-strong',
      '--color-focus-ring',
    ] as const

    for (const bgKey of bgKeys) {
      for (const ctrlKey of controlKeys) {
        const ratio = contrastRatio(tokens[ctrlKey], tokens[bgKey])
        expect(ratio).toBeGreaterThanOrEqual(3.0)
      }
    }
  })

  it('maintains a strictly monotonic 9-step z-index ladder with floating popovers above dialogs & drawers', () => {
    const ladder = [
      '--z-sticky',
      '--z-header-popover',
      '--z-fullscreen',
      '--z-sheet',
      '--z-dialog',
      '--z-lightbox',
      '--z-floating',
      '--z-tooltip',
      '--z-toast',
    ] as const

    const values = ladder.map((key) => {
      const n = Number.parseInt(tokens[key], 10)
      expect(Number.isFinite(n)).toBe(true)
      return n
    })

    for (let i = 1; i < values.length; i++) {
      expect(values[i]).toBeGreaterThan(values[i - 1])
    }
  })

  it('enforces a strict minimum 12px (0.75rem) typography floor across all stylesheets', () => {
    const fontSizeRe = /font-size\s*:\s*([^;]+);/gi
    for (const file of CSS_FILES) {
      const content = readFileSync(resolve(STYLES_DIR, file), 'utf8')
      let match: RegExpExecArray | null
      while ((match = fontSizeRe.exec(content)) !== null) {
        const raw = match[1].trim()
        const pxMatch = /^(\d+(?:\.\d+)?)px$/i.exec(raw)
        if (pxMatch) {
          expect(Number.parseFloat(pxMatch[1])).toBeGreaterThanOrEqual(12)
        }
        const remMatch = /^(\d+(?:\.\d+)?)rem$/i.exec(raw)
        if (remMatch) {
          expect(Number.parseFloat(remMatch[1])).toBeGreaterThanOrEqual(0.75)
        }
      }
    }
  })
})
