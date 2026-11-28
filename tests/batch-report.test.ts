import {describe,it,expect} from 'vitest'
import {newSession,decodeWorkspace,defaultDbProfile,defaultPreferences} from '../src/lib/workspace'
import {reportHtml} from '../src/lib/report'
describe('database and batch workspaces',()=>{
  it('persists database descriptors and batch tasks without requiring secrets',()=>{
    const db=newSession('database'),batch=newSession('batch')
    db.dbSources!.right.profile={...defaultDbProfile('postgres'),name:'fixture',database:'test'}
    db.dbSources!.right.relation='items'
    db.dbOptions!.keys=['id','sub']
    batch.batchItems=[{id:'one',kind:'text',left:'/a',right:'/b'}]
    const w={version:1,sessions:[db,batch],activeId:batch.id,preferences:defaultPreferences,savedSessions:[]}
    const restored=decodeWorkspace(JSON.stringify(w))
    expect(restored.sessions[0]?.dbOptions?.keys).toEqual(['id','sub'])
    expect(restored.sessions[1]?.batchItems?.[0]?.left).toBe('/a')
    batch.batchItems.push({...batch.batchItems[0]!})
    expect(()=>decodeWorkspace(JSON.stringify(w))).toThrow()
  })
  it('escapes arbitrary user data in HTML reports',()=>{
    const html=reportHtml('<script>title</script>',{value:'<script>payload</script>'})
    expect(html).not.toContain('<script>')
    expect(html).toContain('&lt;script&gt;')
  })
})
