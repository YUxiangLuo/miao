import fs from 'node:fs'

const [, , inputPath, outputPath] = process.argv

if (!inputPath || !outputPath) {
  console.error('Usage: bun compile-direct-rules.mjs <input> <output>')
  process.exit(1)
}

const text = fs.readFileSync(inputPath, 'utf8')
const domain = []
const domainSuffix = []
const domainRegex = []

for (const raw of text.split('\n')) {
  const line = raw.trim()
  if (!line || line.startsWith('#')) continue

  if (line.startsWith('full:')) domain.push(line.slice(5))
  else if (line.startsWith('regexp:')) domainRegex.push(line.slice(7))
  else if (line.startsWith('domain:')) domainSuffix.push(line.slice(7))
  else domainSuffix.push(line)
}

const rules = {}
if (domain.length) rules.domain = domain
if (domainSuffix.length) rules.domain_suffix = domainSuffix
if (domainRegex.length) rules.domain_regex = domainRegex

fs.writeFileSync(
  outputPath,
  JSON.stringify({ version: 4, rules: [rules] }, null, 2),
)

const total = domain.length + domainSuffix.length + domainRegex.length
console.log(
  `Parsed ${total} rules: domain=${domain.length}, domain_suffix=${domainSuffix.length}, domain_regex=${domainRegex.length}`,
)
