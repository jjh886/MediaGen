// 设备证明采用 reference/sub2api 的 DeviceCheck 与 CBOR 协议。平台规则仅存在于插件。

const addon = require(process.argv[2])
const { execFileSync } = require('node:child_process')
const raw = execFileSync(
  '/usr/bin/osascript',
  [
    '-l',
    'JavaScript',
    '-e',
    `ObjC.import("Foundation"); ObjC.import("AppKit");
const screen = $.NSScreen.mainScreen; const frame = screen.frame;
JSON.stringify({locale: ObjC.unwrap($.NSLocale.currentLocale.localeIdentifier),languages: ObjC.deepUnwrap($.NSLocale.preferredLanguages),timezone: ObjC.unwrap($.NSTimeZone.localTimeZone.name),width:Number(frame.size.width),height:Number(frame.size.height),scale:Number(screen.backingScaleFactor)})`,
  ],
  { timeout: 5000, maxBuffer: 65536 }
)
const values = JSON.parse(raw.toString())
const signals = {
  schemaVersion: 1,
  preferredLanguages: values.languages.slice(0, 16),
  locale: values.locale,
  timezone: values.timezone,
  screenSizeSum: Math.round(values.width + values.height),
  screenScale: values.scale || 1,
  appSessionId: process.argv[4],
}
const bundleID = execFileSync(
  '/usr/bin/plutil',
  ['-extract', 'CFBundleIdentifier', 'raw', process.argv[3]],
  { timeout: 5000, maxBuffer: 65536 }
)
  .toString()
  .trim()
if (!bundleID.startsWith('com.openai.')) throw new Error('官方应用标识无效')

function head(major, value) {
  if (value < 24) return Buffer.from([major + value])
  if (value <= 255) return Buffer.from([major + 24, value])
  if (value <= 65535) {
    const out = Buffer.allocUnsafe(3)
    out[0] = major + 25
    out.writeUInt16BE(value, 1)
    return out
  }
  const out = Buffer.allocUnsafe(5)
  out[0] = major + 26
  out.writeUInt32BE(value, 1)
  return out
}
function uint(value) {
  return head(0, value)
}
function text(value) {
  const body = Buffer.from(value, 'utf8')
  return Buffer.concat([head(96, body.length), body])
}
function float(value) {
  if (Number.isSafeInteger(value) && value >= 0) return uint(value)
  const out = Buffer.allocUnsafe(9)
  out[0] = 251
  out.writeDoubleBE(value, 1)
  return out
}
function array(values) {
  return Buffer.concat([head(128, values.length), ...values])
}
function map(entries) {
  return Buffer.concat([
    head(160, entries.length),
    ...entries.flatMap(([key, value]) => [uint(key), value]),
  ])
}
function field(key, value) {
  return Buffer.concat([text(key), text(value)])
}
function base64url(value) {
  return value.toString('base64').replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/u, '')
}

;(async () => {
  const result = await addon.generateToken()
  if (!result || !result.supported) throw new Error('DeviceCheck is not supported on this Mac')
  if (!result.tokenBase64) throw new Error('DeviceCheck returned no token')
  const fingerprint = map([
    [0, uint(signals.schemaVersion)],
    [1, array(signals.preferredLanguages.map(text))],
    [2, text(signals.locale)],
    [3, text(signals.timezone)],
    [4, uint(signals.screenSizeSum)],
    [5, float(signals.screenScale)],
    [6, text(signals.appSessionId)],
  ])
  const fields = [
    field('token', result.tokenBase64),
    field('bundle_id', bundleID),
    Buffer.concat([text('f'), head(64, fingerprint.length), fingerprint]),
  ]
  if (result.latencyMs != null) {
    fields.push(Buffer.concat([text('t'), float(result.latencyMs)]))
  }
  const token = 'v1.' + base64url(Buffer.concat([Buffer.from([160 + fields.length]), ...fields]))
  process.stdout.write(JSON.stringify({ v: 1, s: 0, t: token }))
})().catch((error) => {
  process.stderr.write(error instanceof Error ? error.message : String(error))
  process.exitCode = 1
})
