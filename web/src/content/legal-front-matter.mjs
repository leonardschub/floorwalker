/*
 * The front matter contract for the legal documents.
 *
 * The files under web/src/content/legal/ are the complete documents the app
 * publishes: today the open-source licences page alone, which bundled
 * third-party code requires whenever the binary is shipped. Each one opens with a small YAML block carrying its title, its
 * last-updated date and the language it is written in, and the body below the
 * block starts at H2 because the title is the page's only H1.
 *
 * This module is the one place that shape is read and written. The legal pages
 * call parseLegalDocument at render time, and the node scripts under
 * web/scripts/ call it before vite runs, so the page and the validator cannot
 * disagree about what a document says.
 */

export const LEGAL_DOCUMENTS = ['oss-licenses']

const FIELD = /^([A-Za-z][A-Za-z0-9-]*):(.*)$/
const LAST_UPDATED = /^\d{4}-\d{2}-\d{2}$/
const BARE = /^[A-Za-z0-9][A-Za-z0-9 ._()-]*$/

export function parseLegalDocument(raw) {
 const text = raw.replace(/^\ufeff/, '').replace(/\r\n?/g, '\n')
 const lines = text.split('\n')

 if (lines[0] !== '---') {
  throw new Error('front matter must open with a --- line')
 }

 const close = lines.indexOf('---', 1)
 if (close === -1) {
  throw new Error('front matter must close with a --- line')
 }

 const fields = new Map()
 for (let index = 1; index < close; index += 1) {
  const line = lines[index]
  if (line.trim() === '' || line.startsWith('#')) continue

  const match = FIELD.exec(line)
  if (!match) {
   throw new Error(
    'front matter line ' + (index + 1) + ' is not a key: value pair',
   )
  }

  const key = match[1]
  if (fields.has(key)) {
   throw new Error('front matter repeats the ' + key + ' field')
  }
  fields.set(key, parseValue(match[2].trim(), key))
 }

 const data = {
  title: readText(fields, 'title'),
  lastUpdated: readDate(fields),
  language: readText(fields, 'language'),
  generated: readFlag(fields, 'generated'),
 }

 for (const key of fields.keys()) {
  if (!(key in data)) {
   throw new Error('front matter carries an unknown field: ' + key)
  }
 }

 const body = lines
  .slice(close + 1)
  .join('\n')
  .replace(/^\n+/, '')
 return { data, body }
}

export function formatLegalDocument(fields, body) {
 const lines = ['---']
 for (const [key, value] of Object.entries(fields)) {
  lines.push(key + ': ' + formatValue(value))
 }
 lines.push('---', '', body.trim(), '')
 return lines.join('\n')
}

function parseValue(value, key) {
 if (value === '') return null
 if (value === 'true') return true
 if (value === 'false') return false

 if (value.startsWith('"')) {
  if (value.length < 2 || !value.endsWith('"')) {
   throw new Error(
    'front matter field ' + key + ' has an unclosed double quote',
   )
  }
  return value.slice(1, -1).replace(/\\(["\\])/g, '$1')
 }

 if (value.startsWith("'")) {
  if (value.length < 2 || !value.endsWith("'")) {
   throw new Error(
    'front matter field ' + key + ' has an unclosed single quote',
   )
  }
  return value.slice(1, -1).replace(/''/g, "'")
 }

 return value
}

function readText(fields, key) {
 const value = fields.get(key)
 if (value === undefined || value === null || value === '') {
  throw new Error('front matter is missing ' + key)
 }
 if (typeof value !== 'string') {
  throw new Error('front matter field ' + key + ' must be text')
 }
 return value
}

function readDate(fields) {
 const value = readText(fields, 'lastUpdated')
 if (!LAST_UPDATED.test(value)) {
  throw new Error('front matter field lastUpdated must be a YYYY-MM-DD date')
 }
 if (!isRealDate(value)) {
  throw new Error(
   'front matter field lastUpdated is not a date that exists: ' + value,
  )
 }
 return value
}

/*
 * The shape above still accepts a month 13 or a day 32. A round trip through a
 * UTC date rejects the values a calendar does not have.
 */
function isRealDate(value) {
 const [year, month, day] = value.split('-').map(Number)
 const date = new Date(Date.UTC(year, month - 1, day))
 return (
  date.getUTCFullYear() === year &&
  date.getUTCMonth() === month - 1 &&
  date.getUTCDate() === day
 )
}

function readFlag(fields, key) {
 const value = fields.get(key)
 if (value === undefined || value === null) return false
 if (typeof value !== 'boolean') {
  throw new Error('front matter field ' + key + ' must be true or false')
 }
 return value
}

function formatValue(value) {
 if (typeof value === 'boolean') return String(value)
 if (BARE.test(value)) return value
 return '"' + value.replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"'
}
