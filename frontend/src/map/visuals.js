// 地图视觉编码的纯函数：速度 → 线宽/粒子，以及确定性星空。
// 与渲染解耦，方便单测。

export const MIN_FLOW_WIDTH = 1.2
export const MAX_FLOW_WIDTH = 4.0
export const MAX_PARTICLES_TOTAL = 50
export const MAX_PARTICLES_PER_PATH = 3

// speed: 字节/秒。1.2px（静止）→ 4.0px（≥1MB/s），按 log10 映射。
export function flowWidth(speed) {
  const value = Math.max(0, Number(speed) || 0)
  const t = Math.min(1, Math.max(0, (Math.log10(value + 1) - 2) / 4))
  return MIN_FLOW_WIDTH + t * (MAX_FLOW_WIDTH - MIN_FLOW_WIDTH)
}

// 活跃连接（>=1KB/s）才有粒子，速度越快粒子越多，最多 3 个。
export function particleCount(speed) {
  const value = Math.max(0, Number(speed) || 0)
  if (value < 1024) return 0
  if (value < 10 * 1024) return 1
  if (value < 100 * 1024) return 2
  return MAX_PARTICLES_PER_PATH
}

// 1KB/s → 4s 一圈，≥1MB/s → 1.2s。
export function particleDuration(speed) {
  const value = Math.max(1024, Number(speed) || 0)
  const t = Math.min(1, Math.log10(value / 1024) / 3)
  return 4 - t * 2.8
}

function fnv1a(text) {
  let hash = 2166136261
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index)
    hash = Math.imul(hash, 16777619)
  }
  return hash >>> 0
}

// 确定性星空：同一份数据每次渲染一致，避免闪烁。
export function buildStarfield(count = 140, width = 1000, height = 500) {
  const stars = []
  for (let index = 0; index < count; index += 1) {
    const hash = fnv1a(`star-${index}`)
    stars.push({
      id: index,
      x: ((hash % 1000) / 1000) * width,
      y: (((hash >>> 10) % 1000) / 1000) * height,
      r: 0.4 + (((hash >>> 20) % 100) / 100) * 0.8,
      opacity: 0.15 + (((hash >>> 5) % 100) / 100) * 0.35,
    })
  }
  return stars
}

export const STARFIELD = buildStarfield()

// 全体路径的粒子配额：按速度排序，总粒子数封顶，避免滤镜 + 动画卡顿。
export function allocateParticles(pathsWithSpeed) {
  const sorted = [...(pathsWithSpeed || [])].sort((a, b) => b.speed - a.speed)
  const allocation = new Map()
  let budget = MAX_PARTICLES_TOTAL
  for (const item of sorted) {
    if (budget <= 0) break
    const count = Math.min(particleCount(item.speed), budget)
    if (count > 0) {
      allocation.set(item.id, count)
      budget -= count
    }
  }
  return allocation
}
