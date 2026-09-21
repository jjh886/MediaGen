function main(ctx) {
  const auth = grokAuth(ctx)
  const session = {
    move: {
      '/session/voice': '/session/audio/output/voice',
      '/session/turn_detection': '/session/audio/input/turn_detection',
    },
    set: { '/session/type': 'realtime', '/session/model': ctx.request.model },
  }
  return {
    connection: {
      url: 'wss://api.x.ai/v1/realtime?model=' + encodeURIComponent(ctx.request.model),
      headers: { authorization: 'Bearer ' + auth.access_token },
      fingerprint: 'none',
    },
    client_event_adapter: {
      events: {
        'session.update': {
          move: {
            '/session/audio/output/voice': '/session/voice',
            '/session/audio/input/turn_detection': '/session/turn_detection',
          },
          remove: ['/session/type', '/session/model'],
        },
      },
    },
    server_event_adapter: {
      events: { 'session.created': session, 'session.updated': session },
      type_aliases: {
        'response.audio.delta': 'response.output_audio.delta',
        'response.audio.done': 'response.output_audio.done',
        'response.audio_transcript.delta': 'response.output_audio_transcript.delta',
        'response.audio_transcript.done': 'response.output_audio_transcript.done',
        'response.text.delta': 'response.output_text.delta',
        'response.text.done': 'response.output_text.done',
      },
    },
  }
}
