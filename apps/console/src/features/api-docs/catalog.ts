import { textDocs } from './textDocs'
import { imageDocs } from './imageDocs'
import { audioDocs } from './audioDocs'
import { fileDocs } from './fileDocs'
import { describeContract } from './contracts'
import type { ApiDocument } from './schema'
export type { ApiDocument } from './schema'
export const documents: ApiDocument[] = [...textDocs, ...imageDocs, ...audioDocs, ...fileDocs].map(
  (doc) => ({ ...doc, ...describeContract(doc) })
)
