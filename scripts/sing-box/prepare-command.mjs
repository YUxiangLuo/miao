import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const source = process.argv[2]
if (!source) throw new Error('Usage: bun prepare-command.mjs <upstream checkout>')

const target = join(source, 'cmd/miao-kernel')
mkdirSync(target)

// Copy the pinned upstream implementations, including signal handling and
// namespace helpers. No private copy of run/check lifecycle logic to maintain.
for (const name of [
  'main.go', 'cmd.go', 'cmd_check.go', 'cmd_run.go', 'cmd_version.go',
  'cmd_netns_holder.go', 'cmd_run_userns_linux.go', 'cmd_run_userns_other.go',
]) {
  copyFileSync(join(source, 'cmd/sing-box', name), join(target, name))
}

function replaceExactlyOnce(name, before, after) {
  const path = join(target, name)
  const text = readFileSync(path, 'utf8')
  if (text.split(before).length !== 2) {
    throw new Error(`Upstream ${name} changed; review Miao command branding`)
  }
  writeFileSync(path, text.replace(before, after))
}

replaceExactlyOnce('cmd.go', 'Use:              "sing-box",', 'Use:              "miao-kernel",')
replaceExactlyOnce('cmd_version.go', '"sing-box version "', '"miao-kernel version "')

for (const name of ['miao_context_test.go', 'miao_registry_test.go', 'miao_config_test.go']) {
  copyFileSync(fileURLToPath(new URL(`./tests/${name}`, import.meta.url)), join(target, name))
}
