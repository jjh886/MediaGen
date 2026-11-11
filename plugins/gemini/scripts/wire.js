// 此接口仅使用 protobuf varint 和 length-delimited 字段，结构来自网页协议。
function geminiWireVarint(number) {
  if (!Number.isSafeInteger(number) || number < 0)
    throw new Error('invalid_input: protobuf 整数无效')
  const bytes = []
  do {
    const byte = number % 128
    number = Math.floor(number / 128)
    bytes.push(byte | (number ? 128 : 0))
  } while (number)
  return bytes
}

function geminiWireBytes(text) {
  const encoded = host.call('crypto.base64_encode_utf8', { text }).value
  return Array.from(host.call('crypto.base64_decode_latin1', { text: encoded }).value, (char) =>
    char.charCodeAt(0)
  )
}

function geminiWireEncode(fields) {
  const bytes = []
  for (const [number, value] of fields) {
    if (value === null || value === undefined) continue
    if (typeof value === 'number')
      bytes.push(...geminiWireVarint(number * 8), ...geminiWireVarint(value))
    else {
      const data = typeof value === 'string' ? geminiWireBytes(value) : value
      bytes.push(...geminiWireVarint(number * 8 + 2), ...geminiWireVarint(data.length))
      for (const byte of data) bytes.push(byte)
    }
  }
  return bytes
}

function geminiWireBase64(bytes) {
  let text = ''
  for (const byte of bytes) text += String.fromCharCode(byte)
  return host.call('crypto.base64_encode_latin1', { text }).value
}

function geminiWireDecode(bytes) {
  let position = 0
  const fields = {}
  const number = () => {
    let value = 0,
      scale = 1
    while (position < bytes.length && scale <= 2 ** 49) {
      const byte = bytes[position++]
      value += (byte & 127) * scale
      if (byte < 128) return value
      scale *= 128
    }
    throw new Error('invalid_response: protobuf 整数截断或溢出')
  }
  while (position < bytes.length) {
    const tag = number(),
      field = Math.floor(tag / 8),
      wire = tag % 8
    if (!field) throw new Error('invalid_response: protobuf 字段编号无效')
    let value
    if (wire === 0) value = number()
    else if (wire === 2) {
      const length = number()
      if (position + length > bytes.length) throw new Error('invalid_response: protobuf 字段截断')
      value = bytes.slice(position, position + length)
      position += length
    } else if (wire === 1 || wire === 5) {
      position += wire === 1 ? 8 : 4
      if (position > bytes.length) throw new Error('invalid_response: protobuf 定长字段截断')
      continue
    } else throw new Error('invalid_response: protobuf wire type 不支持')
    ;(fields[field] ||= []).push(value)
  }
  return fields
}

function geminiWireMessage(fields, key) {
  return Array.isArray(fields?.[key]?.[0]) ? geminiWireDecode(fields[key][0]) : {}
}

function geminiWireText(fields, key) {
  const bytes = fields?.[key]?.[0]
  if (!Array.isArray(bytes)) return ''
  const encoded = geminiWireBase64(bytes).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
  return host.call('crypto.base64url_decode_utf8', { text: encoded }).value
}

function geminiWireInner(inner) {
  const encode = (items) =>
    geminiWireEncode(
      Array.from(items, (value, index) => [index + 1, Array.isArray(value) ? encode(value) : value])
    )
  const message = (items) =>
    geminiWireEncode(
      Array.from(items).flatMap((value, index) =>
        index === 3 && Array.isArray(value)
          ? value.map((file) => [4, encode(file)])
          : [[index + 1, Array.isArray(value) ? encode(value) : value]]
      )
    )
  return geminiWireEncode(
    inner.map((value, index) => [
      index + 1,
      index === 0
        ? message(value)
        : index === 30
          ? value.flatMap(geminiWireVarint)
          : index === 55 && Array.isArray(value?.[0])
            ? geminiWireEncode([[1, value[0].flatMap(geminiWireVarint)]])
            : index === 2 && Array.isArray(value)
              ? geminiWireEncode(
                  value.map((item, key) => [
                    key + 1,
                    key === 9 && item
                      ? Array.from(
                          host.call('crypto.base64_decode_latin1', { text: item }).value,
                          (c) => c.charCodeAt(0)
                        )
                      : item,
                  ])
                )
              : Array.isArray(value)
                ? encode(value)
                : value,
    ])
  )
}

function geminiWireEvent(value) {
  if (!Array.isArray(value) || typeof value[0] !== 'string' || ['noop', 'close'].includes(value[0]))
    return null
  const bytes = Array.from(
    host.call('crypto.base64_decode_latin1', { text: value[0] }).value,
    (char) => char.charCodeAt(0)
  )
  const envelope = geminiWireDecode(bytes)
  if (envelope[1]) return { ack: true }
  if (envelope[4]) return { end: true }
  const body = geminiWireMessage(envelope, 2)
  const candidate = geminiWireMessage(geminiWireMessage(body, 7), 1)
  const data = geminiWireMessage(geminiWireMessage(candidate, 4), 4)
  if (!Object.keys(data).length) return { done: candidate[1]?.[0] === 1 }
  const frame = Array(43).fill(null)
  const ids = geminiWireMessage(data, 2)
  frame[1] = [geminiWireText(ids, 1), geminiWireText(ids, 2)]
  // protobuf bytes 在 JSPB 中表示为 base64，图片上下文含二进制，不能按 UTF-8 解码。
  frame[26] = Array.isArray(data[27]?.[0]) ? geminiWireBase64(data[27][0]) : ''
  frame[31] = data[32]?.[0] === 1
  frame[42] = geminiWireText(data, 43)
  frame[4] = []
  const visit = (fields, depth = 0) => {
    if (depth > 4) return
    let id = ''
    try {
      id = geminiWireText(fields, 1)
    } catch (_) {}
    if (id.startsWith('rc_')) {
      const item = Array(38).fill(null)
      item[0] = id
      item[1] = [geminiWireText(geminiWireMessage(fields, 2), 1)]
      item[37] = [[geminiWireText(geminiWireMessage(geminiWireMessage(fields, 38), 1), 1)]]
      frame[4].push(item)
      return
    }
    for (const values of Object.values(fields))
      for (const value of values)
        if (Array.isArray(value)) {
          try {
            visit(geminiWireDecode(value), depth + 1)
          } catch (_) {}
        }
  }
  for (const bytes of data[5] || []) visit(geminiWireDecode(bytes))
  return { frame, done: candidate[1]?.[0] === 1 }
}
