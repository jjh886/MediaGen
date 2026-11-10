const timeOptions: Intl.DateTimeFormatOptions = {
  // Etc/GMT 的符号与 UTC 偏移相反；固定 +8，不随浏览器时区或夏令时变化。
  timeZone: 'Etc/GMT-8',
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  hourCycle: 'h23',
}
const shortFormatter = new Intl.DateTimeFormat('zh-CN', timeOptions)
const fullFormatter = new Intl.DateTimeFormat('zh-CN', {
  ...timeOptions,
  year: 'numeric',
  second: '2-digit',
})

export function formatDateTime(value?: string | null, compact = false): string {
  if (!value) return '-'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return '-'
  return (compact ? shortFormatter : fullFormatter).format(date)
}
