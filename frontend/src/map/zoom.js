// 地图缩放/平移的 viewBox 数学，纯函数方便单测。
// 宽高比锁定为基准窗口的比例，缩放只改宽度，高度随之推导。

import { MAP_WIDTH } from './projection.js'

export const MIN_SCALE = 1
export const MAX_SCALE = 8

// 基准窗口：裁掉底部无数据的南冰洋
export const BASE_VIEW_BOX = { x: 0, y: 30, w: MAP_WIDTH, h: 440 }

const ASPECT = BASE_VIEW_BOX.h / BASE_VIEW_BOX.w

export function initialViewBox() {
  return { ...BASE_VIEW_BOX }
}

export function scaleOf(viewBox) {
  return BASE_VIEW_BOX.w / viewBox.w
}

export function clampViewBox(viewBox) {
  const w = Math.min(BASE_VIEW_BOX.w, Math.max(BASE_VIEW_BOX.w / MAX_SCALE, viewBox.w))
  const h = w * ASPECT
  const x = Math.min(BASE_VIEW_BOX.x + BASE_VIEW_BOX.w - w, Math.max(BASE_VIEW_BOX.x, viewBox.x))
  const y = Math.min(BASE_VIEW_BOX.y + BASE_VIEW_BOX.h - h, Math.max(BASE_VIEW_BOX.y, viewBox.y))
  return { x, y, w, h }
}

// 以视图坐标 (cx, cy) 为锚点缩放：锚点在屏幕上保持不动
export function zoomAtPoint(viewBox, cx, cy, factor) {
  const targetW = viewBox.w / factor
  const clampedW = Math.min(BASE_VIEW_BOX.w, Math.max(BASE_VIEW_BOX.w / MAX_SCALE, targetW))
  const actualFactor = viewBox.w / clampedW
  return clampViewBox({
    ...viewBox,
    w: clampedW,
    x: cx - (cx - viewBox.x) / actualFactor,
    y: cy - (cy - viewBox.y) / actualFactor,
  })
}

export function panBy(viewBox, dx, dy) {
  return clampViewBox({ ...viewBox, x: viewBox.x + dx, y: viewBox.y + dy })
}
