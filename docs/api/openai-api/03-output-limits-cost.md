## Customize Image Output

You can configure the following output options:

- **Size**: Image dimensions (for example, `1024x1024`, `1024x1536`)
- **Quality**: Rendering quality (for example, `low`, `medium`, `high`)
- **Format**: File output format
- **Compression**: Compression level (0-100%) for JPEG and WebP formats
- **Background**: Opaque or automatic

`size`, `quality`, and `background` support the `auto` option, where the model will automatically select the best option based on the prompt.

`gpt-image-2` doesn't currently support transparent backgrounds. Requests with
`background: "transparent"` aren't supported for this model.

### Size and quality options

`gpt-image-2` accepts any resolution in the `size` parameter when it satisfies the constraints below. Square images are typically fastest to generate.

<table>
  <tbody>
    <tr>
      <td>Popular sizes</td>
      <td>
        <ul>
          <li>
            <code>1024x1024</code> (square)
          </li>
          <li>
            <code>1536x1024</code> (landscape)
          </li>
          <li>
            <code>1024x1536</code> (portrait)
          </li>
          <li>
            <code>2048x2048</code> (2K square)
          </li>
          <li>
            <code>2048x1152</code> (2K landscape)
          </li>
          <li>
            <code>3840x2160</code> (4K landscape)
          </li>
          <li>
            <code>2160x3840</code> (4K portrait)
          </li>
          <li>
            <code>auto</code> (default)
          </li>
        </ul>
      </td>
    </tr>
    <tr>
      <td>Size constraints</td>
      <td>
        <ul>
          <li>
            Maximum edge length must be less than or equal to 
            <code>3840px</code>
          </li>
          <li>
            Both edges must be multiples of <code>16px</code>
          </li>
          <li>
            Long edge to short edge ratio must not exceed <code>3:1</code>
          </li>
          <li>
            Total pixels must be at least <code>655,360</code> and no more than 
            <code>8,294,400</code>
          </li>
        </ul>
      </td>
    </tr>
    <tr>
      <td>Quality options</td>
      <td>
        <ul>
          <li>
            <code>low</code>
          </li>
          <li>
            <code>medium</code>
          </li>
          <li>
            <code>high</code>
          </li>
          <li>
            <code>auto</code> (default)
          </li>
        </ul>
      </td>
    </tr>
  </tbody>
</table>

Use `quality: "low"` for fast drafts, thumbnails, and quick iterations. It is
the fastest option and works well for many common use cases before you move to
`medium` or `high` for final assets.

Outputs that contain more than `2560x1440` (`3,686,400`) total pixels,
typically referred to as 2K, are considered experimental.

### Output format

The Image API returns base64-encoded image data.
The default format is `png`, but you can also request `jpeg` or `webp`.

If using `jpeg` or `webp`, you can also specify the `output_compression` parameter to control the compression level (0-100%). For example, `output_compression=50` will compress the image by 50%.

Using `jpeg` is faster than `png`, so you should prioritize this format if
latency is a concern.

## Limitations

GPT Image models (`gpt-image-2`, `gpt-image-1.5`, `gpt-image-1`, and `gpt-image-1-mini`) are powerful and versatile image generation models, but they still have some limitations to be aware of:

- **Latency:** Complex prompts may take up to 2 minutes to process.
- **Text Rendering:** Although significantly improved, the model can still struggle with precise text placement and clarity.
- **Consistency:** While capable of producing consistent imagery, the model may occasionally struggle to maintain visual consistency for recurring characters or brand elements across multiple generations.
- **Composition Control:** Despite improved instruction following, the model may have difficulty placing elements precisely in structured or layout-sensitive compositions.

### Content Moderation

All prompts and generated images are filtered in accordance with our [content policy](https://openai.com/policies/usage-policies/).

For image generation using GPT Image models (`gpt-image-2`, `gpt-image-1.5`, `gpt-image-1`, and `gpt-image-1-mini`), you can control moderation strictness with the `moderation` parameter. This parameter supports two values:

- `auto` (default): Standard filtering that seeks to limit creating certain categories of potentially age-inappropriate content.
- `low`: Less restrictive filtering.

### Supported models

When using image generation in the Responses API, `gpt-5` and newer models should support the image generation tool. [Check the model detail page for your model](https://developers.openai.com/api/docs/models) to confirm if your desired model can use the image generation tool.

## Cost and latency

### `gpt-image-2` output tokens

For `gpt-image-2`, use the calculator to estimate output tokens from the requested `quality` and `size`:

### Models prior to `gpt-image-2`

GPT Image models prior to `gpt-image-2` generate images by first producing specialized image tokens. Both latency and eventual cost are proportional to the number of tokens required to render an image—larger image sizes and higher quality settings result in more tokens.

The number of tokens generated depends on image dimensions and quality:

| Quality | Square (1024×1024) | Portrait (1024×1536) | Landscape (1536×1024) |
| ------- | ------------------ | -------------------- | --------------------- |
| Low     | 272 tokens         | 408 tokens           | 400 tokens            |
| Medium  | 1056 tokens        | 1584 tokens          | 1568 tokens           |
| High    | 4160 tokens        | 6240 tokens          | 6208 tokens           |

Note that you will also need to account for [input tokens](https://developers.openai.com/api/docs/guides/images-vision?api-mode=responses#calculating-costs): text tokens for the prompt and image tokens for the input images if editing images.
Because `gpt-image-2` always processes image inputs at high fidelity, edit requests that include reference images can use more input tokens.

Refer to the [pricing page](https://developers.openai.com/api/docs/pricing#image-generation) for current
text and image token prices, and use the [Calculating costs](#calculating-costs)
section below to estimate request costs.

The final cost is the sum of:

- input text tokens
- input image tokens if using the edits endpoint
- image output tokens

### Calculating costs

Use the pricing calculator below to estimate request costs for GPT Image models.
`gpt-image-2` supports thousands of valid resolutions; the table below lists the
same sizes used for previous GPT Image models for comparison. For GPT Image 1.5,
GPT Image 1, and GPT Image 1 Mini, the legacy per-image output pricing table is
also listed below. You should still account for text and image input tokens when
estimating the total cost of a request.

A larger non-square resolution can sometimes produce fewer output tokens than
a smaller or square resolution at the same quality setting.

<table
  style={{ borderCollapse: "collapse", tableLayout: "fixed", width: "100%" }}
>
  <thead>
    <tr>
      <th style={{ textAlign: "left", padding: "8px", width: "28%" }}>Model</th>
      <th style={{ textAlign: "left", padding: "8px", width: "14%" }}>
        Quality
      </th>
      <th style={{ padding: "8px", width: "19.33%" }}>1024 x 1024</th>
      <th style={{ padding: "8px", width: "19.33%" }}>1024 x 1536</th>
      <th style={{ padding: "8px", width: "19.34%" }}>1536 x 1024</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td rowSpan="3" style={{ padding: "8px", width: "28%" }}>
        GPT Image 2
        <br />
        <span style={{ fontSize: "0.875em" }}>Additional sizes available</span>
      </td>
      <td style={{ padding: "8px" }}>Low</td>
      <td style={{ padding: "8px" }}>$0.006</td>
      <td style={{ padding: "8px" }}>$0.005</td>
      <td style={{ padding: "8px" }}>$0.005</td>
    </tr>
    <tr>
      <td style={{ padding: "8px" }}>Medium</td>
      <td style={{ padding: "8px" }}>$0.053</td>
      <td style={{ padding: "8px" }}>$0.041</td>
      <td style={{ padding: "8px" }}>$0.041</td>
    </tr>
    <tr>
      <td style={{ padding: "8px" }}>High</td>
      <td style={{ padding: "8px" }}>$0.211</td>
      <td style={{ padding: "8px" }}>$0.165</td>
      <td style={{ padding: "8px" }}>$0.165</td>
    </tr>

    <tr>
      <td rowSpan="3" style={{ padding: "8px", width: "28%" }}>
        GPT Image 1.5
      </td>
      <td style={{ padding: "8px" }}>Low</td>
      <td style={{ padding: "8px" }}>$0.009</td>
      <td style={{ padding: "8px" }}>$0.013</td>
      <td style={{ padding: "8px" }}>$0.013</td>
    </tr>
    <tr>
      <td style={{ padding: "8px" }}>Medium</td>
      <td style={{ padding: "8px" }}>$0.034</td>
      <td style={{ padding: "8px" }}>$0.05</td>
      <td style={{ padding: "8px" }}>$0.05</td>
    </tr>
    <tr>
      <td style={{ padding: "8px" }}>High</td>
      <td style={{ padding: "8px" }}>$0.133</td>
      <td style={{ padding: "8px" }}>$0.2</td>
      <td style={{ padding: "8px" }}>$0.2</td>
    </tr>

    <tr>
      <td rowSpan="3" style={{ padding: "8px", width: "28%" }}>
        GPT Image 1
      </td>
      <td style={{ padding: "8px" }}>Low</td>
      <td style={{ padding: "8px" }}>$0.011</td>
      <td style={{ padding: "8px" }}>$0.016</td>
      <td style={{ padding: "8px" }}>$0.016</td>
    </tr>
    <tr>
      <td style={{ padding: "8px" }}>Medium</td>
      <td style={{ padding: "8px" }}>$0.042</td>
      <td style={{ padding: "8px" }}>$0.063</td>
      <td style={{ padding: "8px" }}>$0.063</td>
    </tr>
    <tr>
      <td style={{ padding: "8px" }}>High</td>
      <td style={{ padding: "8px" }}>$0.167</td>
      <td style={{ padding: "8px" }}>$0.25</td>
      <td style={{ padding: "8px" }}>$0.25</td>
    </tr>

    <tr>
      <td rowSpan="3" style={{ padding: "8px", width: "28%" }}>
        GPT Image 1 Mini
      </td>
      <td style={{ padding: "8px" }}>Low</td>
      <td style={{ padding: "8px" }}>$0.005</td>
      <td style={{ padding: "8px" }}>$0.006</td>
      <td style={{ padding: "8px" }}>$0.006</td>
    </tr>
    <tr>
      <td style={{ padding: "8px" }}>Medium</td>
      <td style={{ padding: "8px" }}>$0.011</td>
      <td style={{ padding: "8px" }}>$0.015</td>
      <td style={{ padding: "8px" }}>$0.015</td>
    </tr>
    <tr>
      <td style={{ padding: "8px" }}>High</td>
      <td style={{ padding: "8px" }}>$0.036</td>
      <td style={{ padding: "8px" }}>$0.052</td>
      <td style={{ padding: "8px" }}>$0.052</td>
    </tr>

  </tbody>
</table>

### Partial images cost

If you want to [stream image generation](#streaming) using the `partial_images` parameter, each partial image will incur an additional 100 image output tokens.
