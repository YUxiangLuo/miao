import { Errored } from 'solid-js'
import { render } from '@solidjs/web'
import App from './App'
import { Button, LogoIcon } from './components/ui'
import { initMotionPolicy } from './motion'
import './styles.css'

initMotionPolicy()

const rootElement = document.getElementById('root')
if (!rootElement) throw new Error('找不到 #root 挂载点')

render(
  () => (
    <Errored
      fallback={(error, reset) => (
        <div class="shell-state-screen">
          <div class="shell-state-card" role="alert">
            <LogoIcon size={40} />
            <strong>控制台界面出现异常</strong>
            <span class="shell-state-sub">
              {error() instanceof Error ? (error() as Error).message : String(error())}
            </span>
            <div style={{ display: 'flex', gap: '8px', 'margin-top': '8px' }}>
              <Button tone="primary" size="sm" onClick={reset}>重试渲染</Button>
              <Button tone="secondary" size="sm" onClick={() => window.location.reload()}>刷新页面</Button>
            </div>
          </div>
        </div>
      )}
    >
      <App />
    </Errored>
  ),
  rootElement,
)
