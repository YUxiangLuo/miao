import { describe, expect, it } from 'vitest'
import { COMMON_DOMAIN_SITES, COMMON_PROCESS_APPS, processNameFor } from './ruleApps'

describe('ruleApps', () => {
  it('offers categorized common apps with labels and names', () => {
    expect(COMMON_PROCESS_APPS.length).toBeGreaterThanOrEqual(4)
    for (const group of COMMON_PROCESS_APPS) {
      expect(group.category).toBeTruthy()
      expect(group.apps.length).toBeGreaterThan(0)
      for (const app of group.apps) {
        expect(app.label).toBeTruthy()
        expect(app.name).toBeTruthy()
      }
    }
  })

  it('keeps linux names on linux', () => {
    const qb = COMMON_PROCESS_APPS[0].apps[0]
    expect(processNameFor(qb, 'linux')).toBe('qbittorrent')
  })

  it('appends .exe on windows, honoring explicit windowsName', () => {
    const qb = COMMON_PROCESS_APPS[0].apps[0]
    expect(processNameFor(qb, 'windows')).toBe('qbittorrent.exe')

    const chrome = COMMON_PROCESS_APPS.flatMap((g) => g.apps).find((a) => a.label === 'Chrome')
    if (!chrome) throw new Error('Chrome app fixture missing')
    expect(processNameFor(chrome, 'windows')).toBe('chrome.exe')
    expect(processNameFor(chrome, 'linux')).toBe('chrome')
  })

  it('offers common domain presets', () => {
    expect(COMMON_DOMAIN_SITES.length).toBeGreaterThanOrEqual(6)
    for (const site of COMMON_DOMAIN_SITES) {
      expect(site).toMatch(/^[a-z0-9.-]+\.[a-z]{2,}$/)
    }
  })
})
