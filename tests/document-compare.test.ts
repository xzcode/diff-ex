import {describe,it,expect} from 'vitest'
import {compareDocuments,differs} from '../src/lib/document-compare'
import {defaultPreferences,decodeWorkspace,newSession,defaultSshProfile} from '../src/lib/workspace'
import type {DocumentData,DocUnit} from '../src/contracts'
function unit(text:string,layout='x',warning:string|null=null):DocUnit{return {label:text,text,layout,layoutFingerprint:layout,warning,page:null}}
const doc=(units:DocUnit[],format='DOCX'):DocumentData=>({format,units,notes:[],fingerprint:'file'})
describe('document comparison',()=>{
  it('aligns inserted Word paragraphs, detects structural-only changes',()=>{
    const left=doc([unit('','layout A'),unit('a'),unit('b')])
    const right=doc([unit('','layout B'),unit('a'),unit('insert'),unit('b','style B')])
    const rows=compareDocuments(left,right)
    expect(rows.map(r=>[r.textStatus,r.layoutStatus])).toEqual([['same','different'],['same','same'],['rightOnly','unknown'],['same','different']])
    expect(rows.filter(differs)).toHaveLength(3)
  })
  it('keeps PDF page positions and does not assert equality on failed extraction',()=>{
    const rows=compareDocuments(doc([unit('','x','no text'),unit('one')],'PDF'),doc([unit('','x','no text'),unit('two')],'PDF'))
    expect(rows[0]?.textStatus).toBe('unknown')
    expect(rows[1]?.textStatus).toBe('different')
  })
})
describe('version source persistence',()=>{
  it('roundtrips Git selections, remote profiles and document references',()=>{
    const git=newSession('git'),remote=newSession('remote'),word=newSession('document')
    git.gitSources!.left.repository='/tmp/project';git.gitSources!.right.revision='refs/heads/feature'
    const profile={...defaultSshProfile(),name:'Test',host:'example.invalid',username:'reader',fingerprint:'SHA256:fixture',auth:'key' as const,keyPath:'/tmp/key'}
    remote.remoteSources!.right.profile=profile;remote.remoteSources!.right.path='/data'
    const w={version:1,sessions:[git,remote,word],activeId:remote.id,preferences:{...defaultPreferences,remoteProfiles:[profile]},savedSessions:[]}
    const restored=decodeWorkspace(JSON.stringify(w))
    expect(restored.sessions[0]?.gitSources?.right.revision).toBe('refs/heads/feature')
    expect(restored.sessions[1]?.remoteSources?.right.profile.host).toBe('example.invalid')
    expect(restored.preferences.remoteProfiles?.[0]).toEqual(profile)
    remote.remoteSources!.right.profile.port=0
    expect(()=>decodeWorkspace(JSON.stringify(w))).toThrow()
  })
})
