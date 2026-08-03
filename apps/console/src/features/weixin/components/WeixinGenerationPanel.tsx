import { ImagePlus } from 'lucide-react'
import { KtInput, KtSelect } from '@/shared/ui'
import type { WeixinSettingsUpdate, WeixinSettingsView } from '../types/weixin'
import { ToggleRow, WeixinField, WeixinPanel } from './WeixinShared'

const RATIO_OPTIONS = [
  { value: 'auto', label: '自动' },
  { value: '1:1', label: '1:1 方图' },
  { value: '9:16', label: '9:16 竖图' },
  { value: '16:9', label: '16:9 横图' },
  { value: '2:1', label: '2:1 宽图' },
]

interface WeixinGenerationPanelProps {
  settings: WeixinSettingsView
  draft: WeixinSettingsUpdate
  onChange: (patch: Partial<WeixinSettingsUpdate>) => void
}

export function WeixinGenerationPanel({ settings, draft, onChange }: WeixinGenerationPanelProps) {
  const modelOptions = settings.models.map((model) => ({
    value: model.modelId,
    label: `${model.displayName} · ${model.platform}`,
  }))

  return (
    <WeixinPanel title="微信生图">
      <GenerationIntro />
      <GenerationToggles draft={draft} onChange={onChange} />
      <GenerationDefaults draft={draft} modelOptions={modelOptions} onChange={onChange} />
      {modelOptions.length === 0 && <MissingModelNotice />}
    </WeixinPanel>
  )
}

function GenerationIntro() {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 10, color: 'var(--text-secondary)' }}>
      <ImagePlus size={18} />
      <span style={{ fontSize: 13 }}>微信里发送“生成 提示词”后，会进入后台异步生成队列。</span>
    </div>
  )
}

function GenerationToggles({
  draft,
  onChange,
}: Pick<WeixinGenerationPanelProps, 'draft' | 'onChange'>) {
  return (
    <>
      <ToggleRow
        title="允许微信创建图片"
        description="关闭后微信只能查询状态和接收通知，不会创建新图片任务。"
        checked={draft.generationEnabled}
        onChange={(generationEnabled) => onChange({ generationEnabled })}
      />
      <ToggleRow
        title="直接把消息当作提示词"
        description="开启后不需要写“生成”，普通文字也会排队生成图片。"
        checked={draft.directPromptEnabled}
        onChange={(directPromptEnabled) => onChange({ directPromptEnabled })}
      />
    </>
  )
}

function GenerationDefaults({
  draft,
  modelOptions,
  onChange,
}: Pick<WeixinGenerationPanelProps, 'draft' | 'onChange'> & {
  modelOptions: { value: string; label: string }[]
}) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '1fr 150px 120px', gap: 12 }}>
      <WeixinField label="默认模型">
        <KtSelect
          value={draft.defaultModelId}
          placeholder="选择图片模型"
          options={modelOptions}
          disabled={modelOptions.length === 0}
          onChange={(defaultModelId) => onChange({ defaultModelId })}
        />
      </WeixinField>
      <WeixinField label="默认比例">
        <KtSelect
          value={draft.defaultAspectRatio}
          options={RATIO_OPTIONS}
          onChange={(defaultAspectRatio) => onChange({ defaultAspectRatio })}
        />
      </WeixinField>
      <WeixinField label="默认张数">
        <KtInput
          type="number"
          min={1}
          max={4}
          value={draft.defaultMaxImages}
          onChange={(event) => onChange({ defaultMaxImages: Number(event.target.value || 1) })}
        />
      </WeixinField>
    </div>
  )
}

function MissingModelNotice() {
  return (
    <div style={{ color: 'var(--color-warning)', fontSize: 13 }}>
      当前没有可用图片模型，请先确认插件 manifest 是否提供图片模型。
    </div>
  )
}
