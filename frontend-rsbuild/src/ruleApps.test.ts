import { describe, expect, it } from '@rstest/core'
import { COMMON_DOMAIN_SITES, COMMON_PROCESS_APPS } from './ruleApps'

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



  it('offers common domain presets', () => {
    expect(COMMON_DOMAIN_SITES.length).toBeGreaterThanOrEqual(6)
    for (const site of COMMON_DOMAIN_SITES) {
      expect(site).toMatch(/^[a-z0-9.-]+\.[a-z]{2,}$/)
    }
  })
})
