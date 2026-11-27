import {type AssetReader,readAssetBlob} from './asset-io'
export interface Raster{width:number;height:number;data:Uint8ClampedArray}
export interface ImageStats{width:number;height:number;common:number;changed:number;leftOnly:number;rightOnly:number;maxDelta:number;heat:Uint8ClampedArray}
export function rasterMime(head:Uint8Array):string|undefined{
  const starts=(v:number[])=>v.every((n,i)=>head[i]===n)
  const ascii=(s:number,e:number)=>String.fromCharCode(...head.slice(s,e))
  if(starts([137,80,78,71,13,10,26,10]))return 'image/png'
  if(starts([255,216,255]))return 'image/jpeg'
  if(['GIF87a','GIF89a'].includes(ascii(0,6)))return 'image/gif'
  if(ascii(0,4)==='RIFF'&&ascii(8,12)==='WEBP')return 'image/webp'
  if(ascii(0,2)==='BM')return 'image/bmp'
  if(starts([0,0,1,0]))return 'image/x-icon'
  if(ascii(4,8)==='ftyp'&&['avif','avis'].includes(ascii(8,12)))return 'image/avif'
}
export async function decodeRaster(reader:AssetReader,active:()=>boolean):Promise<Raster>{
  const mime=rasterMime(await reader.read(0,32))
  if(!mime)throw new Error('Unsupported raster image. SVG and external image resources are not loaded.')
  const blob=new Blob([await readAssetBlob(reader,active)],{type:mime})
  if(!active())throw new Error('Comparison cancelled.')
  // ImageBitmap decodes a single raster frame, never external SVG resources.
  const bitmap=await createImageBitmap(blob)
  try{
    if(!active())throw new Error('Comparison cancelled.')
    const canvas=document.createElement('canvas');canvas.width=bitmap.width;canvas.height=bitmap.height
    const ctx=canvas.getContext('2d',{willReadFrequently:true})
    if(!ctx||!canvas.width||!canvas.height)throw new Error('Image canvas allocation failed.')
    ctx.drawImage(bitmap,0,0)
    return {width:canvas.width,height:canvas.height,data:ctx.getImageData(0,0,canvas.width,canvas.height).data}
  }finally{bitmap.close()}
}
export function compareRaster(left:Raster,right:Raster,tolerance:number):ImageStats{
  const width=Math.max(left.width,right.width),height=Math.max(left.height,right.height)
  const heat=new Uint8ClampedArray(width*height*4)
  let changed=0,common=0,leftOnly=0,rightOnly=0,maxDelta=0
  const threshold=Math.max(0,Math.min(255,tolerance))
  for(let y=0;y<height;y++)for(let x=0;x<width;x++){
    const a=x<left.width&&y<left.height,b=x<right.width&&y<right.height
    if(!a&&!b)continue
    const p=(y*width+x)*4
    if(!b){leftOnly++;heat[p]=227;heat[p+1]=73;heat[p+2]=95;heat[p+3]=255;continue}
    if(!a){rightOnly++;heat[p]=37;heat[p+1]=185;heat[p+2]=125;heat[p+3]=255;continue}
    common++
    const l=(y*left.width+x)*4,r=(y*right.width+x)*4
    const delta=Math.max(Math.abs(left.data[l]-right.data[r]),Math.abs(left.data[l+1]-right.data[r+1]),Math.abs(left.data[l+2]-right.data[r+2]),Math.abs(left.data[l+3]-right.data[r+3]))
    maxDelta=Math.max(maxDelta,delta)
    if(delta>threshold){changed++;heat[p]=255;heat[p+1]=Math.max(20,180-delta);heat[p+2]=25;heat[p+3]=255}
    else{const gray=Math.round((left.data[l]+left.data[l+1]+left.data[l+2])/3);heat[p]=gray;heat[p+1]=gray;heat[p+2]=gray;heat[p+3]=Math.min(left.data[l+3],100)}
  }
  return {width,height,common,changed,leftOnly,rightOnly,maxDelta,heat}
}
export function rasterCanvas(raster:Raster):HTMLCanvasElement{
  const canvas=document.createElement('canvas');canvas.width=raster.width;canvas.height=raster.height
  const ctx=canvas.getContext('2d');if(!ctx)throw new Error('Image canvas allocation failed.')
  ctx.putImageData(new ImageData(new Uint8ClampedArray(raster.data),raster.width,raster.height),0,0)
  return canvas
}
