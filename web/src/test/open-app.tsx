import { render } from '@testing-library/react'
import { MemoryRouter } from 'react-router'

import App from '../app'

/** Opens the app at `path`, as the desktop shell's webview would. */
export function openAt(path: string) {
 return render(
  <MemoryRouter initialEntries={[path]}>
   <App />
  </MemoryRouter>,
 )
}

/* The world client, the agents client and every page are loaded on first use, as in
   the app, and the first test to need one pays for loading it. */
export const loaded = { timeout: 5000 }
