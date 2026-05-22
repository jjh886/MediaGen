import { useState, useEffect } from 'react'
import { KtModal, KtButton, KtInput } from '@/shared/ui'
import { batchRegister } from '../services/account-service'
import { getAllSettings, type AppSettings } from '@/shared/services/settings-service'

interface BatchRegisterDialogProps {
  open: boolean
  onClose: () => void
  platform: string
  onSuccess: () => void
}

function generatePassword(): string {
  const chars = 'ABCDEFGHJKMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789'
  let result = ''
  for (let i = 0; i < 12; i++) {
    result += chars[Math.floor(Math.random() * chars.length)]
  }
  return result
}

function generateBirthday(): string {
  const start = new Date(1985, 0, 1).getTime()
  const end = new Date(2000, 11, 31).getTime()
  const date = new Date(start + Math.random() * (end - start))
  return date.toISOString().slice(0, 10)
}

const fieldStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 6,
}

const labelStyle: React.CSSProperties = {
  color: 'var(--text-secondary)',
  fontSize: 13,
}

export default function BatchRegisterDialog({
  open,
  onClose,
  platform,
  onSuccess,
}: BatchRegisterDialogProps) {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [count, setCount] = useState('5')
  const [password, setPassword] = useState('')
  const [birthday, setBirthday] = useState('')
  const [running, setRunning] = useState(false)
  const [progress, setProgress] = useState(0)

  const pwdMode = settings?.auto_register_password_mode ?? 'random'
  const bdMode = settings?.auto_register_birthday_mode ?? 'random'

  useEffect(() => {
    if (!settings) {
      getAllSettings().then(setSettings)
    }
  }, [settings])

  useEffect(() => {
    if (!settings) return
    setPassword(pwdMode === 'random' ? generatePassword() : settings.auto_register_password)
    setBirthday(bdMode === 'random' ? generateBirthday() : settings.auto_register_birthday)
  }, [settings, pwdMode, bdMode])

  const handleStart = async () => {
    const n = Math.min(50, Math.max(1, Number(count) || 1))
    const finalPassword = pwdMode === 'random' ? generatePassword() : password
    const finalBirthday = bdMode === 'random' ? generateBirthday() : birthday
    setRunning(true)
    setProgress(0)
    try {
      await batchRegister(platform, n, finalPassword, finalBirthday)
      setProgress(100)
      onSuccess()
    } finally {
      setRunning(false)
    }
  }

  return (
    <KtModal
      open={open}
      onClose={onClose}
      title="批量注册"
      footer={
        running ? (
          <KtButton variant="default" onClick={onClose}>
            关闭
          </KtButton>
        ) : (
          <>
            <KtButton variant="default" onClick={onClose}>
              取消
            </KtButton>
            <KtButton variant="primary" onClick={handleStart}>
              开始注册
            </KtButton>
          </>
        )
      }
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
        <div style={fieldStyle}>
          <label style={labelStyle}>注册数量（1-50）</label>
          <KtInput
            type="number"
            min={1}
            max={50}
            value={count}
            onChange={(e) => setCount(e.target.value)}
            disabled={running}
          />
        </div>

        <div style={fieldStyle}>
          <label style={labelStyle}>
            密码{pwdMode === 'random' ? '（每次随机生成）' : '（固定密码）'}
          </label>
          {pwdMode === 'default' ? (
            <KtInput
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              disabled={running}
            />
          ) : (
            <div style={{ fontSize: 12, color: 'var(--text-disabled)' }}>
              注册时自动生成随机密码
            </div>
          )}
        </div>

        <div style={fieldStyle}>
          <label style={labelStyle}>
            生日{bdMode === 'random' ? '（每次随机生成）' : '（固定生日）'}
          </label>
          {bdMode === 'default' ? (
            <KtInput
              type="date"
              value={birthday}
              onChange={(e) => setBirthday(e.target.value)}
              disabled={running}
            />
          ) : (
            <div style={{ fontSize: 12, color: 'var(--text-disabled)' }}>
              注册时自动生成随机生日（1985-2000年）
            </div>
          )}
        </div>

        {running && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            <div style={{ color: 'var(--text-secondary)', fontSize: 13 }}>
              注册进度: {progress}%
            </div>
            <div
              style={{
                height: 6,
                background: 'rgba(255,255,255,0.05)',
                borderRadius: 3,
                overflow: 'hidden',
              }}
            >
              <div
                style={{
                  width: `${progress}%`,
                  height: '100%',
                  background: 'var(--color-brand)',
                  borderRadius: 3,
                  transition: 'width 300ms ease',
                }}
              />
            </div>
          </div>
        )}
      </div>
    </KtModal>
  )
}
