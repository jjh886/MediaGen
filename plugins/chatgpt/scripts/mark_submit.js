;(() => {
  const SK = Symbol.for('_cgpt')
  const state = (window[SK] = window[SK] || {})
  state.status = 'generating'
  state.submit_started_at = Date.now()
  state.download_url = ''
  state.generated_file_id = ''
  state.result_conversation_id = ''
  state.result_at = 0
  state.conversation_id = ''
  state.image_gen_task_id = ''
  state.async_status = null
  state.stream_status = ''
  state.message = ''
  state.pending_result = null
  return JSON.stringify({ marked: true, submit_started_at: state.submit_started_at })
})()
