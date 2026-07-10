### Streaming

The Responses API and Image API support streaming image generation. You can stream partial images as the APIs generate them, providing a more interactive experience.

You can adjust the `partial_images` parameter to receive 0-3 partial images.

- If you set `partial_images` to 0, you will only receive the final image.
- For values larger than zero, you may not receive the full number of partial images you requested if the full image is generated more quickly.

<div data-content-switcher-pane data-value="responses">
    <div class="hidden">Responses API</div>
    Stream an image

```javascript
import OpenAI from "openai";
import fs from "fs";
const openai = new OpenAI();

const stream = await openai.responses.create({
  model: "gpt-5.5",
  input:
    "Draw a gorgeous image of a river made of white owl feathers, snaking its way through a serene winter landscape",
  stream: true,
  tools: [{ type: "image_generation", partial_images: 2 }],
});

for await (const event of stream) {
  if (event.type === "response.image_generation_call.partial_image") {
    const idx = event.partial_image_index;
    const imageBase64 = event.partial_image_b64;
    const imageBuffer = Buffer.from(imageBase64, "base64");
    fs.writeFileSync(\`river\${idx}.png\`, imageBuffer);
  }
}
```

```python
from openai import OpenAI
import base64

client = OpenAI()

stream = client.responses.create(
    model="gpt-5.5",
    input="Draw a gorgeous image of a river made of white owl feathers, snaking its way through a serene winter landscape",
    stream=True,
    tools=[{"type": "image_generation", "partial_images": 2}],
)

for event in stream:
    if event.type == "response.image_generation_call.partial_image":
        idx = event.partial_image_index
        image_base64 = event.partial_image_b64
        image_bytes = base64.b64decode(image_base64)
        with open(f"river{idx}.png", "wb") as f:
            f.write(image_bytes)
```

  </div>
  <div data-content-switcher-pane data-value="image" hidden>
    <div class="hidden">Image API</div>
    Stream an image

```javascript
import fs from "fs";
import OpenAI from "openai";

const openai = new OpenAI();

const prompt =
  "Draw a gorgeous image of a river made of white owl feathers, snaking its way through a serene winter landscape";
const stream = await openai.images.generate({
  prompt: prompt,
  model: "gpt-image-2",
  stream: true,
  partial_images: 2,
});

for await (const event of stream) {
  if (event.type === "image_generation.partial_image") {
    const idx = event.partial_image_index;
    const imageBase64 = event.b64_json;
    const imageBuffer = Buffer.from(imageBase64, "base64");
    fs.writeFileSync(\`river\${idx}.png\`, imageBuffer);
  }
}
```

```python
from openai import OpenAI
import base64

client = OpenAI()

stream = client.images.generate(
    prompt="Draw a gorgeous image of a river made of white owl feathers, snaking its way through a serene winter landscape",
    model="gpt-image-2",
    stream=True,
    partial_images=2,
)

for event in stream:
    if event.type == "image_generation.partial_image":
        idx = event.partial_image_index
        image_base64 = event.b64_json
        image_bytes = base64.b64decode(image_base64)
        with open(f"river{idx}.png", "wb") as f:
            f.write(image_bytes)
```

  </div>

#### Result

<div className="images-examples">

| Partial 1                                                                                                                       | Partial 2                                                                                                                       | Final image                                                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| <img className="images-example-image" src="https://cdn.openai.com/API/docs/images/imgen1p5-streaming1.png" alt="1st partial" /> | <img className="images-example-image" src="https://cdn.openai.com/API/docs/images/imgen1p5-streaming2.png" alt="2nd partial" /> | <img className="images-example-image" src="https://cdn.openai.com/API/docs/images/imgen1p5-streaming3.png" alt="3rd partial" /> |

</div>

<div className="images-edit-prompt body-small">
  Prompt: Draw a gorgeous image of a river made of white owl feathers, snaking
  its way through a serene winter landscape
</div>

### Revised prompt

When using the image generation tool in the Responses API, the mainline model (for example, `gpt-5.5`) will automatically revise your prompt for improved performance.

You can access the revised prompt in the `revised_prompt` field of the image generation call:

Revised prompt response

```json
{
  "id": "ig_123",
  "type": "image_generation_call",
  "status": "completed",
  "revised_prompt": "A gray tabby cat hugging an otter. The otter is wearing an orange scarf. Both animals are cute and friendly, depicted in a warm, heartwarming style.",
  "result": "..."
}
```

## Edit Images

The [image edits](https://developers.openai.com/api/docs/api-reference/images/createEdit) endpoint lets you:

- Edit existing images
- Generate new images using other images as a reference
- Edit parts of an image by uploading an image and mask that identifies the areas to replace

### Create a new image using image references

You can use one or more images as a reference to generate a new image.

In this example, we'll use 4 input images to generate a new image of a gift basket containing the items in the reference images.

<div data-content-switcher-pane data-value="responses">
    <div class="hidden">Responses API</div>
    </div>
  <div data-content-switcher-pane data-value="image" hidden>
    <div class="hidden">Image API</div>
    Edit an image

```python
import base64
from openai import OpenAI
client = OpenAI()

prompt = """
Generate a photorealistic image of a gift basket on a white background
labeled 'Relax & Unwind' with a ribbon and handwriting-like font,
containing all the items in the reference pictures.
"""

result = client.images.edit(
    model="gpt-image-2",
    image=[
        open("body-lotion.png", "rb"),
        open("bath-bomb.png", "rb"),
        open("incense-kit.png", "rb"),
        open("soap.png", "rb"),
    ],
    prompt=prompt
)

image_base64 = result.data[0].b64_json
image_bytes = base64.b64decode(image_base64)

# Save the image to a file
with open("gift-basket.png", "wb") as f:
    f.write(image_bytes)
```

```javascript
import fs from "fs";
import OpenAI, { toFile } from "openai";

const client = new OpenAI();

const prompt = \`
Generate a photorealistic image of a gift basket on a white background
labeled 'Relax & Unwind' with a ribbon and handwriting-like font,
containing all the items in the reference pictures.
\`;

const imageFiles = [
    "bath-bomb.png",
    "body-lotion.png",
    "incense-kit.png",
    "soap.png",
];

const images = await Promise.all(
    imageFiles.map(async (file) =>
        await toFile(fs.createReadStream(file), null, {
            type: "image/png",
        })
    ),
);

const response = await client.images.edit({
    model: "gpt-image-2",
    image: images,
    prompt,
});

// Save the image to a file
const image_base64 = response.data[0].b64_json;
const image_bytes = Buffer.from(image_base64, "base64");
fs.writeFileSync("basket.png", image_bytes);
```

```bash
curl -s -D >(grep -i x-request-id >&2) \\
  -o >(jq -r '.data[0].b64_json' | base64 --decode > gift-basket.png) \\
  -X POST "https://api.openai.com/v1/images/edits" \\
  -H "Authorization: Bearer $OPENAI_API_KEY" \\
  -F "model=gpt-image-2" \\
  -F "image[]=@body-lotion.png" \\
  -F "image[]=@bath-bomb.png" \\
  -F "image[]=@incense-kit.png" \\
  -F "image[]=@soap.png" \\
  -F 'prompt=Generate a photorealistic image of a gift basket on a white background labeled "Relax & Unwind" with a ribbon and handwriting-like font, containing all the items in the reference pictures'
```

```cli
openai images edit \\
  --model gpt-image-2 \\
  --image body-lotion.png \\
  --image bath-bomb.png \\
  --image incense-kit.png \\
  --image soap.png \\
  --prompt 'Generate a photorealistic image of a gift basket on a white background labeled "Relax & Unwind" with a ribbon and handwriting-like font, containing all the items in the reference pictures' \\
  --raw-output \\
  --transform 'data.0.b64_json' | base64 --decode > gift-basket.png
```

  </div>

### Edit an image using a mask

You can provide a mask to indicate which part of the image should be edited.

When using a mask with GPT Image, additional instructions are sent to the model to help guide the editing process accordingly.

Masking with GPT Image is entirely prompt-based. The model uses the mask as
guidance, but may not follow its exact shape with complete precision.

If you provide multiple input images, the mask will be applied to the first image.

<div data-content-switcher-pane data-value="responses">
    <div class="hidden">Responses API</div>
    Edit an image with a mask

```python
from openai import OpenAI
client = OpenAI()

fileId = create_file("sunlit_lounge.png")
maskId = create_file("mask.png")

response = client.responses.create(
    model="gpt-5.5",
    input=[
        {
            "role": "user",
            "content": [
                {
                    "type": "input_text",
                    "text": "generate an image of the same sunlit indoor lounge area with a pool but the pool should contain a flamingo",
                },
                {
                    "type": "input_image",
                    "file_id": fileId,
                }
            ],
        },
    ],
    tools=[
        {
            "type": "image_generation",
            "quality": "high",
            "input_image_mask": {
                "file_id": maskId,
            }
        },
    ],
)

image_data = [
    output.result
    for output in response.output
    if output.type == "image_generation_call"
]

if image_data:
    image_base64 = image_data[0]
    with open("lounge.png", "wb") as f:
        f.write(base64.b64decode(image_base64))
```

```javascript
import OpenAI from 'openai'
const openai = new OpenAI()

const fileId = await createFile('sunlit_lounge.png')
const maskId = await createFile('mask.png')

const response = await openai.responses.create({
  model: 'gpt-5.5',
  input: [
    {
      role: 'user',
      content: [
        {
          type: 'input_text',
          text: 'generate an image of the same sunlit indoor lounge area with a pool but the pool should contain a flamingo',
        },
        {
          type: 'input_image',
          file_id: fileId,
        },
      ],
    },
  ],
  tools: [
    {
      type: 'image_generation',
      quality: 'high',
      input_image_mask: {
        file_id: maskId,
      },
    },
  ],
})

const imageData = response.output
  .filter((output) => output.type === 'image_generation_call')
  .map((output) => output.result)

if (imageData.length > 0) {
  const imageBase64 = imageData[0]
  const fs = await import('fs')
  fs.writeFileSync('lounge.png', Buffer.from(imageBase64, 'base64'))
}
```

  </div>
  <div data-content-switcher-pane data-value="image" hidden>
    <div class="hidden">Image API</div>
    Edit an image with a mask

```python
from openai import OpenAI
client = OpenAI()

result = client.images.edit(
    model="gpt-image-2",
    image=open("sunlit_lounge.png", "rb"),
    mask=open("mask.png", "rb"),
    prompt="A sunlit indoor lounge area with a pool containing a flamingo"
)

image_base64 = result.data[0].b64_json
image_bytes = base64.b64decode(image_base64)

# Save the image to a file
with open("composition.png", "wb") as f:
    f.write(image_bytes)
```

```javascript
import fs from 'fs'
import OpenAI, { toFile } from 'openai'

const client = new OpenAI()

const rsp = await client.images.edit({
  model: 'gpt-image-2',
  image: await toFile(fs.createReadStream('sunlit_lounge.png'), null, {
    type: 'image/png',
  }),
  mask: await toFile(fs.createReadStream('mask.png'), null, {
    type: 'image/png',
  }),
  prompt: 'A sunlit indoor lounge area with a pool containing a flamingo',
})

// Save the image to a file
const image_base64 = rsp.data[0].b64_json
const image_bytes = Buffer.from(image_base64, 'base64')
fs.writeFileSync('lounge.png', image_bytes)
```

```bash
curl -s -D >(grep -i x-request-id >&2) \\
  -o >(jq -r '.data[0].b64_json' | base64 --decode > lounge.png) \\
  -X POST "https://api.openai.com/v1/images/edits" \\
  -H "Authorization: Bearer $OPENAI_API_KEY" \\
  -F "model=gpt-image-2" \\
  -F "mask=@mask.png" \\
  -F "image[]=@sunlit_lounge.png" \\
  -F 'prompt=A sunlit indoor lounge area with a pool containing a flamingo'
```

```cli
openai images edit \\
  --model gpt-image-2 \\
  --image sunlit_lounge.png \\
  --mask mask.png \\
  --prompt "A sunlit indoor lounge area with a pool containing a flamingo" \\
  --raw-output \\
  --transform 'data.0.b64_json' | base64 --decode > out.png
```

  </div>

<div className="images-examples">

| Image                                                                                                                                 | Mask                                                                                                                            | Output                                                                                                                                                                               |
| ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| <img className="images-example-image" src="https://cdn.openai.com/API/docs/images/sunlit_lounge.png" alt="A pink room with a pool" /> | <img className="images-example-image" src="https://cdn.openai.com/API/docs/images/mask.png" alt="A mask in part of the pool" /> | <img className="images-example-image" src="https://cdn.openai.com/API/docs/images/sunlit_lounge_result.png" alt="The original pool with an inflatable flamigo replacing the mask" /> |

</div>

<div className="images-edit-prompt body-small">
  Prompt: a sunlit indoor lounge area with a pool containing a flamingo
</div>

#### Mask requirements

The image to edit and mask must be of the same format and size (less than 50MB in size).

The mask image must also contain an alpha channel. If you're using an image editing tool to create the mask, make sure to save the mask with an alpha channel.

You can modify a black and white image programmatically to add an alpha channel.

Add an alpha channel to a black and white mask

```python
from PIL import Image
from io import BytesIO

# 1. Load your black & white mask as a grayscale image
mask = Image.open(img_path_mask).convert("L")

# 2. Convert it to RGBA so it has space for an alpha channel
mask_rgba = mask.convert("RGBA")

# 3. Then use the mask itself to fill that alpha channel
mask_rgba.putalpha(mask)

# 4. Convert the mask into bytes
buf = BytesIO()
mask_rgba.save(buf, format="PNG")
mask_bytes = buf.getvalue()

# 5. Save the resulting file
img_path_mask_alpha = "mask_alpha.png"
with open(img_path_mask_alpha, "wb") as f:
    f.write(mask_bytes)
```

### Image input fidelity

The `input_fidelity` parameter controls how strongly a model preserves details from input images during edits and reference-image workflows. For `gpt-image-2`, omit this parameter; the API doesn't allow changing it because the model processes every image input at high fidelity automatically.

Because `gpt-image-2` always processes image inputs at high fidelity, image
input tokens can be higher for edit requests that include reference images. To
understand the cost implications, refer to the [vision
costs](https://developers.openai.com/api/docs/guides/images-vision?api-mode=responses#calculating-costs)
section.
