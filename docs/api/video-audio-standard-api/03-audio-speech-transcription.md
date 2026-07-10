## 音频生成

音频生成首期按“文本转语音”实现。

也就是 OpenAI Speech API 风格：

```text
POST /v1/audio/speech
```

不要把“音乐生成”混在首期音频生成接口里。

如果后续要做音乐生成，建议新增：

```text
POST /v1/audio/music
```

或：

```text
POST /v1/audio/generations
```

但首期“音频生成”明确指 TTS。

### POST `/v1/audio/speech`

根据文本生成语音。

### Content-Type

```http
Content-Type: application/json
```

### Body Parameters

#### `model: string`

必填。

语音模型 ID。

示例：

```json
{
  "model": "tts-1"
}
```

#### `input: string`

必填。

要朗读的文本。

示例：

```json
{
  "input": "欢迎使用 MediaGen。"
}
```

#### `voice: string`

必填或插件默认。

声音名称。

建议兼容 OpenAI 常见声音名，同时允许插件自定义：

- `alloy`
- `ash`
- `ballad`
- `coral`
- `echo`
- `fable`
- `nova`
- `onyx`
- `sage`
- `shimmer`

插件可以在 manifest 中声明支持的声音。

#### `response_format: string`

可选。

输出音频格式。

建议支持：

- `mp3`
- `opus`
- `aac`
- `flac`
- `wav`
- `pcm`

默认：

- `mp3`

#### `speed: number`

可选。

语速。

建议范围：

- `0.25..4.0`

默认：

- `1.0`

#### `instructions: string`

可选。

朗读风格、情绪、停顿、语气说明。

示例：

```json
{
  "instructions": "Use a calm and friendly tone, with clear pauses between sentences."
}
```

#### `language: string`

MediaGen 扩展字段。

可选。

语言代码，例如：

- `zh`
- `en`
- `ja`
- `ko`

如果不传，由模型或插件自动判断。

#### `stream_format: string`

可选。

如果后续支持流式音频，可以使用：

- `sse`
- `audio`

首期可以不实现。

### Response

默认返回二进制音频。

示例：

```http
Content-Type: audio/mpeg
Content-Disposition: attachment; filename="speech.mp3"
```

### Example

```bash
curl http://127.0.0.1:18080/v1/audio/speech \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "tts-1",
    "input": "欢迎使用 MediaGen。",
    "voice": "alloy",
    "response_format": "mp3",
    "speed": 1.0
  }' \
  --output speech.mp3
```

### JSON 返回扩展

标准 OpenAI Speech API 通常直接返回音频二进制。

MediaGen 如果需要方便本地任务系统管理，可以支持扩展字段：

```json
{
  "response_format": "json"
}
```

返回：

```json
{
  "id": "audio_abc123",
  "object": "audio.speech",
  "created_at": 1780000000,
  "model": "tts-1",
  "voice": "alloy",
  "format": "mp3",
  "content_url": "http://127.0.0.1:18080/v1/audio/speech/audio_abc123/content"
}
```

但首期为了兼容主流 SDK，默认应该返回二进制。

## 音频转文字

### POST `/v1/audio/transcriptions`

把音频文件转成文字。

该接口按 OpenAI Transcriptions API 风格设计。

### Content-Type

```http
Content-Type: multipart/form-data
```

### Body Parameters

#### `file: file`

必填。

音频文件。

建议支持格式：

- `mp3`
- `mp4`
- `mpeg`
- `mpga`
- `m4a`
- `wav`
- `webm`
- `ogg`
- `flac`

#### `model: string`

必填。

转写模型 ID。

示例：

```text
whisper-1
```

#### `language: string`

可选。

输入音频语言。

建议使用 ISO-639-1，例如：

- `zh`
- `en`
- `ja`
- `ko`

不传则自动识别。

#### `prompt: string`

可选。

给模型的提示词，用于提高专有名词、上下文、标点风格的识别准确度。

示例：

```text
这是一段关于 MediaGen 插件架构的中文会议录音。
```

#### `response_format: string`

可选。

建议支持：

- `json`
- `text`
- `srt`
- `verbose_json`
- `vtt`

默认：

- `json`

#### `temperature: number`

可选。

建议范围：

- `0..1`

默认：

- `0`

#### `timestamp_granularities: array`

可选。

当 `response_format` 为 `verbose_json` 时支持。

建议兼容：

- `word`
- `segment`

示例：

```text
timestamp_granularities[]=word
timestamp_granularities[]=segment
```

#### `diarization: boolean`

MediaGen 扩展字段。

可选。

是否尝试说话人分离。

首期可以不实现。

#### `speaker_count: number`

MediaGen 扩展字段。

可选。

预期说话人数。

首期可以不实现。

### Response: json

```json
{
  "text": "欢迎使用 MediaGen。"
}
```

### Response: verbose_json

```json
{
  "task": "transcribe",
  "language": "zh",
  "duration": 3.4,
  "text": "欢迎使用 MediaGen。",
  "segments": [
    {
      "id": 0,
      "seek": 0,
      "start": 0.0,
      "end": 3.4,
      "text": "欢迎使用 MediaGen。",
      "tokens": [],
      "temperature": 0.0,
      "avg_logprob": -0.1,
      "compression_ratio": 1.1,
      "no_speech_prob": 0.01
    }
  ],
  "words": [
    {
      "word": "欢迎",
      "start": 0.0,
      "end": 0.6
    }
  ]
}
```

### Example

```bash
curl http://127.0.0.1:18080/v1/audio/transcriptions \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  -F "model=whisper-1" \
  -F "file=@./meeting.wav" \
  -F "language=zh" \
  -F "response_format=verbose_json" \
  -F "timestamp_granularities[]=segment"
```
