import {diffArrays} from 'diff'
import type {DocumentData,DocUnit} from '../contracts'
export interface DocumentRow {id:number;left:DocUnit|null;right:DocUnit|null;textStatus:'same'|'different'|'leftOnly'|'rightOnly'|'unknown';layoutStatus:'same'|'different'|'unknown'}
function row(id:number,left:DocUnit|null,right:DocUnit|null):DocumentRow{
  const textStatus=!left?'rightOnly':!right?'leftOnly':left.warning||right.warning?'unknown':left.text===right.text?'same':'different'
  const layoutStatus=!left?.layoutFingerprint||!right?.layoutFingerprint?'unknown':left.layoutFingerprint===right.layoutFingerprint?'same':'different'
  return {id,left,right,textStatus,layoutStatus}
}
export function compareDocuments(left:DocumentData,right:DocumentData):DocumentRow[]{
  const a=left.units,b=right.units,result:DocumentRow[]=[]
  if(left.format!=='DOCX'||right.format!=='DOCX'){
    for(let i=0;i<Math.max(a.length,b.length);i++)result.push(row(i,a[i]??null,b[i]??null))
    return result
  }
  // Align Word paragraphs by extracted text while keeping the layout/resource
  // summary as a fixed comparison unit. Changes to one paragraph do not shift
  // all later unchanged paragraphs.
  let ai=1,bi=1,removed=0,added=0
  if(a[0]||b[0])result.push(row(result.length,a[0]??null,b[0]??null))
  function flush(){for(let j=0;j<Math.max(removed,added);j++)result.push(row(result.length,j<removed?a[ai+j]!:null,j<added?b[bi+j]!:null));ai+=removed;bi+=added;removed=0;added=0}
  for(const part of diffArrays(a.slice(1).map(v=>v.text),b.slice(1).map(v=>v.text))){
    if(part.added)added+=part.count
    else if(part.removed)removed+=part.count
    else{flush();for(let j=0;j<part.count;j++)result.push(row(result.length,a[ai++]!,b[bi++]!))}
  }
  flush();return result
}
export function differs(r:DocumentRow):boolean{return r.textStatus!=='same'||r.layoutStatus!=='same'}
