import { type ApiExample, image, imageNotes } from './samples'
export const imageDocs: ApiExample[] = [
  {
    group: '图片',
    method: 'POST',
    path: '/v1/images/generations',
    title: '图片生成 / 图片排队',
    description:
      '默认同步返回完整 Base64。使用 async=true 或 sync=false 可立即返回 taskId，再轮询排队接口。',
    notes: imageNotes,
    body: {
      model: 'gpt-image-2',
      prompt: '一只窗边的猫，水彩画。',
      n: 1,
      size: '1024x1024',
      response_format: 'b64_json',
      sync: true,
    },
    response: image,
  },
  {
    group: '图片',
    method: 'POST',
    path: '/v1/images/edits',
    title: '图片编辑',
    description: '支持 multipart 上传，或通过 JSON images 输入图片。当前排队仅开放图片生成。',
    notes: imageNotes,
    form: ['model=gpt-image-2', 'prompt=把背景改为蓝色', 'image=@reference.png'],
    response: image,
  },
  {
    group: '图片',
    method: 'POST',
    path: '/v1/images/variations',
    title: '图片变体',
    description:
      '保留标准接口；仅模型声明 supports_variation=true 时可用，当前未支持的插件返回 400。',
    notes: ['model、image 文件必填；n、size、response_format 同图片生成。'],
    form: ['model=gpt-image-2', 'image=@reference.png'],
    response: {
      error: {
        message: '当前模型不支持此图片接口',
        type: 'invalid_request_error',
        code: 'unsupported_parameter',
        param: null,
      },
    },
    status: 400,
  },
  {
    group: '排队',
    method: 'POST',
    path: '/v1/images/generations',
    title: '异步排队示例',
    description: '持久化排队，允许轮询、取消、提升优先级和结果回调；对文本、音频接口无效。',
    notes: imageNotes,
    body: { model: 'gpt-image-2', prompt: '一只窗边的猫', sync: false },
    status: 202,
    response: {
      success: true,
      data: { taskId: 'task-example', status: 'queued', message: '开始排队生成中' },
    },
  },
  {
    group: '排队',
    method: 'GET',
    path: '/api/v2/generation-queue',
    title: '查询等待队列',
    description: '仅返回当前 API Key 拥有的任务。',
    notes: ['limit：1–100，默认 40；offset：默认 0。'],
    response: {
      success: true,
      data: {
        total: 1,
        items: [{ taskId: 'task-example', modelId: 'gpt-image-2', status: 'queued', position: 1 }],
      },
    },
  },
  {
    group: '排队',
    method: 'GET',
    path: '/api/v2/generation-tasks/{task_id}',
    title: '查询任务与图片结果',
    description:
      '完成后 result 包含 OpenAI 图片响应，data[].b64_json 即图片内容；任务 ID 不能跨 API Key 访问。',
    notes: [
      'task_id：入队时返回的 taskId。状态：pending/queued/processing/completed/failed/cancelled，以返回状态为准。',
    ],
    response: {
      success: true,
      data: {
        taskId: 'task-example',
        status: 'completed',
        position: null,
        result: image,
        failureReason: null,
      },
    },
  },
  {
    group: '排队',
    method: 'POST',
    path: '/api/v2/generation-tasks/batch',
    title: '批量查询图片任务',
    description: '单次最多 1000 个任务 ID。',
    notes: ['taskIds：非空字符串数组。不存在或无权访问的任务进入 notFoundTaskIds。'],
    body: { taskIds: ['task-example'] },
    response: {
      success: true,
      data: {
        items: [{ taskId: 'task-example', status: 'completed', result: image }],
        notFoundTaskIds: [],
      },
    },
  },
  {
    group: '排队',
    method: 'POST',
    path: '/api/v2/generation-queue/actions',
    title: '取消或提升排队任务',
    description: 'promote 提升排队优先级，cancel 取消等待或执行任务；已结束任务不能重新生成。',
    notes: ['action：promote / cancel；taskIds：任务 ID 数组。'],
    body: { action: 'cancel', taskIds: ['task-example'] },
    response: { success: true, data: { action: 'cancel', affected: 1 } },
  },
]
