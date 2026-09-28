import { textDocs } from './textDocs'
import { imageDocs } from './imageDocs'
import { audioDocs } from './audioDocs'
import { fileDocs } from './fileDocs'
import { describeContract } from './contracts'
import type { ApiDocument } from './schema'
export type { ApiDocument } from './schema'
export const documentGroups = ['模型', '文本', '图片', '视频', '音频', '实时语音', '文件', '排队']
export const documents: ApiDocument[] = [...textDocs, ...imageDocs, ...audioDocs, ...fileDocs]
  .sort((left, right) => documentGroups.indexOf(left.group) - documentGroups.indexOf(right.group))
  .map((doc) => ({ ...doc, ...describeContract(doc) }))
