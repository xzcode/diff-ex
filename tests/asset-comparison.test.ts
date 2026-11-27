import { describe, expect, it } from 'vitest'
import { countBytes, emptyStats } from '../src/lib/asset-io'
import { compareRaster, rasterMime, type Raster } from '../src/lib/image-diff'
import { parseDelimited, decodeDelimited, compareTables, alignColumns, compareCells, getCell, colLabel } from '../src/lib/table-diff'
import { decodeWorkspace, defaultPreferences, newSession } from '../src/lib/workspace'

describe('binary positional scan', () => {
  it('counts both missing and changed bytes across page boundaries', () => {
    const stats=emptyStats()
    countBytes(stats,new Uint8Array([0,1,2]),new Uint8Array([0,9,2]),0)
    countBytes(stats,new Uint8Array([7]),new Uint8Array([7,8,9]),3)
    expect(stats).toEqual({compared:6,changed:1,leftOnly:0,rightOnly:2,firstDifference:1,lastDifference:5})
  })
})
describe('decoded raster comparison', () => {
  const img=(w:number,h:number,rgba:number[]):Raster=>({width:w,height:h,data:new Uint8ClampedArray(rgba)})
  it('counts alpha, tolerance, and unmatched dimensions without counting empty corners',()=>{
    const left=img(2,1,[10,20,30,255,0,0,0,0]),right=img(1,2,[10,20,30,250,0,0,0,0])
    const result=compareRaster(left,right,0)
    expect([result.common,result.changed,result.leftOnly,result.rightOnly,result.maxDelta]).toEqual([1,1,1,1,5])
    expect(compareRaster(left,right,5).changed).toBe(0)
    expect(result.heat[15]).toBe(0)
  })
  it('recognizes raster signatures, not arbitrary text or SVG',()=>{
    expect(rasterMime(new Uint8Array([137,80,78,71,13,10,26,10]))).toBe('image/png')
    expect(rasterMime(new TextEncoder().encode('<svg/>'))).toBeUndefined()
  })
})
describe('delimited tables and row/column alignment',()=>{
  it('preserves quoted newlines, commas, doubled quotes, empty values and raw numbers',()=>{
    const table=parseDelimited('\uFEFFid,name,note\r\n0001,"a,b","line 1\r\nline ""2"""\r\n2,,900719925474099312345\r\n',',')
    expect(table.rows.length).toBe(3)
    expect(table.rows[1]?.map(c=>c?.value)).toEqual(['0001','a,b','line 1\r\nline "2"'])
    expect(table.rows[2]?.map(c=>c?.value)).toEqual(['2','','900719925474099312345'])
    expect(parseDelimited('a,\n',',').rows[0]?.[1]?.value).toBe('')
    expect(()=>parseDelimited('"unterminated',',')).toThrow('Unclosed')
    expect(()=>parseDelimited('"a"x',',')).toThrow()
    expect(decodeDelimited(new Uint8Array([255,254,97,0]),'auto')).toBe('a')
    expect(()=>decodeDelimited(new Uint8Array([255,254,97,0]),'utf-8')).toThrow()
  })
  it('aligns reordered keys and columns without treating position changes as cell edits',()=>{
    const a=parseDelimited('id,name,n\n1,a,7\n2,b,8\n3,c,9',',')
    const b=parseDelimited('name,id,n\nb,2,10\na,1,7\nd,4,11',',')
    const opts={header:true,columnMode:'header' as const,rowMode:'key' as const,keyColumn:'header:id',ignoreType:false}
    const r=compareTables(a,b,opts)
    expect(r.rows.map(row=>[row.key,row.status])).toEqual([['1','same'],['2','changed'],['3','leftOnly'],['4','rightOnly']])
    expect([r.changed,r.leftOnly,r.rightOnly,r.columnDifferences]).toEqual([1,3,3,0])
    expect(()=>compareTables(a,parseDelimited('id,name,n\n1,a,1\n1,b,2',','),opts)).toThrow('Duplicate key')
    expect(()=>compareTables(a,parseDelimited('id,name,n\n,a,1',','),opts)).toThrow('nonempty')
    expect(()=>alignColumns(parseDelimited('a,a\n1,2',','),b,true,'header')).toThrow('unique')
    expect(compareCells({kind:'number',value:'1'},{kind:'string',value:'1'},false)).toBe('changed')
    expect(compareCells({kind:'number',value:'1'},{kind:'string',value:'1'},true)).toBe('same')
  })
  it('preserves absolute worksheet positions and detects column differences',()=>{
    const a={...parseDelimited('x',','),rowStart:4,colStart:2}
    const b={...parseDelimited('x',','),rowStart:5,colStart:2}
    const r=compareTables(a,b,{header:false,columnMode:'position',rowMode:'position',keyColumn:'',ignoreType:false})
    expect(r.rows.map(row=>[row.left,row.right])).toEqual([[4,null],[null,5]])
    expect(getCell(a,4,2)?.value).toBe('x')
    expect(colLabel(26)).toBe('AA')
  })
})
describe('asset session restoration',()=>{
  it('persists descriptors and view options but not binary data',()=>{
    const s=newSession('binary')
    s.assetSources!.left={path:'/tmp/example.bin',name:'example.bin',size:1024,modified:'1'}
    s.assetSettings!.binaryOffset=512
    const raw=JSON.stringify({version:1,sessions:[s],activeId:s.id,preferences:defaultPreferences,savedSessions:[]})
    const restored=decodeWorkspace(raw)
    expect(restored.sessions[0]?.assetSources?.left?.size).toBe(1024)
    expect(restored.sessions[0]?.assetSettings?.binaryOffset).toBe(512)
    s.assetSettings!.binaryPageSize=0
    expect(()=>decodeWorkspace(JSON.stringify({version:1,sessions:[s],activeId:s.id,preferences:defaultPreferences,savedSessions:[]}))).toThrow()
  })
})
