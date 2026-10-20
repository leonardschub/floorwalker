import { i18n } from '@floorwalker/i18n'

import type { WorldClient } from './world-client'

/* The world's fixed text (management-rooms spec, "The console"): every string the 3D
   world draws is a `world.text.*` key in packages/i18n. The page resolves them in its
   language and sends the whole table through `world_strings` before it first starts a
   world and whenever the language changes; the world holds no strings of its own. */

const PREFIX = 'world.text.'

/** Every `world.text.*` key with its text in the current language. A phrase the
    world fills in (a model and an agent, hours and minutes) keeps its `{{name}}`
    placeholders, in the order the language puts them, for the world to fill. */
export function worldStrings(): Record<string, string> {
 const bundle = i18n.getResourceBundle('en', 'translation') as Record<
  string,
  string
 >
 const keys = Object.keys(bundle).filter((key) => key.startsWith(PREFIX))
 return Object.fromEntries(
  keys.map((key) => [key, i18n.t(key, placeholders(bundle[key]))]),
 )
}

/* Each `{{name}}` in a phrase mapped to itself, so i18next leaves it in place. */
function placeholders(text: string): Record<string, string> {
 const names = [...text.matchAll(/\{\{(\w+)\}\}/g)].map((match) => match[1])
 return Object.fromEntries(names.map((name) => [name, `{{${name}}}`]))
}

/** Sends the table now and again on every language change, until the returned
    function is called. */
export function watchWorldStrings(client: WorldClient): () => void {
 const send = () => void client.reportWorldStrings(worldStrings())
 send()
 i18n.on('languageChanged', send)
 return () => i18n.off('languageChanged', send)
}
