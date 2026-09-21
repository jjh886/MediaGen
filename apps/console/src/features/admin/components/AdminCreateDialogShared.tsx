import { useEffect, useState, type FormEvent, type ReactNode } from 'react'
import { KtButton, KtModal } from '@/shared/ui'
import { buildProxyServerUrl } from '@/shared/utils/proxy-url'
export { KtFormField as FormField } from '@/shared/ui'

export interface ApiKeyCreateValues {
  name: string
}

export interface ApiKeyCreateResult {
  id: string
  key: string
  masked_key: string
}

export interface ProxyCreateValues {
  name: string
  server: string
  username: string
  password: string
}

export interface DialogProps<T> {
  open: boolean
  onClose: () => void
  onSubmit: (values: T) => Promise<void>
}

export type ApiKeyFormState = {
  name: string
}

export type ProxyFormState = {
  name: string
  protocol: 'socks5h' | 'socks5' | 'http' | 'https'
  host: string
  port: string
  username: string
  password: string
}

export type FormErrors<T> = Partial<Record<keyof T, string>>

export const gridStyle: React.CSSProperties = {
  display: 'grid',
  gridTemplateColumns: '1fr 1fr',
  gap: 12,
}

export const proxyProtocolOptions = [
  { value: 'http', label: 'HTTP' },
  { value: 'https', label: 'HTTPS' },
  { value: 'socks5h', label: 'SOCKS5H' },
  { value: 'socks5', label: 'SOCKS5' },
]

export const initialApiKeyForm: ApiKeyFormState = { name: '' }
export const initialProxyForm: ProxyFormState = {
  name: '',
  protocol: 'socks5h',
  host: '',
  port: '',
  username: '',
  password: '',
}

function required(value: string, message: string) {
  return value.trim() ? undefined : message
}

export function hasFormErrors<T extends object>(errors: FormErrors<T>) {
  return Object.values(errors).some(Boolean)
}

export function validateApiKeyForm(values: ApiKeyFormState): FormErrors<ApiKeyFormState> {
  return {
    name: required(values.name, '请填写名称'),
  }
}

function validateProxyPort(port: string) {
  const value = Number(port)
  if (!Number.isInteger(value) || value < 1 || value > 65535) {
    return '端口必须是 1 到 65535 之间的整数'
  }
  return undefined
}

export function validateProxyForm(values: ProxyFormState): FormErrors<ProxyFormState> {
  return {
    name: required(values.name, '请填写代理名称'),
    host: required(values.host, '请填写代理地址'),
    port: validateProxyPort(values.port),
    username: values.password.trim() && !values.username.trim() ? '填写密码时也要填写账号' : '',
  }
}

export function toApiKeyCreate(values: ApiKeyFormState): ApiKeyCreateValues {
  return {
    name: values.name.trim(),
  }
}

export function toProxyCreate(values: ProxyFormState): ProxyCreateValues {
  return {
    name: values.name.trim(),
    server: buildProxyServerUrl({ ...values, username: '', password: '' }),
    username: values.username.trim(),
    password: values.password,
  }
}

export function useCreateForm<T extends Record<string, string>>(open: boolean, initialValues: T) {
  const [values, setValues] = useState<T>(initialValues)
  const [errors, setErrors] = useState<FormErrors<T>>({})

  useEffect(() => {
    if (open) {
      setValues(initialValues)
      setErrors({})
    }
  }, [initialValues, open])

  const update = (field: keyof T, value: string) => {
    setValues((old) => ({ ...old, [field]: value }))
    setErrors((old) => ({ ...old, [field]: undefined }))
  }

  const patch = (nextValues: Partial<T>) => {
    setValues((old) => ({ ...old, ...nextValues }))
    setErrors((old) => {
      const nextErrors = { ...old }
      for (const key of Object.keys(nextValues) as (keyof T)[]) {
        nextErrors[key] = undefined
      }
      return nextErrors
    })
  }

  return { values, errors, setErrors, update, patch }
}

export async function submitValidated<TState extends object, TResult>(
  event: FormEvent,
  options: {
    values: TState
    validate: (values: TState) => FormErrors<TState>
    toResult: (values: TState) => TResult
    setErrors: (errors: FormErrors<TState>) => void
    setSaving: (saving: boolean) => void
    onSubmit: (values: TResult) => Promise<void>
    onClose: () => void
    setSubmitError?: (error: string) => void
  }
) {
  event.preventDefault()
  options.setSubmitError?.('')
  const nextErrors = options.validate(options.values)
  options.setErrors(nextErrors)
  if (hasFormErrors(nextErrors)) return
  options.setSaving(true)
  try {
    await options.onSubmit(options.toResult(options.values))
    options.onClose()
  } catch (error) {
    options.setSubmitError?.(errorMessage(error))
  } finally {
    options.setSaving(false)
  }
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : '提交失败，请检查后重试。'
}

function DialogFooter({
  formId,
  saving,
  onClose,
  label,
}: {
  formId: string
  saving: boolean
  onClose: () => void
  label: string
}) {
  return (
    <>
      <KtButton variant="default" onClick={onClose} disabled={saving}>
        取消
      </KtButton>
      <KtButton variant="primary" type="submit" form={formId} loading={saving}>
        {label}
      </KtButton>
    </>
  )
}

export function CreateDialogFrame({
  open,
  onClose,
  title,
  formId,
  saving,
  submitError,
  onSubmit,
  children,
}: {
  open: boolean
  onClose: () => void
  title: string
  formId: string
  saving: boolean
  submitError?: string
  onSubmit: (event: FormEvent) => void
  children: ReactNode
}) {
  return (
    <KtModal
      open={open}
      onClose={onClose}
      title={title}
      closeOnOverlayClick={false}
      footer={
        <DialogFooter
          formId={formId}
          saving={saving}
          onClose={onClose}
          label={title.startsWith('编辑') ? '保存' : '新增'}
        />
      }
    >
      <form id={formId} noValidate onSubmit={onSubmit} style={{ display: 'grid', gap: 14 }}>
        {submitError && (
          <div style={{ color: 'var(--color-error)', fontSize: 13 }}>{submitError}</div>
        )}
        {children}
      </form>
    </KtModal>
  )
}
