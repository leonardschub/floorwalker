/*
 * Generate web/src/content/legal/oss-licenses.md from the installed workspace.
 *
 * The inventory comes from `pnpm licenses list --json --recursive --prod`, which
 * reports the resolved dependency graph — transitive packages included — but
 * carries no license text at all. The text is resolved from each package's own
 * installed directory, which is why the file ships full texts rather than a
 * page of SPDX identifiers: MIT, ISC, BSD-3-Clause and Apache-2.0 section 4(a)
 * all expect the notice or the license copy to reach a recipient.
 *
 * The desktop app also ships components that are not JavaScript packages: Rust
 * crates and fonts whose licenses ask for their text to travel with them. They are
 * listed in DESKTOP_COMPONENTS below, each with the copy of its license committed
 * beside it, and follow the JavaScript packages in their own section.
 *
 * The file is committed, so it can be read during development, and
 * `--check` regenerates it in memory and fails when the committed file no
 * longer matches the installed graph. Run the generator with:
 *
 *   corepack pnpm --filter web legal:licenses
 */

import { execFileSync } from 'node:child_process'
import {
 existsSync,
 readdirSync,
 readFileSync,
 statSync,
 writeFileSync,
} from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
 formatLegalDocument,
 parseLegalDocument,
} from '../src/content/legal-front-matter.mjs'

const here = dirname(fileURLToPath(import.meta.url))
const web = resolve(here, '..')
const root = resolve(web, '..')
const store = join(root, 'node_modules', '.pnpm')
const target = join(web, 'src', 'content', 'legal', 'oss-licenses.md')

const LICENSE_FILE = /^(licen[cs]e|copying|unlicense)/i

/*
 * Components of the desktop app outside the JavaScript workspace whose licenses ask
 * for their text (and any NOTICE) to be shipped. Each `files` path is relative to the
 * repository root; the NOTICE.md beside each says where the copy came from.
 */
const DESKTOP_COMPONENTS = [
 {
  name: 'alacritty_terminal',
  version: '0.26.0',
  license: 'Apache-2.0',
  what:
   "the session terminals' terminal emulator (Rust crate); it publishes no NOTICE file",
  files: [
   'desktop/src-tauri/terminals/third-party/alacritty_terminal/LICENSE-APACHE',
  ],
 },
 {
  name: 'tauri-plugin-opener',
  version: '2.7.0',
  license: 'MIT OR Apache-2.0',
  what: "the seated terminal's link opener (Rust crate); it publishes no NOTICE file",
  files: [
   'desktop/src-tauri/third-party/tauri-plugin-opener/LICENSE_MIT',
   'desktop/src-tauri/third-party/tauri-plugin-opener/LICENSE_APACHE-2.0',
  ],
 },
 {
  name: 'JetBrains Mono',
  version: '2.211',
  license: 'OFL-1.1',
  what: "the session terminals' font",
  files: ['desktop/world-assets/third-party/jetbrains-mono/OFL.txt'],
 },
 {
  name: 'Oswald',
  version: '4.103',
  license: 'OFL-1.1',
  what: "the world's room signs and robot names font",
  files: ['desktop/world-assets/third-party/oswald/OFL.txt'],
 },
]

const check = process.argv.includes('--check')

const storeDirectories = readdirSync(store)
const entries = collectEntries(readInventory())
const body = buildBody(entries) + buildDesktopSection(DESKTOP_COMPONENTS)
const existing = existsSync(target) ? readFileSync(target, 'utf8') : null
const fields = {
 title: 'Open-Source Licenses',
 lastUpdated: lastUpdatedOf(existing, body),
 language: 'en',
 generated: true,
}
const content = formatLegalDocument(fields, body)

if (check) {
 if (existing === null) {
  console.error(
   'legal: oss-licenses.md is missing; run: corepack pnpm --filter web legal:licenses',
  )
  process.exit(1)
 }

 if (existing !== content) {
  console.error(
   'legal: oss-licenses.md is stale; the installed dependency graph has moved on.',
  )
  console.error(
   'legal: regenerate it with: corepack pnpm --filter web legal:licenses',
  )
  process.exit(1)
 }

 console.log(
  'legal: oss-licenses.md is current, with ' + entries.length + ' packages',
 )
} else if (existing === content) {
 console.log('legal: oss-licenses.md is already up to date')
} else {
 writeFileSync(target, content)
 console.log(
  'legal: wrote oss-licenses.md with ' +
   entries.length +
   ' packages, ' +
   entries.filter((entry) => entry.text === null).length +
   ' of them without a license file',
 )
}

function readInventory() {
 const json = execFileSync(
  'corepack',
  ['pnpm', 'licenses', 'list', '--json', '--recursive', '--prod'],
  { cwd: root, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
 )
 return JSON.parse(json)
}

function collectEntries(inventory) {
 const collected = []

 for (const [license, group] of Object.entries(inventory)) {
  for (const entry of group) {
   for (const version of entry.versions) {
    collected.push({
     name: entry.name,
     version,
     license,
     text: readLicenseText(entry, version),
    })
   }
  }
 }

 return collected.sort(
  (a, b) => compare(a.name, b.name) || compare(a.version, b.version),
 )
}

function readLicenseText(entry, version) {
 const directory = findDirectory(entry, version)
 if (directory === null) return null

 const files = readdirSync(directory)
  .filter(
   (file) =>
    LICENSE_FILE.test(file) && statSync(join(directory, file)).isFile(),
  )
  .sort()

 if (files.length === 0) return null

 return files
  .map((file) => readFileSync(join(directory, file), 'utf8'))
  .map(normalizeText)
  .join('\n\n')
}

/*
 * Several published license files carry Windows line endings and trailing
 * spaces. Normalising them keeps the committed document on the line ending the
 * rest of the repository uses, and keeps it byte-identical to what Prettier
 * prints, which is what `--check` compares against.
 */
function normalizeText(text) {
 return text
  .replace(/\r\n?/g, '\n')
  .replace(/[ \t]+$/gm, '')
  .trim()
}

/*
 * The installed directory of one package-version pair. The store path is
 * derived first because pnpm's own `paths` field has moved between releases;
 * the recorded path and the peer-suffixed directory names are the fallbacks.
 */
function findDirectory(entry, version) {
 const base = entry.name.replace('/', '+') + '@' + version
 const candidates = [
  join(store, base, 'node_modules', entry.name),
  ...(entry.paths || []),
  ...storeDirectories
   .filter((directory) => directory.startsWith(base + '_'))
   .sort()
   .map((directory) => join(store, directory, 'node_modules', entry.name)),
 ]

 return candidates.find((candidate) => isDirectory(candidate)) || null
}

function buildBody(entries) {
 const counts = new Map()
 for (const entry of entries) {
  counts.set(entry.license, (counts.get(entry.license) || 0) + 1)
 }

 const withoutText = entries.filter((entry) => entry.text === null).length
 const lines = []

 lines.push('## Summary')
 lines.push('')
 lines.push(
  'Every JavaScript package installed in this workspace sits below, at the version the',
 )
 lines.push(
  'lockfile pins. The list is generated from the installed dependency graph rather than',
 )
 lines.push(
  'from the web bundle, so it is wider than the packages a browser downloads.',
 )
 if (withoutText > 0) {
  lines.push('')
  lines.push(
   withoutText +
    ' of the ' +
    entries.length +
    ' packages publish no license file of their own; those entries say so instead of',
  )
  lines.push('reproducing a text that does not exist.')
 }
 lines.push('')
 lines.push(
  ...formatTable(
   ['License', 'Packages'],
   [...counts]
    .sort((a, b) => compare(a[0], b[0]))
    .map(([license, count]) => [license, String(count)]),
  ),
 )
 lines.push('')
 lines.push('## License texts')
 lines.push('')
 lines.push(
  'The texts below are reproduced from each installed package, and each entry names the',
 )
 lines.push('license its package metadata declares.')

 for (const entry of entries) {
  lines.push('')
  lines.push('### ' + entry.name + ' ' + entry.version)
  lines.push('')
  lines.push('Declared license: ' + entry.license)
  lines.push('')
  if (entry.text === null) {
   lines.push(
    'The published package includes no license file, so there is no text to reproduce.',
   )
  } else {
   lines.push(fence(entry.text))
  }
 }

 return lines.join('\n') + '\n'
}

function buildDesktopSection(components) {
 const lines = []

 lines.push('')
 lines.push('## Desktop app components')
 lines.push('')
 lines.push(
  'The desktop app also ships these components, which are not JavaScript packages. Their',
 )
 lines.push(
  'texts are reproduced from the copies committed in this repository beside each one.',
 )

 for (const component of components) {
  lines.push('')
  lines.push('### ' + component.name + ' ' + component.version)
  lines.push('')
  lines.push(
   'Declared license: ' +
    component.license +
    '. Used as ' +
    component.what +
    '.',
  )
  for (const file of component.files) {
   lines.push('')
   lines.push(fence(normalizeText(readFileSync(join(root, file), 'utf8'))))
  }
 }

 return lines.join('\n') + '\n'
}

function fence(text) {
 const runs = text.match(/`+/g) || []
 const marker = '`'.repeat(Math.max(3, ...runs.map((run) => run.length + 1)))
 return marker + '\n' + text + '\n' + marker
}

// Prettier pads every cell of a table to its column width, so the generated
// table does the same and the committed file survives `prettier --check`.
function formatTable(header, rows) {
 const widths = header.map((cell, index) =>
  Math.max(cell.length, ...rows.map((row) => row[index].length)),
 )
 const line = (cells) =>
  '| ' +
  cells.map((cell, index) => cell.padEnd(widths[index])).join(' | ') +
  ' |'
 return [
  line(header),
  '| ' + widths.map((width) => '-'.repeat(width)).join(' | ') + ' |',
  ...rows.map(line),
 ]
}

function lastUpdatedOf(existing, body) {
 if (existing !== null) {
  try {
   const previous = parseLegalDocument(existing)
   if (previous.body === body) return previous.data.lastUpdated
  } catch {
   // A committed file that does not parse is replaced outright.
  }
 }

 return new Date().toISOString().slice(0, 10)
}

function compare(a, b) {
 if (a === b) return 0
 return a < b ? -1 : 1
}

function isDirectory(path) {
 try {
  return statSync(path).isDirectory()
 } catch {
  return false
 }
}
