````

## 音频翻译

音频翻译不是首期必做，但可以兼容 OpenAI 路径：

```text
POST /v1/audio/translations
````

语义：

- 输入非英文音频。
- 输出英文文本。

首期可以返回 `501 Not Implemented`：

```json
{
  "error": {
    "message": "当前模型暂不支持音频翻译",
    "type": "invalid_request_error",
    "param": "model",
    "code": "unsupported_capability"
  }
}
```

## 插件 manifest 能力声明

插件需要声明视频和音频能力，服务端才能正确暴露模型。

### 视频模型声明

```json
{
  "models": [
    {
      "model_id": "sora-video",
      "display_name": "Sora Video",
      "task_kind": "video",
      "aliases": ["sora-2"]
    }
  ],
  "video_capabilities": {
    "max_reference_images": 4,
    "supports_first_frame": true,
    "supports_last_frame": false,
    "supports_negative_prompt": false,
    "supported_seconds": [4, 8, 12],
    "supported_sizes": ["1280x720", "720x1280"],
    "supported_aspect_ratios": ["16:9", "9:16", "1:1"],
    "supported_output_formats": ["mp4"],
    "supports_stream": false
  }
}
```

### 音频生成模型声明

```json
{
  "models": [
    {
      "model_id": "tts-1",
      "display_name": "Text to Speech",
      "task_kind": "audio_speech"
    }
  ],
  "audio_speech_capabilities": {
    "voices": ["alloy", "nova", "shimmer"],
    "supported_formats": ["mp3", "wav", "aac"],
    "speed_range": [0.25, 4.0],
    "supports_instructions": true,
    "supports_stream": false
  }
}
```

### 音频转写模型声明

```json
{
  "models": [
    {
      "model_id": "whisper-1",
      "display_name": "Whisper",
      "task_kind": "audio_transcription"
    }
  ],
  "audio_transcription_capabilities": {
    "supported_formats": ["mp3", "mp4", "m4a", "wav", "webm", "ogg", "flac"],
    "supported_response_formats": ["json", "text", "srt", "verbose_json", "vtt"],
    "supports_word_timestamps": true,
    "supports_segment_timestamps": true,
    "supports_diarization": false
  }
}
```

## 服务端任务模型

现有任务模型应该扩展 `task_kind`，不要只围绕图片。

推荐任务类型：

- `image.generate`
- `image.edit`
- `video.generate`
- `audio.speech`
- `audio.transcribe`
- `audio.translate`

任务记录建议增加：

| 字段               | 说明                       |
| ------------------ | -------------------------- |
| `input_assets`     | 输入文件、参考图、参考音频 |
| `output_assets`    | 输出图片、视频、音频、文本 |
| `progress`         | 进度                       |
| `duration_seconds` | 音视频时长                 |
| `mime_type`        | 输出 MIME                  |
| `content_path`     | 本地文件路径或存储 key     |

## 文件与内容存储

视频和音频文件不适合全部放进 JSON。

推荐规则：

- 图片可以继续支持 `b64_json`。
- 视频默认返回 `content_url`。
- 音频生成默认返回二进制。
- 音频转文字默认返回 JSON。
- 大文件统一保存到本地任务结果目录。
- `/content` 接口负责读取文件并返回。

本地文件建议按任务分目录：

```text
storage/results/
  video_abc123/
    output.mp4
    thumbnail.jpg
    metadata.json
  audio_abc123/
    speech.mp3
    metadata.json
```

## 与现有图片接口的关系

视频和音频不要复制一套完全不同的系统。

应该复用：

- API Key 鉴权。
- 账号池授权。
- 插件模型发现。
- 任务队列。
- 浏览器会话。
- 插件 flow runner。
- 结果存储。
- 审计日志。
- OpenAI 风格错误响应。

但要新增：

- 视频请求解析。
- 音频请求解析。
- 二进制文件上传解析。
- 视频任务状态查询。
- 视频内容下载。
- 音频二进制响应。
- 转写 multipart 解析。

## 首期实现建议

第一阶段建议只做最稳的闭环：

1. `/v1/videos` 支持文生视频和单张参考图。
2. `/v1/videos/{id}` 支持查询状态。
3. `/v1/videos/{id}/content` 支持下载 mp4。
4. `/v1/audio/speech` 支持文本生成语音，返回 mp3。
5. `/v1/audio/transcriptions` 支持上传音频，返回 JSON 文本。
6. 插件 manifest 增加 `video_capabilities`、`audio_speech_capabilities`、`audio_transcription_capabilities`。
7. `/v1/models` 能返回这些模型。

暂不做：

- 视频编辑。
- 视频延长。
- 多参考图强一致性。
- 视频 SSE 进度。
- 音频流式播放。
- 说话人分离。
- 音乐生成。
- 超长音频异步转写。

## 验收标准

视频生成验收：

- 能通过 `/v1/models` 看到视频模型。
- 能用文本 prompt 创建视频任务。
- 能用单张参考图创建视频任务。
- 能轮询任务状态。
- 能下载 mp4。
- API Key 授权范围生效。
- 插件不支持的参数有明确错误。

音频生成验收：

- 能通过 `/v1/models` 看到 TTS 模型。
- 能调用 `/v1/audio/speech` 返回 mp3。
- `voice`、`response_format`、`speed` 至少能被解析。
- 不支持的 voice 有明确错误。

音频转文字验收：

- 能通过 `/v1/models` 看到转写模型。
- 能上传 wav/mp3。
- 能返回 `json`。
- 能返回 `text`。
- `language`、`prompt`、`response_format` 能被解析。
- 大文件超过限制时有明确错误。

兼容验收：

- 现有图片接口不受影响。
- 现有插件不需要修改也能继续工作。
- 没有视频/音频能力的插件不会出现在对应模型列表中。
- 服务端通用层不写死具体平台规则。

## 参考链接

- OpenAI Video API：<https://platform.openai.com/docs/api-reference/videos>
- OpenAI Video Guide：<https://platform.openai.com/docs/guides/video-generation>
- OpenAI Audio Speech API：<https://platform.openai.com/docs/api-reference/audio/createSpeech>
- OpenAI Audio Transcriptions API：<https://platform.openai.com/docs/api-reference/audio/createTranscription>
