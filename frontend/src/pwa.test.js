import { describe, it, expect } from 'vitest'
import { readFileSync, existsSync } from 'node:fs'
import { join } from 'node:path'

const root = join(import.meta.dirname, '..')
const publicDir = join(root, 'public')

describe('PWA 资源', () => {
  it('index.html 链接 manifest 并注册 service worker', () => {
    const html = readFileSync(join(root, 'index.html'), 'utf8')

    expect(html).toContain('<link rel="manifest" href="/manifest.webmanifest"')
    expect(html).toContain('name="theme-color"')
    expect(html).toContain("serviceWorker.register('/sw.js')")
    // dev 服务器下 /sw.js 不存在，注册失败必须静默
    expect(html).toContain('.catch(() => {})')
  })

  it('manifest 满足 Chrome 可安装门槛', () => {
    const manifest = JSON.parse(readFileSync(join(publicDir, 'manifest.webmanifest'), 'utf8'))

    expect(manifest.name).toBeTruthy()
    expect(manifest.display).toBe('standalone')
    expect(manifest.start_url).toBe('/')

    const icons = manifest.icons.map((icon) => icon.src)
    // 192 / 512 PNG 是安装提示的硬性要求
    for (const src of ['/icon-192.png', '/icon-512.png']) {
      expect(icons).toContain(src)
      expect(existsSync(join(publicDir, src.slice(1)))).toBe(true)
    }
    // manifest 引用的每个图标文件都必须存在
    for (const src of icons) {
      expect(existsSync(join(publicDir, src.slice(1)))).toBe(true)
    }
  })

  it('service worker 只拦截导航请求，不碰 /api 与 WebSocket', () => {
    const sw = readFileSync(join(publicDir, 'sw.js'), 'utf8')

    expect(sw).toContain("addEventListener('fetch'")
    expect(sw).toContain("request.mode !== 'navigate'")
    expect(sw).toContain("request.method !== 'GET'")
  })
})
