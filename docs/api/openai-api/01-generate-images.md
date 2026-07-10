# Image generation

## Overview

The OpenAI API lets you generate and edit images from text prompts using GPT Image models, including our latest, `gpt-image-2`. You can access image generation capabilities through two APIs:

### Image API

Starting with `gpt-image-1` and later models, the [Image API](https://developers.openai.com/api/docs/api-reference/images) provides two endpoints, each with distinct capabilities:

- **Generations**: [Generate images](#generate-images) from scratch based on a text prompt
- **Edits**: [Modify existing images](#edit-images) using a new prompt, either partially or entirely

The Image API also includes a variations endpoint for models that support it, such as DALL·E 2.

### Responses API

The [Responses API](https://developers.openai.com/api/docs/api-reference/responses/create#responses-create-tools) allows you to generate images as part of conversations or multi-step flows. It supports image generation as a [built-in tool](https://developers.openai.com/api/docs/guides/tools?api-mode=responses), and accepts image inputs and outputs within context.

Compared to the Image API, it adds:

- **Multi-turn editing**: Iteratively make high fidelity edits to images with prompting
- **Flexible inputs**: Accept image [File](https://developers.openai.com/api/docs/api-reference/files) IDs as input images, not just bytes

The Responses API image generation tool uses its own GPT Image model selection. For details on mainline models that support calling this tool, refer to the [supported models](#supported-models) below.

### Choosing the right API

- If you only need to generate or edit a single image from one prompt, the Image API is your best choice.
- If you want to build conversational, editable image experiences with GPT Image, go with the Responses API.

Both APIs let you [customize output](#customize-image-output) by adjusting quality, size, format, and compression. Transparent backgrounds depend on model support.

This guide focuses on GPT Image.

To ensure these models are used responsibly, you may need to complete the [API
Organization
Verification](https://help.openai.com/en/articles/10910291-api-organization-verification)
from your [developer
console](https://platform.openai.com/settings/organization/general) before
using GPT Image models, including `gpt-image-2`, `gpt-image-1.5`,
`gpt-image-1`, and `gpt-image-1-mini`.

<div
  className="not-prose"
  style={{ float: "right", margin: "10px 0 10px 10px" }}
>
  <img src="https://cdn.openai.com/API/docs/images/mug.png"
    alt="A beige coffee mug on a wooden table"
    style={{ height: "180px", width: "auto", borderRadius: "8px" }}
  />
</div>

## Generate Images

You can use the [image generation endpoint](https://developers.openai.com/api/docs/api-reference/images/create) to create images based on text prompts, or the [image generation tool](https://developers.openai.com/api/docs/guides/tools?api-mode=responses) in the Responses API to generate images as part of a conversation.

To learn more about customizing the output (size, quality, format, compression), refer to the [customize image output](#customize-image-output) section below.

You can set the `n` parameter to generate multiple images at once in a single request (by default, the API returns a single image).

<div data-content-switcher-pane data-value="responses">
    <div class="hidden">Responses API</div>
    Generate an image

```javascript
import OpenAI from 'openai'
const openai = new OpenAI()

const response = await openai.responses.create({
  model: 'gpt-5.5',
  input: 'Generate an image of gray tabby cat hugging an otter with an orange scarf',
  tools: [{ type: 'image_generation' }],
})

// Save the image to a file
const imageData = response.output
  .filter((output) => output.type === 'image_generation_call')
  .map((output) => output.result)

if (imageData.length > 0) {
  const imageBase64 = imageData[0]
  const fs = await import('fs')
  fs.writeFileSync('otter.png', Buffer.from(imageBase64, 'base64'))
}
```

```python
from openai import OpenAI
import base64

client = OpenAI()

response = client.responses.create(
    model="gpt-5.5",
    input="Generate an image of gray tabby cat hugging an otter with an orange scarf",
    tools=[{"type": "image_generation"}],
)

# Save the image to a file
image_data = [
    output.result
    for output in response.output
    if output.type == "image_generation_call"
]

if image_data:
    image_base64 = image_data[0]
    with open("otter.png", "wb") as f:
        f.write(base64.b64decode(image_base64))
```

  </div>
  <div data-content-switcher-pane data-value="image" hidden>
    <div class="hidden">Image API</div>
    Generate an image

```javascript
import OpenAI from "openai";
import fs from "fs";
const openai = new OpenAI();

const prompt = \`
A children's book drawing of a veterinarian using a stethoscope to
listen to the heartbeat of a baby otter.
\`;

const result = await openai.images.generate({
    model: "gpt-image-2",
    prompt,
});

// Save the image to a file
const image_base64 = result.data[0].b64_json;
const image_bytes = Buffer.from(image_base64, "base64");
fs.writeFileSync("otter.png", image_bytes);
```

```python
from openai import OpenAI
import base64
client = OpenAI()

prompt = """
A children's book drawing of a veterinarian using a stethoscope to
listen to the heartbeat of a baby otter.
"""

result = client.images.generate(
    model="gpt-image-2",
    prompt=prompt
)

image_base64 = result.data[0].b64_json
image_bytes = base64.b64decode(image_base64)

# Save the image to a file
with open("otter.png", "wb") as f:
    f.write(image_bytes)
```

```bash
curl -X POST "https://api.openai.com/v1/images/generations" \\
    -H "Authorization: Bearer $OPENAI_API_KEY" \\
    -H "Content-type: application/json" \\
    -d '{
        "model": "gpt-image-2",
        "prompt": "A childrens book drawing of a veterinarian using a stethoscope to listen to the heartbeat of a baby otter."
    }' | jq -r '.data[0].b64_json' | base64 --decode > otter.png
```

```cli
openai images generate \\
  --model gpt-image-2 \\
  --prompt "A childrens book drawing of a veterinarian using a stethoscope to listen to the heartbeat of a baby otter." \\
  --raw-output \\
  --transform 'data.0.b64_json' | base64 --decode > otter.png
```

  </div>

### Multi-turn image generation

With the Responses API, you can build multi-turn conversations involving image generation either by providing image generation calls outputs within context (you can also just use the image ID), or by using the [`previous_response_id` parameter](https://developers.openai.com/api/docs/guides/conversation-state?api-mode=responses#openai-apis-for-conversation-state).
This lets you iterate on images across multiple turns—refining prompts, applying new instructions, and evolving the visual output as the conversation progresses.

With the Responses API image generation tool, supported tool models can choose whether to generate a new image or edit one already in the conversation. The optional `action` parameter controls this behavior: keep `action: "auto"` to let the model decide, set `action: "generate"` to always create a new image, or set `action: "edit"` to force editing when an image is in context.

Force image creation with action

```javascript
import OpenAI from 'openai'
const openai = new OpenAI()

const response = await openai.responses.create({
  model: 'gpt-5.5',
  input: 'Generate an image of gray tabby cat hugging an otter with an orange scarf',
  tools: [{ type: 'image_generation', action: 'generate' }],
})

// Save the image to a file
const imageData = response.output
  .filter((output) => output.type === 'image_generation_call')
  .map((output) => output.result)

if (imageData.length > 0) {
  const imageBase64 = imageData[0]
  const fs = await import('fs')
  fs.writeFileSync('otter.png', Buffer.from(imageBase64, 'base64'))
}
```

```python
from openai import OpenAI
import base64

client = OpenAI()

response = client.responses.create(
    model="gpt-5.5",
    input="Generate an image of gray tabby cat hugging an otter with an orange scarf",
    tools=[{"type": "image_generation", "action": "generate"}],
)

# Save the image to a file
image_data = [
    output.result
    for output in response.output
    if output.type == "image_generation_call"
]

if image_data:
    image_base64 = image_data[0]
    with open("otter.png", "wb") as f:
        f.write(base64.b64decode(image_base64))
```

If you force `edit` without providing an image in context, the call will return an error. Leave `action` at `auto` to have the model decide when to generate or edit.

<div data-content-switcher-pane data-value="responseid">
    <div class="hidden">Using previous response ID</div>
    Multi-turn image generation

```javascript
import OpenAI from 'openai'
const openai = new OpenAI()

const response = await openai.responses.create({
  model: 'gpt-5.5',
  input: 'Generate an image of gray tabby cat hugging an otter with an orange scarf',
  tools: [{ type: 'image_generation' }],
})

const imageData = response.output
  .filter((output) => output.type === 'image_generation_call')
  .map((output) => output.result)

if (imageData.length > 0) {
  const imageBase64 = imageData[0]
  const fs = await import('fs')
  fs.writeFileSync('cat_and_otter.png', Buffer.from(imageBase64, 'base64'))
}

// Follow up

const response_fwup = await openai.responses.create({
  model: 'gpt-5.5',
  previous_response_id: response.id,
  input: 'Now make it look realistic',
  tools: [{ type: 'image_generation' }],
})

const imageData_fwup = response_fwup.output
  .filter((output) => output.type === 'image_generation_call')
  .map((output) => output.result)

if (imageData_fwup.length > 0) {
  const imageBase64 = imageData_fwup[0]
  const fs = await import('fs')
  fs.writeFileSync('cat_and_otter_realistic.png', Buffer.from(imageBase64, 'base64'))
}
```

```python
from openai import OpenAI
import base64

client = OpenAI()

response = client.responses.create(
    model="gpt-5.5",
    input="Generate an image of gray tabby cat hugging an otter with an orange scarf",
    tools=[{"type": "image_generation"}],
)

image_data = [
    output.result
    for output in response.output
    if output.type == "image_generation_call"
]

if image_data:
    image_base64 = image_data[0]

    with open("cat_and_otter.png", "wb") as f:
        f.write(base64.b64decode(image_base64))


# Follow up

response_fwup = client.responses.create(
    model="gpt-5.5",
    previous_response_id=response.id,
    input="Now make it look realistic",
    tools=[{"type": "image_generation"}],
)

image_data_fwup = [
    output.result
    for output in response_fwup.output
    if output.type == "image_generation_call"
]

if image_data_fwup:
    image_base64 = image_data_fwup[0]
    with open("cat_and_otter_realistic.png", "wb") as f:
        f.write(base64.b64decode(image_base64))
```

  </div>
  <div data-content-switcher-pane data-value="imageid" hidden>
    <div class="hidden">Using image ID</div>
    Multi-turn image generation

```javascript
import OpenAI from 'openai'
const openai = new OpenAI()

const response = await openai.responses.create({
  model: 'gpt-5.5',
  input: 'Generate an image of gray tabby cat hugging an otter with an orange scarf',
  tools: [{ type: 'image_generation' }],
})

const imageGenerationCalls = response.output.filter(
  (output) => output.type === 'image_generation_call'
)

const imageData = imageGenerationCalls.map((output) => output.result)

if (imageData.length > 0) {
  const imageBase64 = imageData[0]
  const fs = await import('fs')
  fs.writeFileSync('cat_and_otter.png', Buffer.from(imageBase64, 'base64'))
}

// Follow up

const response_fwup = await openai.responses.create({
  model: 'gpt-5.5',
  input: [
    {
      role: 'user',
      content: [{ type: 'input_text', text: 'Now make it look realistic' }],
    },
    {
      type: 'image_generation_call',
      id: imageGenerationCalls[0].id,
    },
  ],
  tools: [{ type: 'image_generation' }],
})

const imageData_fwup = response_fwup.output
  .filter((output) => output.type === 'image_generation_call')
  .map((output) => output.result)

if (imageData_fwup.length > 0) {
  const imageBase64 = imageData_fwup[0]
  const fs = await import('fs')
  fs.writeFileSync('cat_and_otter_realistic.png', Buffer.from(imageBase64, 'base64'))
}
```

```python
import openai
import base64

response = openai.responses.create(
    model="gpt-5.5",
    input="Generate an image of gray tabby cat hugging an otter with an orange scarf",
    tools=[{"type": "image_generation"}],
)

image_generation_calls = [
    output
    for output in response.output
    if output.type == "image_generation_call"
]

image_data = [output.result for output in image_generation_calls]

if image_data:
    image_base64 = image_data[0]

    with open("cat_and_otter.png", "wb") as f:
        f.write(base64.b64decode(image_base64))


# Follow up

response_fwup = openai.responses.create(
    model="gpt-5.5",
    input=[
        {
            "role": "user",
            "content": [{"type": "input_text", "text": "Now make it look realistic"}],
        },
        {
            "type": "image_generation_call",
            "id": image_generation_calls[0].id,
        },
    ],
    tools=[{"type": "image_generation"}],
)

image_data_fwup = [
    output.result
    for output in response_fwup.output
    if output.type == "image_generation_call"
]

if image_data_fwup:
    image_base64 = image_data_fwup[0]
    with open("cat_and_otter_realistic.png", "wb") as f:
        f.write(base64.b64decode(image_base64))
```

  </div>

#### Result

<div className="not-prose">
  <table style={{ width: "100%" }}>
    <tbody>
      <tr>
        <td style={{ verticalAlign: "top", padding: "0 16px 16px 0" }}>
          "Generate an image of gray tabby cat hugging an otter with an orange
          scarf"
        </td>
        <td
          style={{
            textAlign: "right",
            verticalAlign: "top",
            paddingBottom: "16px",
          }}
        >
          <img src="https://cdn.openai.com/API/docs/images/cat_and_otter.png"
            alt="A cat and an otter"
            style={{ width: "200px", borderRadius: "8px" }}
          />
        </td>
      </tr>
      <tr>
        <td style={{ verticalAlign: "top", padding: "0 16px 0 0" }}>
          "Now make it look realistic"
        </td>
        <td style={{ textAlign: "right", verticalAlign: "top" }}>
          <img src="https://cdn.openai.com/API/docs/images/cat_and_otter_realistic.png"
            alt="A cat and an otter"
            style={{ width: "200px", borderRadius: "8px" }}
          />
        </td>
      </tr>
    </tbody>
  </table>
</div>
