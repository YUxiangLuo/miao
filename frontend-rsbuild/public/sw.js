// Miao PWA Service Worker。
// 存在的唯一理由是满足 Chrome 的可安装门槛（install prompt 要求带 fetch
// handler 的 SW）。面板离开后端就是死页面，所以不做激进的离线缓存：
// 仅对导航请求 network-first，成功时刷新缓存、断网时回退缓存；
// /api 与 WebSocket 一律直连网络（WS 升级请求本就不经过 fetch 事件）。
const CACHE = 'miao-shell-v1'

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.add('/'))
      .then(() => self.skipWaiting())
  )
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key)))
      )
      .then(() => self.clients.claim())
  )
})

self.addEventListener('fetch', (event) => {
  const { request } = event
  if (request.method !== 'GET' || request.mode !== 'navigate') return
  event.respondWith(
    fetch(request)
      .then((response) => {
        if (response.ok) {
          const copy = response.clone()
          caches.open(CACHE).then((cache) => cache.put(request, copy))
        }
        return response
      })
      // 断网且缓存缺失时退化为浏览器默认错误页；只缓存 '/' 一条，直接匹配
      .catch(() => caches.match('/'))
  )
})
