import React from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import './styles.css'

const rootElement = document.getElementById('root')
if (!rootElement) throw new Error('找不到 #root 挂载点')

createRoot(rootElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
