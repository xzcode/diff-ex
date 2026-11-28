import {describe,it,expect} from 'vitest'
import {planThreeWay,mergeText,splitExactLines,mergeReady} from '../src/lib/three-way'
import {newSession,decodeWorkspace,defaultPreferences,isDirty} from '../src/lib/workspace'

const plan=planThreeWay
const merged=(base:string,left:string,right:string)=>mergeText(plan(base,left,right),{})
describe('three-way merge',()=>{
  it('merges independent and adjacent base-relative changes without altering unchanged text',()=>{
    expect(merged('a\nb\nc\n','A\nb\nc\n','a\nB\nc\n')).toBe('A\nB\nc\n')
    expect(merged('a\r\nb\r\nc','A\r\nb\r\nc','a\r\nb\r\nC')).toBe('A\r\nb\r\nC')
    expect(merged('a\nb\nc\n','a\nc\n','a\nb\nC\n')).toBe('a\nC\n')
  })
  it('accepts identical changes once, and preserves exact empty and newline semantics',()=>{
    expect(merged('a\n','x\n','x\n')).toBe('x\n')
    expect(merged('','x','x')).toBe('x')
    expect(merged('','x','')).toBe('x')
    expect(merged('a\n','','a\n')).toBe('')
    expect(splitExactLines('a\r\nb\rc\n')).toEqual(['a\r\n','b\r','c\n'])
    expect(merged('a\n','a','a\n')).toBe('a')
  })
  it('requires explicit choices for unequal inserts, overlapping changes and delete/modify',()=>{
    for(const [base,l,r] of [['','L','R'],['x\ny\n','L\ny\n','R\ny\n'],['a\nb\n','a\n','a\nB\n'],['a\nb\n','a\ninsert\nb\n','a\nB\n']]){
      const segments=plan(base,l,r), conflict=segments.find(s=>s.kind==='conflict')!
      expect(conflict).toBeTruthy()
      expect(mergeText(segments,{})).toBeNull()
      expect(mergeText(segments,{[conflict.id]:{choice:'left',text:''}})).toBe(l)
      expect(mergeText(segments,{[conflict.id]:{choice:'right',text:''}})).toBe(r)
    }
  })
  it('resolves multiple conflicts independently including manual empty replacement',()=>{
    const segments=plan('a\nmid\nb\n','L\nmid\nX\n','R\nmid\nY\n')
    const conflicts=segments.filter(s=>s.kind==='conflict')
    expect(conflicts).toHaveLength(2)
    const resolutions={[conflicts[0]!.id]:{choice:'base' as const,text:''},[conflicts[1]!.id]:{choice:'manual' as const,text:''}}
    expect(mergeText(segments,resolutions)).toBe('a\nmid\n')
  })
  it('restores a prepared merge and tracks unsaved result content',()=>{
    const s=newSession('merge')
    s.merge!.prepared=true;s.merge!.base.content='a\n';s.merge!.savedBase='a\n';s.left.content='a\n';s.right.content='b\n'
    s.merge!.output.content='b\n'
    const w={version:1,sessions:[s],activeId:s.id,preferences:defaultPreferences,savedSessions:[]}
    const restored=decodeWorkspace(JSON.stringify(w)).sessions[0]!
    expect(mergeReady(restored.merge,restored.left.content,restored.right.content)).toBe(true)
    expect(isDirty(restored)).toBe(true)
    s.merge!.prepared=false
    expect(mergeReady(s.merge,s.left.content,s.right.content)).toBe(false)
  })
})
