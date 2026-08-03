import { useCallback, useState, type Dispatch, type SetStateAction } from 'react'
import {
  fetchWeixinSettings,
  runWeixinAction,
  saveWeixinSettings,
  startWeixinBinding,
} from '../services/weixin-service'
import type {
  BindingActionResult,
  WeixinActionRequest,
  WeixinSettingsUpdate,
  WeixinSettingsView,
} from '../types/weixin'

interface WeixinSettingsState {
  settings: WeixinSettingsView | null
  draft: WeixinSettingsUpdate | null
  loading: boolean
  saving: boolean
  acting: boolean
  error: string
  message: string
  setSettings: Dispatch<SetStateAction<WeixinSettingsView | null>>
  setDraft: Dispatch<SetStateAction<WeixinSettingsUpdate | null>>
  setLoading: Dispatch<SetStateAction<boolean>>
  setSaving: Dispatch<SetStateAction<boolean>>
  setActing: Dispatch<SetStateAction<boolean>>
  setError: Dispatch<SetStateAction<string>>
  setMessage: Dispatch<SetStateAction<string>>
}

function useWeixinState(): WeixinSettingsState {
  const [settings, setSettings] = useState<WeixinSettingsView | null>(null)
  const [draft, setDraft] = useState<WeixinSettingsUpdate | null>(null)
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [acting, setActing] = useState(false)
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  return {
    settings,
    draft,
    loading,
    saving,
    acting,
    error,
    message,
    setSettings,
    setDraft,
    setLoading,
    setSaving,
    setActing,
    setError,
    setMessage,
  }
}

export function useWeixinSettings() {
  const state = useWeixinState()
  const load = useLoadWeixinSettings(state)
  const updateDraft = useDraftUpdater(state)
  const save = useSaveOperation(state, load)
  const action = useActionOperation(state, load)
  const beginBinding = useBeginBindingOperation(state, load)
  return {
    settings: state.settings,
    draft: state.draft,
    loading: state.loading,
    saving: state.saving,
    acting: state.acting,
    error: state.error,
    message: state.message,
    load,
    updateDraft,
    save,
    action,
    beginBinding,
  }
}

function useLoadWeixinSettings(state: WeixinSettingsState) {
  const { setDraft, setError, setLoading, setSettings } = state
  return useCallback(async () => {
    setLoading(true)
    setError('')
    try {
      const next = await fetchWeixinSettings()
      setSettings(next)
      setDraft(draftFromSettings(next))
    } catch (error) {
      setError(errorMessage(error))
    } finally {
      setLoading(false)
    }
  }, [setDraft, setError, setLoading, setSettings])
}

function useDraftUpdater(state: WeixinSettingsState) {
  const { setDraft, setError, setMessage } = state
  return useCallback(
    (patch: Partial<WeixinSettingsUpdate>) => {
      setDraft((current) => (current ? { ...current, ...patch } : current))
      setMessage('')
      setError('')
    },
    [setDraft, setError, setMessage]
  )
}

function useSaveOperation(state: WeixinSettingsState, load: () => Promise<void>) {
  const { draft, setError, setMessage, setSaving } = state
  return useCallback(async () => {
    if (!draft) return
    setSaving(true)
    clearFeedback(setError, setMessage)
    try {
      const result = await saveWeixinSettings(draft)
      setMessage(result.message)
      await load()
    } catch (error) {
      setError(errorMessage(error))
    } finally {
      setSaving(false)
    }
  }, [draft, load, setError, setMessage, setSaving])
}

function useActionOperation(state: WeixinSettingsState, load: () => Promise<void>) {
  const { setActing, setError, setMessage } = state
  return useCallback(
    async (request: WeixinActionRequest): Promise<BindingActionResult | null> => {
      setActing(true)
      clearFeedback(setError, setMessage)
      try {
        const result = await runWeixinAction(request)
        setMessage(result.message)
        await load()
        return result
      } catch (error) {
        setError(errorMessage(error))
        return null
      } finally {
        setActing(false)
      }
    },
    [load, setActing, setError, setMessage]
  )
}

function useBeginBindingOperation(state: WeixinSettingsState, load: () => Promise<void>) {
  const { setActing, setError, setMessage } = state
  return useCallback(async (): Promise<BindingActionResult | null> => {
    setActing(true)
    clearFeedback(setError, setMessage)
    try {
      const result = await startWeixinBinding()
      setMessage(result.message)
      await load()
      return result
    } catch (error) {
      setError(errorMessage(error))
      return null
    } finally {
      setActing(false)
    }
  }, [load, setActing, setError, setMessage])
}

function clearFeedback(
  setError: Dispatch<SetStateAction<string>>,
  setMessage: Dispatch<SetStateAction<string>>
) {
  setError('')
  setMessage('')
}

function errorMessage(error: unknown): string {
  return error instanceof Error && error.message.trim() ? error.message : '微信操作失败。'
}

function draftFromSettings(settings: WeixinSettingsView): WeixinSettingsUpdate {
  return {
    generationEnabled: settings.generationEnabled,
    directPromptEnabled: settings.directPromptEnabled,
    defaultModelId: settings.defaultModelId,
    defaultAspectRatio: settings.defaultAspectRatio,
    defaultMaxImages: settings.defaultMaxImages,
    proactiveNotificationsEnabled: settings.proactiveNotificationsEnabled,
    notifyDesktopTasks: settings.notifyDesktopTasks,
    notifyTaskSuccesses: settings.notifyTaskSuccesses,
    notifyNoAccounts: settings.notifyNoAccounts,
    notifyAccountActions: settings.notifyAccountActions,
    quietHoursJson: settings.quietHoursJson,
  }
}
