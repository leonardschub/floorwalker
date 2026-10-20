import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { afterEach, expect, test } from 'vitest'

import { validateLegalDocuments } from './validate-legal.mjs'

/* The app publishes one legal document, the open-source licences page (local-only
   spec, "Legal"): there is no service to set terms or a privacy policy for, AGPL
   replaces an end-user licence, and an imprint belongs on a website. */

const licences = `---
title: Open-Source Licenses
lastUpdated: 2026-10-20
language: en
generated: true
---

## Summary

Every package the app is built from, with its licence.
`

const terms = `---
title: Terms of Service
lastUpdated: 2026-10-15
language: en
---

## This document is a scaffold

It is a placeholder.
`

let directory

afterEach(() => {
 if (directory) rmSync(directory, { recursive: true, force: true })
 directory = undefined
})

function legalFolder(files) {
 directory = mkdtempSync(join(tmpdir(), 'legal-'))
 for (const [name, text] of Object.entries(files)) {
  writeFileSync(join(directory, name), text)
 }
 return directory
}

test('the licences page on its own is the complete set', () => {
 expect(
  validateLegalDocuments(legalFolder({ 'oss-licenses.md': licences })),
 ).toEqual([])
})

test('a terms of service document is refused', () => {
 const problems = validateLegalDocuments(
  legalFolder({ 'oss-licenses.md': licences, 'tos.md': terms }),
 )

 expect(problems).toHaveLength(1)
 expect(problems[0]).toMatch(/^tos\.md: /)
})

test('the licences page is required', () => {
 expect(validateLegalDocuments(legalFolder({}))).toEqual([
  'oss-licenses.md: the document is missing',
 ])
})

test('the documents the app ships validate', () => {
 expect(validateLegalDocuments()).toEqual([])
})
