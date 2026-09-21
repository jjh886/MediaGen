import { textDocs } from './textDocs'
import { imageDocs } from './imageDocs'
import { audioDocs } from './audioDocs'
import { fileDocs } from './fileDocs'
export type { ApiDocument } from './samples'
export const documents = [...textDocs, ...imageDocs, ...audioDocs, ...fileDocs]
