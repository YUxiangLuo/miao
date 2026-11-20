import { render } from '@solidjs/web'
import App from './App'
import './styles.css'

const rootElement = document.getElementById('root')
if (!rootElement) throw new Error('找不到 #root 挂载点')

render(() => <App />, rootElement)
