import { useEffect, useState } from 'react'
import { CheckCircle2, ScanLine } from 'lucide-react'
import { KtButton, KtInput, KtModal } from '@/shared/ui'
import type { BindingActionResult } from '../types/weixin'

interface WeixinBindingDialogProps {
  open: boolean
  result: BindingActionResult | null
  acting: boolean
  onPoll: (sessionId: string) => Promise<void>
  onSubmitCode: (sessionId: string, code: string) => Promise<void>
  onCancel: (sessionId?: string) => Promise<void>
  onClose: () => void
}

const POLL_INTERVAL_MS = 2200

export function WeixinBindingDialog({
  open,
  result,
  acting,
  onPoll,
  onSubmitCode,
  onCancel,
  onClose,
}: WeixinBindingDialogProps) {
  const [verifyCode, setVerifyCode] = useState('')
  const sessionId = result?.sessionId
  const shouldPoll = shouldPollResult(open, result)

  useEffect(() => {
    if (!shouldPoll || !sessionId) return
    const timer = window.setInterval(() => void onPoll(sessionId), POLL_INTERVAL_MS)
    return () => window.clearInterval(timer)
  }, [onPoll, sessionId, shouldPoll])

  useEffect(() => {
    if (!open) setVerifyCode('')
  }, [open])

  return (
    <KtModal
      open={open}
      title="绑定微信"
      width={460}
      closeOnOverlayClick={!acting}
      onClose={onClose}
      footer={
        <BindingDialogFooter
          acting={acting}
          result={result}
          verifyCode={verifyCode}
          onCancel={onCancel}
          onSubmitCode={onSubmitCode}
        />
      }
    >
      <BindingDialogBody result={result} verifyCode={verifyCode} setVerifyCode={setVerifyCode} />
    </KtModal>
  )
}

function BindingDialogFooter({
  acting,
  result,
  verifyCode,
  onCancel,
  onSubmitCode,
}: Pick<WeixinBindingDialogProps, 'acting' | 'result' | 'onCancel' | 'onSubmitCode'> & {
  verifyCode: string
}) {
  const sessionId = result?.sessionId
  return (
    <>
      <KtButton disabled={acting} onClick={() => void onCancel(sessionId)}>
        取消
      </KtButton>
      {result?.status === 'need_verify_code' && sessionId && (
        <KtButton
          variant="primary"
          loading={acting}
          onClick={() => void onSubmitCode(sessionId, verifyCode)}
        >
          提交验证码
        </KtButton>
      )}
    </>
  )
}

function BindingDialogBody({
  result,
  verifyCode,
  setVerifyCode,
}: {
  result: BindingActionResult | null
  verifyCode: string
  setVerifyCode: (value: string) => void
}) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <div style={{ color: 'var(--text-secondary)', fontSize: 13 }}>
        {result?.message || '正在准备微信二维码...'}
      </div>
      {result?.qrImage && <QrPreview qrImage={result.qrImage} />}
      {result?.status === 'need_verify_code' && (
        <VerifyCodeField value={verifyCode} onChange={setVerifyCode} />
      )}
      {result?.activationCode && <ActivationCodeBlock code={result.activationCode} />}
      {!result?.qrImage && !result?.activationCode && <WaitingState />}
    </div>
  )
}

function QrPreview({ qrImage }: { qrImage: string }) {
  return (
    <div
      style={{
        display: 'grid',
        placeItems: 'center',
        padding: 16,
        borderRadius: 8,
        background: 'rgba(255,255,255,0.04)',
      }}
    >
      <img src={qrImage} alt="微信绑定二维码" style={{ width: 240, height: 240 }} />
    </div>
  )
}

function VerifyCodeField({
  value,
  onChange,
}: {
  value: string
  onChange: (value: string) => void
}) {
  return (
    <label style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <span style={{ color: 'var(--text-secondary)', fontSize: 13 }}>手机微信验证码</span>
      <KtInput
        inputMode="numeric"
        maxLength={12}
        value={value}
        onChange={(event) => onChange(event.target.value.replace(/\D/g, ''))}
      />
    </label>
  )
}

function ActivationCodeBlock({ code }: { code: string }) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
        padding: 14,
        borderRadius: 8,
        color: 'var(--color-success)',
        background: 'var(--color-success-bg)',
      }}
    >
      <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
        <CheckCircle2 size={16} />
        扫码已确认
      </span>
      <strong style={{ color: 'var(--text-bright)', fontSize: 24, letterSpacing: 0 }}>
        {code}
      </strong>
      <span style={{ color: 'var(--text-secondary)', fontSize: 13 }}>
        请把这 6 位数字发送到刚刚扫码的微信会话里，服务收到后会完成激活。
      </span>
    </div>
  )
}

function WaitingState() {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'var(--text-tertiary)' }}>
      <ScanLine size={16} />
      <span>等待服务返回扫码状态。</span>
    </div>
  )
}

function shouldPollResult(open: boolean, result: BindingActionResult | null): boolean {
  return Boolean(
    open && result?.sessionId && ['wait', 'scanned', 'verifying'].includes(result.status)
  )
}
