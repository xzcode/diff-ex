// Browser preview helpers. Desktop calls the independent Rust engines.
import { diffArrays } from 'diff'
import { parseTree, printParseErrorCode, type Node, type ParseError } from 'jsonc-parser'
import type { TextOptions, TextDiffResult, StructuredOptions, StructuredResult, StructureChange } from '../contracts'

export function previewText(left: string, right: string, options: TextOptions): TextDiffResult {
  if (options.algorithm !== 'myers') throw new Error('Patience and Histogram are available in the desktop app. Select Myers in the browser preview.')
  if (options.ignoreLineRegex) throw new Error('Regular expression filtering requires the desktop app.')
  const prefixes = (options.ignoreLinePrefixes || []).map(p => p.trim()).filter(Boolean)
  const prepare = (text: string) => {
    const lines = text.split(/\r\n|\r|\n/)
    const endings = text.match(/\r\n|\r|\n/g) || []
    const result: { text: string; line: number }[] = []
    lines.forEach((line, index) => {
      if (options.ignoreBlankLines && !line.trim()) return
      if (prefixes.some(prefix => line.trimStart().startsWith(prefix))) return
      let value = options.whitespace === 'all' ? line.replace(/\s/g, '') : options.whitespace === 'trim' ? line.trim() : line
      if (options.ignoreCase) value = value.toLowerCase()
      if (!options.ignoreLineEndings) value += endings[index] || ''
      result.push({ text: value, line: index })
    })
    return { result, length: lines.length }
  }
  const a = prepare(left), b = prepare(right)
  const changes = diffArrays(a.result.map(v => v.text), b.result.map(v => v.text))
  const out: TextDiffResult = { hunks: [], equal: true, added: 0, removed: 0, modified: 0, unchanged: 0 }
  let ai = 0, bi = 0
  let pending: { a: number; b: number; removed: number; added: number } | null = null
  const start = (list: typeof a, i: number) => list.result[i]?.line ?? (list.result.length ? list.result[list.result.length - 1]!.line + 1 : 0)
  const end = (list: typeof a, i: number, count: number) => count ? (list.result[i + count - 1]?.line ?? list.length - 1) + 1 : start(list, i)
  const flush = () => {
    if (!pending) return
    const h = pending
    out.hunks.push({ leftStart: start(a, h.a), leftEnd: end(a, h.a, h.removed), rightStart: start(b, h.b), rightEnd: end(b, h.b, h.added) })
    out.modified += Math.min(h.removed, h.added)
    out.removed += Math.max(0, h.removed - h.added)
    out.added += Math.max(0, h.added - h.removed)
    pending = null
  }
  for (const part of changes) {
    if (!part.added && !part.removed) { flush(); ai += part.count; bi += part.count; out.unchanged += part.count }
    else {
      pending ??= { a: ai, b: bi, removed: 0, added: 0 }
      if (part.removed) { pending.removed += part.count; ai += part.count }
      if (part.added) { pending.added += part.count; bi += part.count }
    }
  }
  flush()
  out.equal = out.hunks.length === 0
  return out
}

type Semantic = { kind: 'object'; entries: Map<string, Semantic> } | { kind: 'array'; items: Semantic[] } |
  { kind: 'string'; value: string } | { kind: 'number'; value: string } | { kind: 'boolean'; value: boolean } | { kind: 'null' }
function numberKey(raw: string): string {
  // Exact decimal canonicalization without converting through IEEE 754.
  const match = /^(-?)(\d+)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/.exec(raw)!
  let digits = (match[2] + (match[3] || '')).replace(/^0+/, '')
  if (!digits) return '0'
  let scale = BigInt(match[4] || '0') - BigInt((match[3] || '').length)
  const suffix = /0+$/.exec(digits)?.[0].length || 0
  if (suffix) { digits = digits.slice(0, -suffix); scale += BigInt(suffix) }
  return match[1] + digits + 'e' + scale.toString()
}
function parseJson(text: string, side: string): Semantic {
  const errors: ParseError[] = []
  const tree = parseTree(text, errors, { allowTrailingComma: false, disallowComments: true, allowEmptyContent: false })
  if (!tree || errors.length) {
    const err = errors[0]
    const before = text.slice(0, err?.offset || 0)
    const line = before.split('\n').length
    const col = before.length - before.lastIndexOf('\n')
    throw new Error(`${side}: ${err ? printParseErrorCode(err.error) : 'Invalid JSON'} at ${line}:${col}`)
  }
  const convert = (n: Node): Semantic => {
    if (n.type === 'object') {
      const entries = new Map<string, Semantic>()
      for (const p of n.children || []) {
        const key: string = p.children![0].value
        if (entries.has(key)) throw new Error(`${side}: duplicate object key ${JSON.stringify(key)}`)
        entries.set(key, convert(p.children![1]))
      }
      return { kind: 'object', entries }
    }
    if (n.type === 'array') return { kind: 'array', items: (n.children || []).map(convert) }
    if (n.type === 'number') return { kind: 'number', value: text.slice(n.offset, n.offset + n.length) }
    if (n.type === 'string') return { kind: 'string', value: n.value as string }
    if (n.type === 'boolean') return { kind: 'boolean', value: n.value as boolean }
    return { kind: 'null' }
  }
  return convert(tree)
}
function semanticKey(node: Semantic, options: StructuredOptions): string {
  if (node.kind === 'null') return 'null'
  if (node.kind === 'number') return `n:${numberKey(node.value)}`
  if (node.kind === 'boolean') return `b:${node.value}`
  if (node.kind === 'string') return `s:${JSON.stringify(options.ignoreCase ? node.value.toLowerCase() : node.value)}`
  if (node.kind === 'object') return `{${[...node.entries.keys()].sort().map(k => `${JSON.stringify(k)}:${semanticKey(node.entries.get(k)!, options)}`).join(',')}}`
  const keys = node.items.map(v => semanticKey(v, options))
  if (options.ignoreArrayOrder) keys.sort()
  return `[${keys.join(',')}]`
}
function display(node: Semantic, level = 0): string {
  if (node.kind === 'null') return 'null'
  if (node.kind === 'number') return node.value
  if (node.kind === 'string' || node.kind === 'boolean') return JSON.stringify(node.value)
  const indent = '  '.repeat(level)
  if (node.kind === 'array') return node.items.length ? `[\n${node.items.map(v => indent + '  ' + display(v, level + 1)).join(',\n')}\n${indent}]` : '[]'
  const keys = [...node.entries.keys()].sort()
  return keys.length ? `{\n${keys.map(k => indent + '  ' + JSON.stringify(k) + ': ' + display(node.entries.get(k)!, level + 1)).join(',\n')}\n${indent}}` : '{}'
}
export function previewStructured(left: string, right: string, options: StructuredOptions): StructuredResult {
  if (options.format !== 'json') throw new Error('YAML and XML semantic comparison use the desktop engine. JSON is available in this browser preview.')
  const a = parseJson(left, 'Left'), b = parseJson(right, 'Right')
  const changes: StructureChange[] = []
  const escape = (key: string) => key.replace(/~/g, '~0').replace(/\//g, '~1')
  const walk = (l: Semantic | undefined, r: Semantic | undefined, path: string) => {
    if (!l || !r) { changes.push({ path, kind: l ? 'removed' : 'added', left: l ? display(l) : null, right: r ? display(r) : null }); return }
    if (semanticKey(l, options) === semanticKey(r, options)) return
    if (l.kind !== r.kind) { changes.push({ path, kind: 'typeChanged', left: display(l), right: display(r) }); return }
    if (l.kind === 'object' && r.kind === 'object') {
      const keys = [...new Set([...l.entries.keys(), ...r.entries.keys()])].sort()
      keys.forEach(k => walk(l.entries.get(k), r.entries.get(k), path + '/' + escape(k)))
    } else if (l.kind === 'array' && r.kind === 'array' && !options.ignoreArrayOrder) {
      for (let i = 0; i < Math.max(l.items.length, r.items.length); i++) walk(l.items[i], r.items[i], path + '/' + i)
    } else changes.push({ path, kind: 'changed', left: display(l), right: display(r) })
  }
  walk(a, b, '')
  return { equal: changes.length === 0, changes, leftCanonical: display(a), rightCanonical: display(b) }
}
