import { cleanup, screen, waitFor } from '@testing-library/react'
import { afterEach, expect, test } from 'vitest'

import { clearMocks, fakeShell } from './test/fake-shell'
import { loaded, openAt } from './test/open-app'

afterEach(() => {
 cleanup()
 clearMocks()
})

test('the app opens straight into the world and starts it, with no sign-in', async () => {
 const shell = fakeShell()

 openAt('/')

 await waitFor(
  () =>
   expect(
    document
     .querySelector('[data-world-region="view"]')
     ?.getAttribute('data-world-state'),
   ).toBe('running'),
  loaded,
 )
 expect(shell.asked).toContain('world_start')
})

test('the world shows the side panel with the live sessions, with no account', async () => {
 fakeShell()

 openAt('/')

 expect(
  await screen.findByText('Make the paddles faster', {}, loaded),
 ).toBeTruthy()
})
