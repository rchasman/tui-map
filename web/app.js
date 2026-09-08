const canvas=document.querySelector('#world'),stage=document.querySelector('#stage');
const ctx=canvas.getContext('2d',{alpha:false});
const loading=document.querySelector('#loading');
const worker=new Worker(new URL('./worker.js',import.meta.url),{type:'module'});
let cols=0,rows=0,cw=8,ch=16,dpr=1,previous=null,ready=false,inFlight=false;
let gesture=null,lastPointer=null,pointerMove=null;
let lastRequest=0;

function send(message){worker.postMessage(message);}
function resize(){
  const bounds=stage.getBoundingClientRect();
  cw=window.innerWidth<580?7:8;ch=cw*2;
  cols=Math.max(40,Math.min(240,Math.floor(bounds.width/cw)));
  rows=Math.max(16,Math.min(100,Math.floor(bounds.height/ch)));
  // Scale the whole grid to fit unusually small windows without clipping controls.
  const fit=Math.min(1,bounds.width/(cols*cw),bounds.height/(rows*ch));
  const width=cols*cw,height=rows*ch;dpr=Math.min(devicePixelRatio||1,2);
  canvas.width=Math.round(width*dpr);canvas.height=Math.round(height*dpr);
  canvas.style.width=`${width*fit}px`;canvas.style.height=`${height*fit}px`;
  canvas.style.left=`${(bounds.width-width*fit)/2}px`;canvas.style.top=`${(bounds.height-height*fit)/2}px`;
  ctx.setTransform(dpr,0,0,dpr,0,0);ctx.font=`${ch-2}px monospace`;
  ctx.textBaseline='alphabetic';ctx.fillStyle='#080e16';ctx.fillRect(0,0,width,height);
  previous=null;
  if(ready)send({type:'resize',cols,rows});
}

const colors=new Map();
function color(rgb){let css=colors.get(rgb);if(!css){css=`#${rgb.toString(16).padStart(6,'0')}`;if(colors.size>4096)colors.clear();colors.set(rgb,css);}return css;}
function paint(cells){
  if(cells.length!==cols*rows*3){previous=null;return;}
  for(let i=0;i<cells.length;i+=3){
    if(previous&&cells[i]===previous[i]&&cells[i+1]===previous[i+1]&&cells[i+2]===previous[i+2])continue;
    const index=i/3,x=(index%cols)*cw,y=Math.floor(index/cols)*ch,code=cells[i];
    ctx.fillStyle=color(cells[i+2]);ctx.fillRect(x,y,cw,ch);
    if(code===32||code===0x2800)continue;
    ctx.fillStyle=color(cells[i+1]);
    if(code>=0x2800&&code<=0x28ff){
      const bits=code-0x2800,offsets=[[0,0],[0,1],[0,2],[1,0],[1,1],[1,2],[0,3],[1,3]];
      for(let bit=0;bit<8;bit++)if(bits&(1<<bit)){const [dx,dy]=offsets[bit];ctx.fillRect(x+cw*(dx*.5+.18),y+ch*(dy*.25+.08),Math.max(1,cw*.16),Math.max(1,ch*.085));}
    }else if(code===0x2588){ctx.fillRect(x,y,cw,ch);}
    else{ctx.fillText(String.fromCodePoint(code),x,y+ch*.80,cw);}
  }
  previous=cells;
}

function updateStatus(next){
  canvas.dataset.feeds=JSON.stringify(next.feeds);
  canvas.dataset.inspect=String(next.inspect);
  canvas.dataset.selected=JSON.stringify(next.selected);
  for(const key of ['frame','effects','projection','weapon','paused','help','mapRows'])canvas.dataset[key]=String(next[key]);
}
function fail(message){inFlight=false;ready=false;loading.hidden=true;const error=document.querySelector('#error');error.hidden=false;error.textContent=message+' Reload the page to retry.';}
worker.onerror=event=>fail(event.message||'The simulation stopped unexpectedly.');
worker.onmessage=({data})=>{
  if(data.type==='loading')loading.textContent=data.message;
  if(data.type==='ready'){ready=true;loading.hidden=true;send({type:'resize',cols,rows});canvas.focus({preventScroll:true});}
  if(data.type==='detail-ready')canvas.dataset.detail='ready';
  if(data.type==='detail-error')canvas.dataset.detail='unavailable';
  if(data.type==='error')fail(data.message);
  if(data.type==='frame'){inFlight=false;paint(data.cells);updateStatus(data.status);}
};

function frame(now){
  if(ready&&!inFlight&&!document.hidden&&now-lastRequest>=1000/30){
    if(pointerMove){send(pointerMove);pointerMove=null;}
    inFlight=true;lastRequest=now;send({type:'frame',now});
  }
  requestAnimationFrame(frame);
}

function position(event){const b=canvas.getBoundingClientRect();return {col:Math.max(0,Math.min(cols-1,Math.floor((event.clientX-b.left)/b.width*cols))),row:Math.max(0,Math.min(rows-1,Math.floor((event.clientY-b.top)/b.height*rows)))};}
function pointer(kind,pos){send({type:'pointer',kind,...pos});}
canvas.addEventListener('pointerdown',event=>{
  if(!ready||event.button>0)return;
  canvas.focus({preventScroll:true});canvas.setPointerCapture(event.pointerId);
  gesture={id:event.pointerId,x:event.clientX,y:event.clientY,dragging:false};
  lastPointer=position(event);pointer('start',lastPointer);
});
canvas.addEventListener('pointermove',event=>{
  lastPointer=position(event);
  if(gesture&&gesture.id===event.pointerId){
    if(Math.hypot(event.clientX-gesture.x,event.clientY-gesture.y)>4)gesture.dragging=true;
    canvas.classList.toggle('dragging',gesture.dragging);
    pointerMove={type:'pointer',kind:gesture.dragging?'drag':'move',...lastPointer};
  }else pointerMove={type:'pointer',kind:'move',...lastPointer};
});
canvas.addEventListener('pointerup',event=>{
  if(!gesture||gesture.id!==event.pointerId)return;
  if(pointerMove){send(pointerMove);pointerMove=null;}
  pointer('end',position(event));if(!gesture.dragging)pointer('fire',position(event));
  gesture=null;canvas.classList.remove('dragging');
});
canvas.addEventListener('pointercancel',()=>{gesture=null;pointerMove=null;canvas.classList.remove('dragging');pointer('end',lastPointer||{col:0,row:0});});
canvas.addEventListener('pointerleave',()=>{if(!gesture){pointerMove=null;pointer('leave',lastPointer||{col:0,row:0});}});
canvas.addEventListener('contextmenu',event=>event.preventDefault());
let lastWheel=0;
canvas.addEventListener('wheel',event=>{event.preventDefault();if(performance.now()-lastWheel<45)return;lastWheel=performance.now();pointer(event.deltaY<0?'in':'out',position(event));},{passive:false});

function command(key){if(ready)send({type:'command',key});}
document.addEventListener('keydown',event=>{
  if(event.ctrlKey||event.metaKey||event.altKey)return;
  if(['1','2','3','4','5','6','7','8','9','t','i','g','b','s','c','y','L','p','h','j','k','l','r','0',' ','+','=','-','ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Escape','?'].includes(event.key)){
    event.preventDefault();command(event.key);
  }
});
document.addEventListener('visibilitychange',()=>send({type:'pause',paused:document.hidden}));
new ResizeObserver(resize).observe(stage);
resize();send({type:'init',cols,rows});requestAnimationFrame(frame);
