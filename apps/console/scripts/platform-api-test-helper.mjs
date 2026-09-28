import { readFileSync } from 'node:fs'
import vm from 'node:vm'

export function scripts(names, host = {}) {
  const context = vm.createContext({ host, console })
  for (const name of names) {
    vm.runInContext(
      readFileSync(new URL('../../../plugins/' + name, import.meta.url), 'utf8'),
      context
    )
  }
  return (source, values = {}) => {
    Object.assign(context, values)
    return vm.runInContext(source, context)
  }
}
