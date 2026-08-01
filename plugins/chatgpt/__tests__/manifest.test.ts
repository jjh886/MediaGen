import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

type FlowStep = {
  action: string
  script_file?: string
  script_files?: string[]
  selector?: string
  text?: string
  timeout_ms?: number
}

const here = path.dirname(fileURLToPath(import.meta.url))
const manifestPath = path.resolve(here, '../manifest.json')

function generateImageSteps(): FlowStep[] {
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
  return manifest.flows.generate_image.steps
}

describe('chatgpt generate_image manifest', () => {
  it('submits an explicit image request without opening image-mode UI', () => {
    const steps = generateImageSteps()
    const referencedScripts = steps.flatMap((step) => [
      ...(step.script_file ? [step.script_file] : []),
      ...(step.script_files || []),
    ])

    expect(referencedScripts).not.toContain('probe_image_mode.js')
    expect(referencedScripts).not.toContain('wait_create_image_entry.js')
    expect(referencedScripts).not.toContain('select_ratio.js')
    expect(referencedScripts).not.toContain('wait_ratio_menu_option.js')

    const typeStep = steps.find((step) => step.action === 'browser.type_text')
    expect(typeStep?.selector).toBe('[data-mediagen-cgpt-composer="true"]')
    expect(typeStep?.text).toContain('Generate an image')
    expect(typeStep?.text).toContain('{{image_aspect_ratio}}')
  })

  it('keeps one minute for reporting the ten-minute generation timeout', () => {
    const pollStep = generateImageSteps().find(
      (step) =>
        step.action === 'browser.poll_script' && step.script_file === 'generate_image_poll.js'
    )

    expect(pollStep?.timeout_ms).toBe(11 * 60 * 1000)
  })
})
