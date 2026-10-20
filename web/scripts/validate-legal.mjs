/*
 * Check the legal documents before vite runs.
 *
 * The documents under web/src/content/legal/ are rendered by the browser, so
 * nothing in a normal build would notice a missing file, a broken front matter
 * block, an empty body or a heading that skips a level: the page would simply
 * render less than it should. This script parses every document the same way
 * the page does — the shared front matter module, then remark-parse with
 * remark-gfm, the parser react-markdown renders with — and fails, naming the
 * file and the problem, when the directory does not match the contract.
 *
 * It is invoked from web's `dev` and `build` scripts and can be imported from
 * a test: validateLegalDocuments() returns the list of problems instead of
 * exiting, and running the file directly prints them and exits non-zero.
 */

import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

import remarkGfm from 'remark-gfm'
import remarkParse from 'remark-parse'
import { unified } from 'unified'

import {
 LEGAL_DOCUMENTS,
 parseLegalDocument,
} from '../src/content/legal-front-matter.mjs'
import { legalLinkProtocol } from '../src/content/legal-protocols.mjs'

const here = dirname(fileURLToPath(import.meta.url))
const defaultDirectory = join(here, '..', 'src', 'content', 'legal')

const parser = unified().use(remarkParse).use(remarkGfm)

export function validateLegalDocuments(directory = defaultDirectory) {
 const problems = []
 const entries = readdirSync(directory, { withFileTypes: true })
 const files = new Set(
  entries.filter((entry) => entry.isFile()).map((entry) => entry.name),
 )
 const expected = new Set(LEGAL_DOCUMENTS.map((slug) => slug + '.md'))

 for (const entry of entries) {
  if (!expected.has(entry.name)) {
   problems.push(
    entry.name +
     ': not one of the legal documents (expected ' +
     [...expected].join(', ') +
     ')',
   )
  }
 }

 for (const slug of LEGAL_DOCUMENTS) {
  const name = slug + '.md'
  if (!files.has(name)) {
   problems.push(name + ': the document is missing')
   continue
  }
  problems.push(
   ...checkDocument(name, readFileSync(join(directory, name), 'utf8')),
  )
 }

 return problems
}

function checkDocument(name, raw) {
 const problems = []
 let document

 try {
  document = parseLegalDocument(raw)
 } catch (error) {
  return [name + ': ' + error.message]
 }

 const found = collect(parser.parse(document.body))

 for (const heading of found.headings) {
  if (heading.depth === 1) {
   problems.push(
    name +
     ': the body carries an H1; the document title is the page’s only H1, so the body starts at H2',
   )
  }
 }

 let previous = 1
 for (const heading of found.headings) {
  if (heading.depth > previous + 1) {
   problems.push(
    name +
     ': the heading "' +
     heading.text +
     '" is H' +
     heading.depth +
     ' after H' +
     previous +
     ', which skips a level',
   )
  }
  previous = heading.depth
 }

 if (found.text.join(' ').trim() === '') {
  problems.push(
   name +
    ': the body holds nothing once the front matter and headings are stripped',
  )
 }

 for (const url of found.urls) {
  const problem = checkUrl(url)
  if (problem) problems.push(name + ': ' + problem)
 }

 return problems
}

function collect(root) {
 const state = { headings: [], text: [], urls: [] }
 walk(root, false, state)
 return state
}

function walk(node, insideHeading, state) {
 if (node.type === 'heading') {
  state.headings.push({ depth: node.depth, text: textOf(node) })
  for (const child of node.children) walk(child, true, state)
  return
 }

 if (node.type === 'text' && !insideHeading) {
  state.text.push(node.value)
 }

 if (
  node.type === 'link' ||
  node.type === 'image' ||
  node.type === 'definition'
 ) {
  state.urls.push(node.url)
 }

 for (const child of node.children || []) walk(child, insideHeading, state)
}

function textOf(node) {
 if (node.value) return node.value
 return (node.children || []).map(textOf).join('')
}

function checkUrl(url) {
 const prefix = legalLinkProtocol(url)
 if (prefix === null) return null
 return 'the link protocol "' + prefix + '" is not allowed, in: ' + url
}

if (
 process.argv[1] &&
 import.meta.url === pathToFileURL(process.argv[1]).href
) {
 const problems = validateLegalDocuments()

 if (problems.length > 0) {
  for (const problem of problems) console.error('legal: ' + problem)
  process.exit(1)
 }

 console.log(
  'legal: ' +
   LEGAL_DOCUMENTS.length +
   (LEGAL_DOCUMENTS.length === 1 ? ' document' : ' documents') +
   ' validated',
 )
}
