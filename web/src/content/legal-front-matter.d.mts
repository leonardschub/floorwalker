export type LegalFrontMatter = {
 title: string
 lastUpdated: string
 language: string
 generated: boolean
}

export declare const LEGAL_DOCUMENTS: readonly string[]

export declare function parseLegalDocument(raw: string): {
 data: LegalFrontMatter
 body: string
}

export declare function formatLegalDocument(
 fields: Record<string, string | boolean>,
 body: string,
): string
