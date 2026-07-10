import { useState, useEffect } from 'react'
import { KtModal, KtButton, KtInput, KtFormField } from '@/shared/ui'
import { batchRegister } from '../services/account-service'

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

export default function BatchRegisterDialog({
  open,
  onClose,
  platform,
  onSuccess,
}: BatchRegisterDialogProps) {
  const [count, setCount] = useState('5')
  const [password, setPassword] = useState('')
  const [birthday, setBirthday] = useState('')
  const [running, setRunning] = useState(false)
  const [progress, setProgress] = useState(0)

  useEffect(() => {
    if (!open) return
    // 注册参数只在弹窗打开时临时生成，避免依赖设置页里的旧配置。
    setPassword(generatePassword())
    setBirthday(generateBirthday())
  }, [open])

  const handleStart = async () => {
    const n = Math.min(50, Math.max(1, Number(count) || 1))
    setRunning(true)
    setProgress(0)
    try {
      await batchRegister(
        platform,
        n,
        password || generatePassword(),
        birthday || generateBirthday()
      )
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
        <KtFormField label="注册数量（1-50）">
          <KtInput
            type="number"
            min={1}
            max={50}
            value={count}
            onChange={(e) => setCount(e.target.value)}
            disabled={running}
          />
        </KtFormField>

        <KtFormField label="密码">
          <KtInput
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            disabled={running}
          />
        </KtFormField>

        <KtFormField label="生日">
          <KtInput
            type="date"
            value={birthday}
            onChange={(e) => setBirthday(e.target.value)}
            disabled={running}
          />
        </KtFormField>

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
