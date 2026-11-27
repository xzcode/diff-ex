import { describe, expect, it } from 'vitest'
import { mergeJsonNode } from '../src/lib/json-node-merge'
const merge = (source: string, target: string, path: string) => {
  const edit = mergeJsonNode(source, target, path)
  return target.slice(0, edit.offset) + edit.text + target.slice(edit.offset + edit.length)
}
describe('lossless JSON node merge', () => {
  it('replaces only the selected raw value without rounding large numbers', () => {
    expect(merge('{"a":900719925474099312345,"b":false}', '{ "a" : 0, "b" : true }', '/a')).toBe('{ "a" : 900719925474099312345, "b" : true }')
  })
  it('handles JSON pointer escaping, null and empty keys', () => {
    expect(merge('{"a/b~":null,"":2}', '{"a/b~":false,"":1}', '/a~1b~0')).toBe('{"a/b~":null,"":1}')
    expect(merge('{"":2}', '{"":1}', '/')).toBe('{"":2}')
  })
  it('inserts object members without rewriting existing values', () => {
    expect(merge('{"new":1e+3000}', '{"old":9007199254740993}', '/new')).toBe('{"old":9007199254740993, "new": 1e+3000}')
    expect(merge('{"a":1}', '{}', '/a')).toBe('{"a": 1}')
  })
  it('deletes first, middle, last and sole members without invalid commas', () => {
    const target = '{"a":1, "b":2, "c":3}'
    expect(JSON.parse(merge('{}', target, '/a'))).toEqual({b:2,c:3})
    expect(JSON.parse(merge('{}', target, '/b'))).toEqual({a:1,c:3})
    expect(JSON.parse(merge('{}', target, '/c'))).toEqual({a:1,b:2})
    expect(merge('{}','{"a":1}','/a')).toBe('{}')
  })
  it('merges ordered array items and refuses out-of-order additions', () => {
    expect(merge('[1,2,3]', '[1,2]', '/2')).toBe('[1,2, 3]')
    expect(merge('[1]', '[1,2,3]', '/1')).toBe('[1,3]')
    expect(() => merge('[1,2,3]', '[]', '/2')).toThrow()
    expect(merge('[]','[1,2]','')).toBe('[]')
  })
  it('rejects invalid, duplicate and stale paths instead of silently altering data', () => {
    expect(() => merge('{"a":1,"a":2}', '{}', '/a')).toThrow('Duplicate')
    expect(() => merge('{"a":1}', '{broken', '/a')).toThrow('Invalid')
    expect(() => merge('{}', '{}', '/missing')).toThrow()
    expect(() => merge('{"a":{"x":1}}', '{"a":null}', '/a/x')).toThrow()
  })
})
