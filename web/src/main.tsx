import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router'

import App from './app'
import { startTheme } from './theme/theme'
import '@floorwalker/i18n'
import './index.css'

startTheme()

const app = createRoot(document.getElementById('root')!)

app.render(
 <StrictMode>
  <BrowserRouter>
   <App />
  </BrowserRouter>
 </StrictMode>,
)
