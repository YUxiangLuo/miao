import { classNames } from '../utils'
import { iconForDomain } from './siteIcons'

// 轮询刷新时数值变化会触发 key 变化重新挂载,配合 CSS 淡入提示数据已更新
export function AnimatedValue({ value, className }: { value: string | number; className?: string }) {
  return <span key={String(value)} className={classNames('value-anim', 'num', className)}>{value}</span>
}

/** 站点/连接的品牌图标：命中品牌库取官方几何，否则首字母兜底；small 用于统计卡 favicon 条 */
export function SiteMark({ domain, small }: { domain: string; small?: boolean }) {
  const icon = iconForDomain(domain)
  const isLetter = 'letter' in icon
  return (
    <div
      className={classNames('site-icon', isLetter && 'letter', small && 'sm')}
      style={!isLetter ? { background: icon.background, color: icon.color } : undefined}
      data-site={icon.id}
      title={icon.label}
      aria-hidden="true"
    >
      {!isLetter ? (
        <svg viewBox={icon.viewBox || '0 0 24 24'} width={small ? 12 : 18} height={small ? 12 : 18}>
          <path fill="currentColor" d={icon.path} />
        </svg>
      ) : (
        <span>{icon.letter}</span>
      )}
    </div>
  )
}
