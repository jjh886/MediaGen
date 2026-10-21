# OpenAI 兼容视频接口

本文说明当前 MediaGen 已实现的接口。豆包（`doubao-api`）与 Grok 共用 `/v1/videos`，通过 `model` 选择插件；没有单独的豆包专用路由。控制台「接口文档 → 视频」提供可复制的调用示例。

## 准备

1. 在控制台登录对应平台账号，将账号池授权给调用方的 API Key。
2. 使用 `GET /v1/models` 查询 `task_kind=video` 的模型。模型在目录中不代表账号一定有使用权限或剩余额度。
3. 设置服务地址和密钥环境变量：

```sh
export BASE_URL="http://127.0.0.1:18080"
export MEDIAGEN_API_KEY="你的 MediaGen API Key"
```

## 豆包模型

| model                    | 名称              | 输入                 |
| ------------------------ | ----------------- | -------------------- |
| `doubao-seedance-2-5`    | Seedance 2.5      | 文本、图片           |
| `doubao-seedance-2`      | Seedance 2.0      | 文本、图片           |
| `doubao-seedance-2-fast` | Seedance 2.0 Fast | 文本、图片、参考视频 |
| `doubao-seedance-2-mini` | Seedance 2.0 Mini | 文本、图片           |

插件按当前账号的平台配置检查具体模型、时长及比例，不自动切换到其他模型。以上为豆包网页 API 插件，不需要安装 Doubao Work。

## 创建视频

`POST /v1/videos` 支持 JSON 和 multipart/form-data，返回 HTTP 200 及异步任务对象。无需传 `async`、`sync` 或 `stream`；不提供视频 SSE 流。

| 字段               | 类型           | 必填 | 说明                                                                                |
| ------------------ | -------------- | ---- | ----------------------------------------------------------------------------------- |
| `model`            | string         | 是   | 上表中的模型 ID，或模型目录中的其他视频模型                                         |
| `prompt`           | string         | 是   | 最多 32000 字符                                                                     |
| `seconds`          | integer/string | 否   | 豆包默认 10；插件总范围为 4–30 秒，具体模型的范围及步长仍由平台校验                 |
| `size`             | string         | 否   | 豆包默认 `1280x720`，格式为正整数 `宽x高`，用于推导比例，不保证精确输出像素         |
| `quality`          | string         | 否   | 默认 `standard`；当前豆包插件不使用此字段选择清晰度                                 |
| `input_reference`  | object/文件    | 否   | 参考图：JSON `{file_id}`、`{image_url}`，或 multipart 图片文件                      |
| `input_references` | array<object>  | 否   | 多图扩展，每项结构同 `input_reference`；multipart 使用重复字段 `input_references[]` |
| `video_reference`  | object         | 否   | 仅 JSON，参考视频扩展；使用 `{file_id}` 或 `{filename, file_data}`                  |
| `user`             | string         | 否   | 终端用户标识                                                                        |

豆包比例可通过尺寸表达，例如 `1280x720` → `16:9`、`720x1280` → `9:16`、`1024x1024` → `1:1`、`960x1280` → `3:4`、`1280x960` → `4:3`、`1680x720` → `21:9`。当前具体模型还需支持所选比例；不支持时失败，不近似替换。

### 文生视频

```sh
curl "$BASE_URL/v1/videos" \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "doubao-seedance-2-5",
    "prompt": "云层缓慢流过山谷，镜头平稳向前推进",
    "seconds": "10",
    "size": "1280x720"
  }'
```

响应包含以下字段，`id` 用于后续查询：

```json
{
  "id": "video_example",
  "object": "video",
  "model": "doubao-seedance-2-5",
  "status": "queued",
  "progress": 0,
  "seconds": "10",
  "size": "1280x720",
  "quality": "standard"
}
```

### 参考图生成视频

参考图为 PNG、JPEG 或 WebP，单张不超过 10 MiB，合计最多 10 张，最终限制由具体模型决定。视频创建请求体上限 50 MiB。

```sh
curl "$BASE_URL/v1/videos" \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  -F 'model=doubao-seedance-2-fast' \
  -F 'prompt=让参考图中的人物自然转身，保持外貌一致' \
  -F 'seconds=10' \
  -F 'size=720x1280' \
  -F 'input_reference=@reference.png'
```

JSON 可使用 `"input_reference":{"file_id":"上传返回的 ID"}`，或 `"input_reference":{"image_url":"data:image/png;base64,..."}`；也支持远程图片 URL。

### 参考视频生成视频

使用 `doubao-seedance-2-fast`，最多 1 个 MP4/MOV。先上传文件（最多 50 MiB）：

```sh
curl "$BASE_URL/v1/files" \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  -F 'purpose=assistants' \
  -F 'file=@reference.mp4'
```

将上传返回的 `id` 填入 `video_reference.file_id`：

```sh
curl "$BASE_URL/v1/videos" \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "doubao-seedance-2-fast",
    "prompt": "参考视频中的运动方式，生成一段海边漫步镜头",
    "seconds": "10",
    "size": "1280x720",
    "video_reference": {"file_id": "file_example"}
  }'
```

也可传 `video_reference: {"filename":"reference.mp4","file_data":"data:video/mp4;base64,..."}`。内联 Base64 的编码开销计入 50 MiB 请求体限制，较大文件应先上传再引用。参考片时长上限由账号所选模型动态校验。

当前不支持直接 multipart 上传参考视频、参考视频远程 URL、服务器本地路径或音频参考。聊天中分析视频应使用 `/v1/responses` 的 `input_file` 或 `/v1/chat/completions` 的 `file`，与此处的视频生成不同。

## 查询、下载和管理

| 方法   | 路径                            | 说明                                                                                        |
| ------ | ------------------------------- | ------------------------------------------------------------------------------------------- |
| GET    | `/v1/videos`                    | 当前 Key 的视频列表；`after` 游标、`limit=1..100`（默认 20）、`order=asc/desc`（默认 desc） |
| GET    | `/v1/videos/{video_id}`         | 查询状态与失败信息                                                                          |
| GET    | `/v1/videos/{video_id}/content` | 下载视频；有封面时支持 `?variant=thumbnail`                                                 |
| POST   | `/v1/videos/{video_id}/cancel`  | 取消未结束任务（MediaGen 扩展）；已结束返回 409                                             |
| DELETE | `/v1/videos/{video_id}`         | 删除已结束任务；执行中需先取消                                                              |

状态为 `queued`、`in_progress`、`completed` 或 `failed`。取消也映射为 `failed`，错误码为 `video_cancelled`。创建成功仅表示任务被接受，平台权限、验证或生成失败可能在后续任务的 `error` 中返回。

```sh
export VIDEO_ID="创建返回的 video_id"
curl "$BASE_URL/v1/videos/$VIDEO_ID" \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY"
# 确认 status=completed 后下载
curl "$BASE_URL/v1/videos/$VIDEO_ID/content" \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  --output result.mp4
```

未完成时下载返回 409。下载接口流式转发视频字节，任务 JSON 不包含视频 Base64。文件与任务均按 API Key 隔离，所有步骤须使用同一 Key。遇到账号验证，需在控制台完成验证；微信通知需预先绑定和配置。

豆包平台生成完成后，创作目录或无水印下载地址可能尚未同步。插件最多等待 5 分钟，每 5 秒查询原视频下载地址并执行账号心跳；此期间本地任务仍可能为 `in_progress`。等待不会重新提交生成。到时仍未就绪则报告 `download_pending`；登录失效或需要人工验证时立即返回相应错误。

平台明确返回额度耗尽时报告 `quota_exhausted`，有有效重置时间时错误原因包含 `retry after N seconds`；订阅权限不足报告 `platform_rejected`。这些提示不会作为成功视频结果返回，客户端应检查任务的 `status` 和 `error`。

插件视频结果兼容 `download: {url, authorization: "url"}` 下载描述符与已有 URL 字段。对调用方仍通过 `/v1/videos/{video_id}/content` 下载，账号及 API Key 权限要求保持一致；未支持的下载鉴权方式不会被当作可用结果。
