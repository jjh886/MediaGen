import { useEffect, useMemo, useState, type FormEvent } from 'react'
import type { ProxyItem } from '@/shared/services/proxy-service'
import { KtButton, KtInput, KtModal, KtSelect } from '@/shared/ui'
import { parseProxyUrlParts } from '@/shared/utils/proxy-url'
import {
  CreateDialogFrame,
  FormField,
  gridStyle,
  hasFormErrors,
  initialApiKeyForm,
  initialProxyForm,
  proxyProtocolOptions,
  submitValidated,
  toApiKeyCreate,
  toProxyCreate,
  useCreateForm,
  validateApiKeyForm,
  validateProxyForm,
  type ApiKeyCreateResult,
  type ApiKeyCreateValues,
  type ApiKeyFormState,
  type DialogProps,
  type FormErrors,
  type ProxyCreateValues,
  type ProxyFormState,
} from './AdminCreateDialogShared'

export type { ApiKeyCreateResult, ApiKeyCreateValues, ProxyCreateValues }

interface ApiKeyCreateDialogProps {
  open: boolean
  onClose: () => void
  onSubmit: (values: ApiKeyCreateValues) => Promise<ApiKeyCreateResult>
}

function ApiKeyFields({
  values,
  errors,
  onChange,
}: {
  values: ApiKeyFormState
  errors: FormErrors<ApiKeyFormState>
  onChange: (field: keyof ApiKeyFormState, value: string) => void
}) {
  return (
    <FormField label="名称" error={errors.name}>
      <KtInput
        aria-label="名称"
        value={values.name}
        onChange={(event) => onChange('name', event.target.value)}
        error={Boolean(errors.name)}
      />
    </FormField>
  )
}

function copyGeneratedKey(value: string) {
  void navigator.clipboard?.writeText(value).catch(() => undefined)
}

function ApiKeyCreatedDialog({
  open,
  onClose,
  apiKey,
}: {
  open: boolean
  onClose: () => void
  apiKey: string
}) {
  return (
    <KtModal
      open={open}
      onClose={onClose}
      title="API Key 已生成"
      closeOnOverlayClick={false}
      footer={
        <>
          <KtButton variant="default" onClick={() => copyGeneratedKey(apiKey)}>
            复制
          </KtButton>
          <KtButton variant="primary" onClick={onClose}>
            关闭
          </KtButton>
        </>
      }
    >
      <div style={{ display: 'grid', gap: 12 }}>
        <div style={{ color: 'var(--text-secondary)', fontSize: 13 }}>
          这个 Key 只显示一次，关闭前请先保存。
        </div>
        <FormField label="生成的 API Key">
          <KtInput
            aria-label="生成的 API Key"
            readOnly
            value={apiKey}
            onFocus={(event) => event.currentTarget.select()}
          />
        </FormField>
      </div>
    </KtModal>
  )
}

export function ApiKeyCreateDialog({ open, onClose, onSubmit }: ApiKeyCreateDialogProps) {
  const formId = 'api-key-create-form'
  const { values, errors, setErrors, update } = useCreateForm(open, initialApiKeyForm)
  const [saving, setSaving] = useState(false)
  const [createdKey, setCreatedKey] = useState('')
  const [submitError, setSubmitError] = useState('')

  useEffect(() => {
    if (open) {
      setCreatedKey('')
      setSubmitError('')
    }
  }, [open])

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    const nextErrors = validateApiKeyForm(values)
    setErrors(nextErrors)
    if (hasFormErrors(nextErrors)) return
    setSaving(true)
    setSubmitError('')
    try {
      const result = await onSubmit(toApiKeyCreate(values))
      setCreatedKey(result.key)
    } catch (error) {
      setSubmitError(error instanceof Error ? error.message : '创建 API Key 失败。')
    } finally {
      setSaving(false)
    }
  }

  if (createdKey) {
    return <ApiKeyCreatedDialog open={open} onClose={onClose} apiKey={createdKey} />
  }

  return (
    <CreateDialogFrame
      open={open}
      onClose={onClose}
      title="新增 API Key"
      formId={formId}
      saving={saving}
      submitError={submitError}
      onSubmit={submit}
    >
      <ApiKeyFields values={values} errors={errors} onChange={update} />
    </CreateDialogFrame>
  )
}

function ProxyEndpointFields({
  values,
  errors,
  onChange,
  onPatch,
}: {
  values: ProxyFormState
  errors: FormErrors<ProxyFormState>
  onChange: (field: keyof ProxyFormState, value: string) => void
  onPatch: (values: Partial<ProxyFormState>) => void
}) {
  const updateHost = (raw: string) => {
    const parsed = parseProxyUrlParts(raw)
    if (parsed && (raw.includes('://') || raw.includes('@'))) onPatch(parsed)
    else onChange('host', raw)
  }

  return (
    <>
      <div style={gridStyle}>
        <FormField label="协议">
          <KtSelect
            value={values.protocol}
            onChange={(value) => onChange('protocol', value)}
            options={proxyProtocolOptions}
          />
        </FormField>
        <FormField label="端口" error={errors.port}>
          <KtInput
            aria-label="端口"
            type="number"
            min={1}
            max={65535}
            value={values.port}
            onChange={(event) => onChange('port', event.target.value)}
            error={Boolean(errors.port)}
          />
        </FormField>
      </div>
      <FormField label="代理地址" error={errors.host}>
        <KtInput
          aria-label="代理地址"
          placeholder="主机、完整代理 URL 或 username:password@ip:port"
          value={values.host}
          onChange={(event) => updateHost(event.target.value)}
          error={Boolean(errors.host)}
        />
      </FormField>
    </>
  )
}

function ProxyAuthFields({
  values,
  errors,
  onChange,
}: {
  values: ProxyFormState
  errors: FormErrors<ProxyFormState>
  onChange: (field: keyof ProxyFormState, value: string) => void
}) {
  return (
    <div style={gridStyle}>
      <FormField label="账号" error={errors.username}>
        <KtInput
          aria-label="账号"
          value={values.username}
          onChange={(event) => onChange('username', event.target.value)}
          error={Boolean(errors.username)}
        />
      </FormField>
      <FormField label="密码">
        <KtInput
          aria-label="密码"
          type="password"
          value={values.password}
          onChange={(event) => onChange('password', event.target.value)}
        />
      </FormField>
    </div>
  )
}

function ProxyFields({
  values,
  errors,
  onChange,
  onPatch,
}: {
  values: ProxyFormState
  errors: FormErrors<ProxyFormState>
  onChange: (field: keyof ProxyFormState, value: string) => void
  onPatch: (values: Partial<ProxyFormState>) => void
}) {
  return (
    <>
      <FormField label="代理名称" error={errors.name}>
        <KtInput
          aria-label="代理名称"
          value={values.name}
          onChange={(event) => onChange('name', event.target.value)}
          error={Boolean(errors.name)}
        />
      </FormField>
      <ProxyEndpointFields values={values} errors={errors} onChange={onChange} onPatch={onPatch} />
      <ProxyAuthFields values={values} errors={errors} onChange={onChange} />
    </>
  )
}

export function ProxyCreateDialog({
  open,
  onClose,
  onSubmit,
  initial,
}: DialogProps<ProxyCreateValues> & { initial?: ProxyItem | null }) {
  const formId = 'proxy-create-form'
  const defaults = useMemo(
    () =>
      initial
        ? {
            ...initialProxyForm,
            ...parseProxyUrlParts(initial.server),
            name: initial.name,
            username: initial.username,
            password: '',
          }
        : initialProxyForm,
    [initial]
  )
  const { values, errors, setErrors, update, patch } = useCreateForm(open, defaults)
  const [saving, setSaving] = useState(false)
  const [submitError, setSubmitError] = useState('')

  const submit = (event: FormEvent) =>
    void submitValidated(event, {
      values,
      validate: validateProxyForm,
      toResult: toProxyCreate,
      setErrors,
      setSaving,
      setSubmitError,
      onSubmit,
      onClose,
    })

  return (
    <CreateDialogFrame
      open={open}
      onClose={onClose}
      title={initial ? '编辑代理' : '新增代理'}
      formId={formId}
      saving={saving}
      submitError={submitError}
      onSubmit={submit}
    >
      <ProxyFields values={values} errors={errors} onChange={update} onPatch={patch} />
      {initial && (
        <p style={{ color: 'var(--text-secondary)', fontSize: 12 }}>
          密码留空保留原密码。保存后，已绑定账号的后续请求使用新配置。
        </p>
      )}
    </CreateDialogFrame>
  )
}
