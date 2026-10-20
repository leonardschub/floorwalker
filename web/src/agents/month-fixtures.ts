import type { OfTheMonth } from './client'

/* The Of the month view's figures for browser development: invented totals in the
   shape the desktop's registry answers. */

export const fixtureMonth: OfTheMonth = {
 model: {
  leaders: [
   {
    id: 'deepseek-flash',
    name: 'deepseek-flash',
    total: 182_400_000,
    share: 0.41,
   },
   { id: 'claude-opus-5-5', name: 'Opus 5.5', total: 129_000_000, share: 0.29 },
   { id: 'gpt-6.1-sol', name: 'GPT-6.1 Sol', total: 75_600_000, share: 0.17 },
  ],
  lastMonth: 'Opus 5.5',
 },
 harness: {
  leaders: [
   { id: 'claude-code', name: 'Claude Code', total: 301_900_000, share: 0.68 },
   { id: 'codex', name: 'Codex', total: 119_900_000, share: 0.27 },
   { id: 'grok-build', name: 'Grok Build', total: 22_200_000, share: 0.05 },
  ],
  lastMonth: null,
 },
}
