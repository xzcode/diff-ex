import type {TableCell,TableSheet} from '../contracts'
export type ColumnMode='position'|'header'
export type RowMode='position'|'key'
export type CellStatus='same'|'changed'|'leftOnly'|'rightOnly'
export interface ColumnPair{id:string;left:number|null;right:number|null;leftName:string;rightName:string;different:boolean}
export interface RowPair{left:number|null;right:number|null;key:string;id:string;status:CellStatus;changed:number;leftOnly:number;rightOnly:number}
export interface TableResult{columns:ColumnPair[];rows:RowPair[];changed:number;leftOnly:number;rightOnly:number;changedRows:number;leftRows:number;rightRows:number;columnDifferences:number}

export function parseDelimited(text:string,delimiter:string):TableSheet{
  if(![',','\t',';','|'].includes(delimiter))throw new Error('Unsupported delimiter.')
  const input=text.startsWith('\uFEFF')?text.slice(1):text,rows:TableSheet['rows']=[]
  let row:TableCell[]=[],field='',state:'start'|'plain'|'quoted'|'closed'='start'
  const add=()=>{row.push({kind:'string',value:field});field='';state='start'}
  const finish=()=>{add();rows.push(row);row=[]}
  for(let i=0;i<input.length;i++){
    const c=input[i]
    if(state==='quoted'){
      if(c==='"'){if(input[i+1]==='"'){field+='"';i++}else state='closed'}else field+=c
      continue
    }
    if(c===delimiter){add();continue}
    if(c==='\r'||c==='\n'){if(c==='\r'&&input[i+1]==='\n')i++;finish();continue}
    if(state==='closed')throw new Error('Unexpected character after a quoted field at offset '+i+'.')
    if(c==='"'){if(state!=='start')throw new Error('Unexpected quote at offset '+i+'.');state='quoted';continue}
    field+=c;state='plain'
  }
  if(state==='quoted')throw new Error('Unclosed quoted field.')
  if(row.length||state!=='start'||field)finish()
  return {rowStart:0,colStart:0,rows}
}
export function decodeDelimited(bytes:Uint8Array,encoding:string):string{
  const bom=bytes[0]===0xEF&&bytes[1]===0xBB&&bytes[2]===0xBF?'utf-8':bytes[0]===0xFF&&bytes[1]===0xFE?'utf-16le':bytes[0]===0xFE&&bytes[1]===0xFF?'utf-16be':undefined
  const selected=encoding==='auto'?bom||'utf-8':encoding
  if(bom&&selected!==bom)throw new Error('Selected encoding conflicts with the file BOM.')
  const text=new TextDecoder(selected,{fatal:true}).decode(bytes)
  if(text.includes('\0'))throw new Error('The table contains NUL characters. Check the encoding.')
  return text
}
export function colLabel(index:number):string{
  let n=index+1,result=''
  while(n>0){n--;result=String.fromCharCode(65+n%26)+result;n=Math.floor(n/26)}
  return result
}
export const tableWidth=(sheet:TableSheet)=>sheet.rows.reduce((max,row)=>Math.max(max,row.length),0)
export function getCell(sheet:TableSheet,row:number|null,col:number|null):TableCell|null{
  if(row===null||col===null)return null
  return sheet.rows[row-sheet.rowStart]?.[col-sheet.colStart]??null
}
export function compareCells(a:TableCell|null,b:TableCell|null,ignoreType:boolean):CellStatus{
  if(!a&&!b)return 'same'
  if(!a)return 'rightOnly'
  if(!b)return 'leftOnly'
  return a.value===b.value&&(ignoreType||a.kind===b.kind)?'same':'changed'
}
export function alignColumns(left:TableSheet,right:TableSheet,header:boolean,mode:ColumnMode):ColumnPair[]{
  const indexes=(sheet:TableSheet)=>Array.from({length:tableWidth(sheet)},(_,i)=>sheet.colStart+i)
  const title=(sheet:TableSheet,col:number|null)=>col===null?'':header?getCell(sheet,sheet.rowStart,col)?.value??'':colLabel(col)
  const a=indexes(left),b=indexes(right)
  const make=(id:string,l:number|null,r:number|null):ColumnPair=>{
    const leftName=title(left,l),rightName=title(right,r)
    return {id,left:l,right:r,leftName,rightName,different:l===null||r===null||(header&&leftName!==rightName)}
  }
  if(mode==='position'||!header){
    const l=new Set(a),r=new Set(b)
    return [...new Set([...a,...b])].sort((x,y)=>x-y).map(col=>make('position:'+col,l.has(col)?col:null,r.has(col)?col:null))
  }
  const map=(sheet:TableSheet,columns:number[])=>{
    const result=new Map<string,number>()
    for(const col of columns){const name=title(sheet,col);if(!name.trim()||result.has(name))throw new Error('Header matching requires nonempty, unique column names on both sides.');result.set(name,col)}
    return result
  }
  const l=map(left,a),r=map(right,b)
  return [...new Set([...l.keys(),...r.keys()])].map(name=>make('header:'+name,l.get(name)??null,r.get(name)??null))
}
export function compareTables(left:TableSheet,right:TableSheet,options:{header:boolean;columnMode:ColumnMode;rowMode:RowMode;keyColumn:string;ignoreType:boolean}):TableResult{
  const columns=alignColumns(left,right,options.header,options.columnMode)
  const indices=(sheet:TableSheet)=>Array.from({length:Math.max(0,sheet.rows.length-(options.header?1:0))},(_,i)=>sheet.rowStart+i+(options.header?1:0))
  const a=indices(left),b=indices(right)
  let pairs:{left:number|null;right:number|null;key:string;id:string}[]
  if(options.rowMode==='key'){
    const col=columns.find(c=>c.id===options.keyColumn)
    if(!col||col.left===null||col.right===null)throw new Error('Choose a key column present on both sides.')
    const build=(sheet:TableSheet,rows:number[],column:number)=>{
      const map=new Map<string,{row:number;display:string}>()
      for(const row of rows){
        const cell=getCell(sheet,row,column)
        if(!cell||!cell.value.trim())throw new Error('Key matching requires nonempty keys. Row '+(row+1)+'.')
        const key=JSON.stringify([options.ignoreType?'':cell.kind,cell.value])
        if(map.has(key))throw new Error('Duplicate key: '+cell.value)
        map.set(key,{row,display:cell.value})
      }
      return map
    }
    const l=build(left,a,col.left),r=build(right,b,col.right)
    pairs=[...new Set([...l.keys(),...r.keys()])].map(id=>({id,left:l.get(id)?.row??null,right:r.get(id)?.row??null,key:l.get(id)?.display??r.get(id)!.display}))
  }else{
    const l=new Set(a),r=new Set(b)
    pairs=[...new Set([...a,...b])].sort((x,y)=>x-y).map(i=>({id:'row:'+i,left:l.has(i)?i:null,right:r.has(i)?i:null,key:String(i+1)}))
  }
  const result:TableResult={columns,rows:[],changed:0,leftOnly:0,rightOnly:0,changedRows:0,leftRows:0,rightRows:0,columnDifferences:columns.filter(c=>c.different).length}
  for(const pair of pairs){
    const row:RowPair={...pair,status:'same',changed:0,leftOnly:0,rightOnly:0}
    for(const column of columns){const status=compareCells(getCell(left,pair.left,column.left),getCell(right,pair.right,column.right),options.ignoreType);if(status!=='same'){row[status]++;result[status]++}}
    if(pair.left===null){row.status='rightOnly';result.rightRows++}
    else if(pair.right===null){row.status='leftOnly';result.leftRows++}
    else if(row.changed||row.leftOnly||row.rightOnly){row.status='changed';result.changedRows++}
    result.rows.push(row)
  }
  return result
}
