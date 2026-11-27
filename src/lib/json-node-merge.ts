import { parseTree, type Node, type ParseError } from 'jsonc-parser'

// Byte-for-byte value copying from the source text, with localized syntax
// edits to the target. Never serialize through JavaScript numbers.
export interface JsonEdit { offset: number; length: number; text: string }

function strictTree(text: string): Node {
  const errors: ParseError[] = []
  const root = parseTree(text, errors, { allowTrailingComma: false, disallowComments: true, allowEmptyContent: false })
  if (!root || errors.length) throw new Error('Invalid JSON')
  const validate = (node: Node) => {
    if (node.type === 'object') {
      const keys = new Set<string>()
      for (const property of node.children || []) {
        const key = property.children![0].value as string
        if (keys.has(key)) throw new Error('Duplicate JSON key')
        keys.add(key)
        validate(property.children![1])
      }
    } else if (node.type === 'array') {
      for (const child of node.children || []) validate(child)
    }
  }
  validate(root)
  return root
}
function tokens(pointer: string): string[] {
  if (!pointer) return []
  if (!pointer.startsWith('/') || /~(?![01])/.test(pointer)) throw new Error('Invalid JSON pointer')
  return pointer.slice(1).split('/').map(part => part.replace(/~1/g, '/').replace(/~0/g, '~'))
}
function member(parent: Node, key: string): Node | undefined {
  if (parent.type === 'object') return parent.children?.find(p => p.children![0].value === key)
  if (parent.type === 'array' && /^(0|[1-9]\d*)$/.test(key)) {
    const index = Number(key)
    if (Number.isSafeInteger(index)) return parent.children?.[index]
  }
  return undefined
}
function value(node: Node | undefined): Node | undefined {
  return node?.type === 'property' ? node.children![1] : node
}
function lookup(root: Node, path: string[]): Node | undefined {
  let node: Node | undefined = root
  for (const key of path) {
    if (!node) return undefined
    node = value(member(node, key))
  }
  return node
}
const end = (node: Node) => node.offset + node.length
const slice = (text: string, node: Node) => text.slice(node.offset, end(node))

export function mergeJsonNode(source: string, target: string, pointer: string): JsonEdit {
  const from = strictTree(source), to = strictTree(target)
  const parts = tokens(pointer)
  const sourceValue = lookup(from, parts)
  const targetValue = lookup(to, parts)
  let edit: JsonEdit
  if (sourceValue && targetValue) {
    edit = { offset: targetValue.offset, length: targetValue.length, text: slice(source, sourceValue) }
  } else {
    if (!parts.length || (!sourceValue && !targetValue)) throw new Error('JSON merge path no longer exists')
    const key = parts[parts.length - 1]!
    const parent = lookup(to, parts.slice(0, -1))
    const sourceParent = lookup(from, parts.slice(0, -1))
    if (!parent || !sourceParent || parent.type !== sourceParent.type || (parent.type !== 'object' && parent.type !== 'array')) throw new Error('JSON merge parent no longer matches')
    const children = parent.children || []
    const targetMember = member(parent, key)
    if (!sourceValue && targetMember) {
      const i = children.indexOf(targetMember)
      // Delete the selected member and an adjacent comma, not unrelated values.
      const next = children[i + 1], previous = children[i - 1]
      const start = !next && previous ? end(previous) : targetMember.offset
      const finish = next ? next.offset : end(targetMember)
      edit = { offset: start, length: finish - start, text: '' }
    } else if (sourceValue && !targetMember) {
      if (parent.type === 'array' && key !== String(children.length)) throw new Error('Merge array additions in index order')
      const raw = parent.type === 'object' ? JSON.stringify(key) + ': ' + slice(source, sourceValue) : slice(source, sourceValue)
      const last = children[children.length - 1]
      if (last) {
        const first = children[0]!
        const between = target.slice(parent.offset + 1, first.offset)
        const separator = /\r|\n/.test(between) ? between : between || ' '
        edit = { offset: end(last), length: 0, text: ',' + separator + raw }
      } else {
        edit = { offset: parent.offset + 1, length: 0, text: raw }
      }
    } else throw new Error('JSON merge path no longer exists')
  }
  // Reparse the result before returning the edit. An invalid output never
  // reaches the editor or any file-writing operation.
  strictTree(target.slice(0, edit.offset) + edit.text + target.slice(edit.offset + edit.length))
  return edit
}
