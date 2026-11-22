import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const source = process.argv[2]
if (!source) throw new Error('Usage: bun prepare-command.mjs <upstream checkout>')

const target = join(source, 'cmd/harbor-kernel')
mkdirSync(target)

// Copy the pinned upstream implementations, including signal handling and
// namespace helpers. Small checked hooks acknowledge the exact loaded config;
// upstream still owns the run/check/reload lifecycle.
for (const name of [
  'main.go', 'cmd.go', 'cmd_check.go', 'cmd_run.go', 'cmd_version.go',
  'cmd_netns_holder.go', 'cmd_run_userns_linux.go', 'cmd_run_userns_other.go',
]) {
  copyFileSync(join(source, 'cmd/sing-box', name), join(target, name))
}

function replaceExactlyOnce(name, before, after) {
  const path = join(target, name)
  const text = readFileSync(path, 'utf8').replaceAll('\r\n', '\n')
  if (text.split(before).length !== 2) {
    throw new Error(`Upstream ${name} changed; review Harbor command hooks`)
  }
  writeFileSync(path, text.replace(before, after))
}

replaceExactlyOnce('cmd.go', 'Use:              "sing-box",', 'Use:              "harbor-kernel",')
replaceExactlyOnce('cmd_version.go', '"sing-box version "', '"harbor-kernel version "')

copyFileSync(fileURLToPath(new URL('./harbor_activation.go', import.meta.url)), join(target, 'harbor_activation.go'))
replaceExactlyOnce('cmd_run.go',
  'func run() error {\n\toptionsList, err := readConfig()',
  'func run() error {\n\toptionsList, activation, err := readHarborConfig()')
replaceExactlyOnce('cmd_run.go',
  '\t\truntimeDebug.FreeOSMemory()',
  '\t\tif err = activation.publish(); err != nil {\n\t\t\tcancel()\n\t\t\tinstance.Close()\n\t\t\treturn err\n\t\t}\n\t\truntimeDebug.FreeOSMemory()')
replaceExactlyOnce('cmd_run.go',
  '\t\toptions, err = readConfigAndMerge()',
  '\t\toptionsList, activation, err = readHarborConfig()\n\t\tif err != nil {\n\t\t\treturn err\n\t\t}\n\t\toptions, err = mergeOptionsList(optionsList)')

for (const name of ['harbor_context_test.go', 'harbor_registry_test.go', 'harbor_config_test.go', 'harbor_shadowsocks_test.go', 'harbor_activation_test.go', 'harbor_activation_unix_test.go']) {
  copyFileSync(fileURLToPath(new URL(`./tests/${name}`, import.meta.url)), join(target, name))
}
