import { cleanup, fireEvent, screen, within } from '@testing-library/react'
import { afterEach, expect, test } from 'vitest'

import { clearMocks, fakeShell } from './test/fake-shell'
import { loaded, openAt } from './test/open-app'

afterEach(() => {
 cleanup()
 clearMocks()
})

function section(name: string) {
 return screen.getByRole('heading', { level: 2, name }).closest('section')!
}

test("the world's Settings button leads to Appearance, World and About", async () => {
 fakeShell()
 openAt('/')

 fireEvent.click(
  await screen.findByRole('button', { name: 'Settings' }, loaded),
 )

 await screen.findByRole('heading', { level: 1, name: 'Settings' }, loaded)
 const headings = screen
  .getAllByRole('heading', { level: 2 })
  .map((heading) => heading.textContent)
 expect(headings).toEqual(['Appearance', 'World', 'About'])
})

test("Settings' way back returns to the world", async () => {
 fakeShell()
 openAt('/settings')

 fireEvent.click(
  await screen.findByRole('link', { name: 'Back to the world' }, loaded),
 )

 expect(
  await screen.findByRole('heading', { level: 1, name: 'World' }, loaded),
 ).toBeTruthy()
})

/* The product's name, the credit, and the appropriate legal notices of AGPL-3.0
   section 0. */
test('About names the app, credits the maker and carries the licence notices', async () => {
 fakeShell()
 openAt('/settings')
 await screen.findByRole('heading', { level: 2, name: 'About' }, loaded)
 const about = within(section('About'))

 expect(about.getByText('Floorwalker')).toBeTruthy()
 expect(about.getByText('Made by leonardschub (GitHub username)')).toBeTruthy()
 expect(about.getByText('Copyright (C) 2026 leonardschub')).toBeTruthy()
 expect(about.getByText(/without any warranty/i)).toBeTruthy()
 expect(
  about.getByText(
   /redistribute it .* under the terms of the GNU Affero General Public License/,
  ),
 ).toBeTruthy()
 expect(about.getByText(/AGPL-3\.0-or-later/)).toBeTruthy()
 expect(
  about.getByRole('link', { name: /licence text/i }).getAttribute('href'),
 ).toBe('/license')
 expect(
  about
   .getByRole('link', { name: /open-source licences/i })
   .getAttribute('href'),
 ).toBe('/oss-licenses')
})

test('the licence text reads inside the app', async () => {
 fakeShell()
 openAt('/license')

 expect(
  await screen.findByText(/GNU AFFERO GENERAL PUBLIC LICENSE/, {}, loaded),
 ).toBeTruthy()
 expect(screen.getByText(/Version 3, 19 November 2007/)).toBeTruthy()
})

test('the open-source licences page reads inside the app', async () => {
 fakeShell()
 openAt('/oss-licenses')

 expect(
  await screen.findByRole(
   'heading',
   { level: 1, name: 'Open-Source Licenses' },
   loaded,
  ),
 ).toBeTruthy()
})
