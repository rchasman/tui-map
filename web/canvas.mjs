// Paint terminal cells without letting glyph ink leak into adjacent cells.
export function createCellPainter(ctx,{cols,rows,cw,ch,dpr}) {
let previous=null;
const colors=new Map();
function color(rgb){let css=colors.get(rgb);if(!css){css=`#${rgb.toString(16).padStart(6,'0')}`;if(colors.size>4096)colors.clear();colors.set(rgb,css);}return css;}
return function paint(cells){
  if(cells.length!==cols*rows*3){previous=null;return;}
  for(let i=0;i<cells.length;i+=3){
    if(previous&&cells[i]===previous[i]&&cells[i+1]===previous[i+1]&&cells[i+2]===previous[i+2])continue;
    const index=i/3,x=(index%cols)*cw,y=Math.floor(index/cols)*ch,code=cells[i];
    // Each physical pixel belongs to exactly one terminal cell. Integer bounds
    // avoid residual antialiasing at browser zoom / fractional device scales.
    const left=Math.round(x*dpr),top=Math.round(y*dpr);
    const right=Math.round((x+cw)*dpr),bottom=Math.round((y+ch)*dpr);
    ctx.save();
    ctx.setTransform(1,0,0,1,0,0);
    ctx.beginPath();ctx.rect(left,top,right-left,bottom-top);ctx.clip();
    ctx.fillStyle=color(cells[i+2]);ctx.fillRect(left,top,right-left,bottom-top);
    ctx.setTransform(dpr,0,0,dpr,0,0);
    if(code===32||code===0x2800){ctx.restore();continue;}
    ctx.fillStyle=color(cells[i+1]);
    if(code>=0x2800&&code<=0x28ff){
      const bits=code-0x2800,offsets=[[0,0],[0,1],[0,2],[1,0],[1,1],[1,2],[0,3],[1,3]];
      for(let bit=0;bit<8;bit++)if(bits&(1<<bit)){const [dx,dy]=offsets[bit];ctx.fillRect(x+cw*(dx*.5+.18),y+ch*(dy*.25+.08),Math.max(1,cw*.16),Math.max(1,ch*.085));}
    }else if(code===0x2588){ctx.fillRect(x,y,cw,ch);}
    else{
      // Keep map pictograms monochrome and cell-sized instead of emoji blobs.
      const textStyle=[0x2708,0x2601,0x2600,0x2744,0x26a0,0x2668].includes(code)?'\uFE0E':'';
      ctx.fillText(String.fromCodePoint(code)+textStyle,x,y+ch*.80,cw);
    }
    ctx.restore();
  }
  previous=cells;
}

}
