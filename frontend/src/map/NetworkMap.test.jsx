import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { NetworkMap } from './NetworkMap.jsx'

const snapshot = {
  client: {
    type: 'client',
    name: 'This Device',
    geo: { country: 'China', country_code: 'CN', city: 'Shanghai', latitude: 31.2, longitude: 121.5 },
  },
  proxies: [
    {
      type: 'proxy',
      name: 'Tokyo 01',
      server: 'tokyo.example.com',
      geo: { country: 'Japan', country_code: 'JP', city: 'Tokyo', latitude: 35.6, longitude: 139.7 },
    },
    {
      type: 'proxy',
      name: 'Tokyo 02',
      server: 'tokyo2.example.com',
      geo: { country: 'Japan', country_code: 'JP', city: 'Tokyo', latitude: 35.6, longitude: 139.7 },
    },
  ],
  flows: [
    {
      id: 'yt',
      network: 'tcp',
      upload_speed: 20,
      download_speed: 4800,
      upload_total: 2000,
      download_total: 80000,
      rule: 'final',
      port: 443,
      destination: {
        type: 'destination',
        domain: 'googlevideo.com',
        ip: '142.250.1.1',
        geo: { country: 'Germany', country_code: 'DE', city: 'Frankfurt', latitude: 50.1, longitude: 8.6 },
      },
      proxy: {
        type: 'proxy',
        name: 'Tokyo 01',
        server: 'tokyo.example.com',
        geo: { country: 'Japan', country_code: 'JP', city: 'Tokyo', latitude: 35.6, longitude: 139.7 },
      },
    },
    {
      id: 'gh',
      network: 'tcp',
      upload_speed: 1,
      download_speed: 2,
      upload_total: 10,
      download_total: 20,
      rule: 'geosite',
      port: 443,
      destination: {
        type: 'destination',
        domain: 'github.com',
        ip: '20.1.1.1',
        geo: { country: 'United States', country_code: 'US', city: 'Seattle', latitude: 47.6, longitude: -122.3 },
      },
    },
  ],
}

describe('NetworkMap', () => {
  it('filters the visible destination set', async () => {
    const user = userEvent.setup()
    render(<NetworkMap snapshot={snapshot} />)

    expect(screen.getByLabelText('世界网络地图')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'YouTube · Frankfurt' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'GitHub · Seattle' })).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: /直连/ }))
    expect(screen.queryByRole('button', { name: 'YouTube · Frankfurt' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'GitHub · Seattle' })).toBeInTheDocument()
  })

  it('opens destination details from a marker', async () => {
    const user = userEvent.setup()
    render(<NetworkMap snapshot={snapshot} />)

    await user.click(screen.getByRole('button', { name: 'YouTube · Frankfurt' }))
    const panel = screen.getByRole('complementary', { name: '连接详情' })
    expect(within(panel).getByText('googlevideo.com')).toBeInTheDocument()
    expect(within(panel).getByText('142.250.1.1')).toBeInTheDocument()
    expect(within(panel).getByText('Tokyo 01')).toBeInTheDocument()
  })

  it('lets the user switch a node from a city group', async () => {
    const user = userEvent.setup()
    const onSwitchProxy = vi.fn()
    render(
      <NetworkMap
        snapshot={snapshot}
        primaryGroupName="proxy"
        currentNodeName="Tokyo 01"
        delays={{ 'Tokyo 01': 42, 'Tokyo 02': 38 }}
        onSwitchProxy={onSwitchProxy}
      />,
    )

    await user.click(screen.getByRole('button', { name: 'Tokyo × 2' }))
    expect(screen.getByRole('complementary', { name: '代理城市' })).toBeInTheDocument()
    expect(screen.getByText(/当前 Tokyo 01/)).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '切换' }))
    expect(onSwitchProxy).toHaveBeenCalledWith('proxy', 'Tokyo 02')
  })
})
