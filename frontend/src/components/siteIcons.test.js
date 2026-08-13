import { describe, expect, it } from 'vitest'
import { connectionDomain, iconForDomain, normalizeDomain, stripHostPort } from './siteIcons.js'

describe('stripHostPort', () => {
  it('strips a host:port pair', () => {
    expect(stripHostPort('example.com:443')).toBe('example.com')
  })

  it('keeps a bare hostname', () => {
    expect(stripHostPort('api.github.com')).toBe('api.github.com')
  })

  it('unwraps a bracketed IPv6 address', () => {
    expect(stripHostPort('[::1]:443')).toBe('::1')
  })

  it('keeps an unbracketed IPv6 address', () => {
    expect(stripHostPort('2001:db8::1')).toBe('2001:db8::1')
  })
})

describe('connectionDomain', () => {
  it('prefers host over sniffed or IP fallbacks', () => {
    expect(connectionDomain({
      metadata: {
        host: 'api.openai.com:443',
        sniffHost: 'cdn.openai.com',
        destinationIP: '1.2.3.4',
      },
    })).toBe('api.openai.com')
  })

  it('falls back through sniff, remote destination, then IP', () => {
    expect(connectionDomain({ metadata: { sniffHost: 'github.com' } })).toBe('github.com')
    expect(connectionDomain({ metadata: { remoteDestination: '8.8.8.8' } })).toBe('8.8.8.8')
    expect(connectionDomain({ metadata: { destinationIP: '1.1.1.1' } })).toBe('1.1.1.1')
  })

  it('returns unknown when no destination is present', () => {
    expect(connectionDomain({ metadata: {} })).toBe('unknown')
  })
})

describe('iconForDomain', () => {
  it('matches known AI and developer hosts by suffix', () => {
    expect(iconForDomain('api.github.com').id).toBe('github')
    expect(iconForDomain('www.openai.com').id).toBe('openai')
    expect(iconForDomain('claude.ai').id).toBe('anthropic')
    expect(iconForDomain('cli-chat-proxy.grok.com').id).toBe('xai')
    expect(iconForDomain('gemini.google.com').id).toBe('gemini')
    expect(iconForDomain('fonts.gstatic.com').id).toBe('google')
    expect(iconForDomain('pbs.twimg.com').id).toBe('x')
    expect(iconForDomain('www.kimi.ai').id).toBe('kimi')
    expect(iconForDomain('i0.hdslb.com').id).toBe('bilibili')
  })

  it('returns a letter avatar for unknown or IP destinations', () => {
    expect(iconForDomain('obscure-lab.internal')).toMatchObject({ id: 'letter', letter: 'O' })
    expect(iconForDomain('203.0.113.10')).toMatchObject({ id: 'letter', letter: '2' })
    expect(iconForDomain('unknown')).toMatchObject({ id: 'letter', letter: 'U' })
  })

  it('normalizes www and trailing dots before matching', () => {
    expect(normalizeDomain('www.GitHub.com.')).toBe('github.com')
    expect(iconForDomain('www.github.com.')).toMatchObject({ id: 'github', label: 'GitHub' })
  })
})
