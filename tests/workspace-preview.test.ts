import { describe, expect, it } from 'vitest'
import { decodeWorkspace, defaultPreferences, isDirty, isSideDirty, metadataSignature, newSession } from '../src/lib/workspace'
import { previewStructured, previewText } from '../src/lib/preview'
import type { StructuredOptions } from '../src/contracts'
const json: StructuredOptions = { format: 'json', ignoreArrayOrder: false, ignoreCase: false }
describe('workspace persistence correctness', () => {
  it('tracks encoding, BOM and EOL edits even without content edits', () => {
    const s = newSession()
    expect(isDirty(s)).toBe(false)
    s.left.bom = true
    expect(isSideDirty(s, 'left')).toBe(true)
    expect(isSideDirty(s, 'right')).toBe(false)
    s.savedLeftMetadata = metadataSignature(s.left)
    expect(isDirty(s)).toBe(false)
    s.right.lineEnding = 'crlf'
    expect(isDirty(s)).toBe(true)
  })
  it('roundtrips drafts and rules and rejects damaged/unsupported snapshots', () => {
    const s = newSession()
    s.left.content = 'unsaved\n漢字'
    s.textOptions.ignoreBlankLines = true
    s.left.bom = true
    const w = { version: 1, sessions: [s], activeId: s.id, preferences: defaultPreferences, savedSessions: [] }
    const restored = decodeWorkspace(JSON.stringify(w))
    expect(restored.sessions[0].left.content).toBe(s.left.content)
    expect(isDirty(restored.sessions[0])).toBe(true)
    expect(restored.sessions[0].textOptions.ignoreBlankLines).toBe(true)
    expect(() => decodeWorkspace('{broken')).toThrow()
    expect(() => decodeWorkspace(JSON.stringify({ ...w, version: 2 }))).toThrow()
    expect(() => decodeWorkspace(JSON.stringify({ ...w, sessions: [s, s] }))).toThrow('Duplicate')
  })
})
describe('browser preview does not claim unsupported or lossy semantics', () => {
  it('keeps exact large numbers, normalizes equivalent decimal spellings and retains array multiplicity', () => {
    expect(previewStructured('{"n":9007199254740993}', '{"n":9007199254740992}', json).equal).toBe(false)
    expect(previewStructured('{"n":1.00e2}', '{"n":100}', json).equal).toBe(true)
    expect(previewStructured('[1,2,1]', '[2,1,1]', { ...json, ignoreArrayOrder: true }).equal).toBe(true)
    expect(previewStructured('[1,1,2]', '[1,2,2]', { ...json, ignoreArrayOrder: true }).equal).toBe(false)
  })
  it('rejects duplicate keys, distinguishes null/missing/type and escapes pointer paths', () => {
    expect(() => previewStructured('{"x":1,"x":2}', '{}', json)).toThrow('duplicate')
    const r = previewStructured('{"a/b~c":null,"n":1}', '{"n":"1"}', json)
    expect(r.changes).toContainEqual({ path: '/a~1b~0c', kind: 'removed', left: 'null', right: null })
    expect(r.changes.find(c => c.path === '/n')?.kind).toBe('typeChanged')
  })
  it('case rules affect values, not keys', () => {
    expect(previewStructured('{"Name":"HELLO"}', '{"Name":"hello"}', { ...json, ignoreCase: true }).equal).toBe(true)
    expect(previewStructured('{"Name":1}', '{"name":1}', { ...json, ignoreCase: true }).equal).toBe(false)
  })
  it('filters explicit line prefixes without changing original hunk coordinates', () => {
    const o = { ...newSession().textOptions, ignoreLinePrefixes: ['#', '  '] }
    const r = previewText('head\n  # old\nold\ntail', 'head\n# new\nnew\ntail', o)
    expect(r.hunks).toEqual([{ leftStart: 2, leftEnd: 3, rightStart: 2, rightEnd: 3 }])
    expect(previewText('# old\nkeep', '# new\nkeep', o).equal).toBe(true)
    expect(previewText('x # old', 'x # new', o).equal).toBe(false)
    expect(() => previewText('a','a',{...o, ignoreLineRegex: '^DEBUG'})).toThrow('desktop')
  })
  it('compares raw line endings separately and refuses native-only modes', () => {
    const o = newSession().textOptions
    expect(previewText('a\r\n', 'a\n', o).equal).toBe(true)
    expect(previewText('a\r\n', 'a\n', { ...o, ignoreLineEndings: false }).equal).toBe(false)
    expect(previewText('\nA\n\n', 'a\n', { ...o, ignoreCase: true, ignoreBlankLines: true }).equal).toBe(true)
    expect(() => previewText('a', 'b', { ...o, algorithm: 'histogram' })).toThrow('desktop')
    expect(() => previewStructured('x: 1', 'x: 2', { ...json, format: 'yaml' })).toThrow('desktop')
  })
})
