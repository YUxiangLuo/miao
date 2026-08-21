/** favicon 栅格单行最多展示的图标数，超出折叠为 +N */
export const MAX_ICONS = 18

export interface IconFoldPlan {
  shown: number
  more: number
}

/** favicon 单行折叠方案：fit 格放得下就全显；放不下时末格让给 +N（more 含超 MAX 的部分） */
export function foldIcons(total: number, fit: number, max: number = MAX_ICONS): IconFoldPlan {
  const capped = Math.min(total, max)
  if (capped <= fit) return { shown: capped, more: total - capped }
  const shown = Math.max(1, fit - 1)
  return { shown, more: total - shown }
}
