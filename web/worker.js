import init, { BrowserApp } from './pkg/tui_map.js';

let engine, ready=false, paused=false, last=0, accumulator=0;
const STEP=1000/30;

async function loadGroup(layers, label) {
  for(let i=0;i<layers.length;i++){
    const layer=layers[i];
    postMessage({type:'loading',message:`${label} · ${i+1}/${layers.length}`,ready});
    const response=await fetch(new URL(layer.file,import.meta.url));
    if(!response.ok) throw Error(`Map data could not be loaded (${response.status}). Please retry.`);
    engine.load_layer(layer.kind,layer.lod,new Uint8Array(await response.arrayBuffer()));
    // Yield between layers so pending input and frame requests can run.
    await new Promise(resolve=>setTimeout(resolve,0));
  }
  engine.finish_loading();
}

self.onmessage=async ({data})=>{
  try {
    if(data.type==='init'){
      await init();
      engine=new BrowserApp(data.cols,data.rows);
      const response=await fetch(new URL('./data/manifest.json',import.meta.url));
      if(!response.ok) throw Error('The map manifest could not be loaded. Please retry.');
      const manifest=await response.json();
      await loadGroup(manifest.filter(l=>l.tier==='base'),'Drawing coastlines and cities');
      ready=true;postMessage({type:'ready'});
      try{
        await loadGroup(manifest.filter(l=>l.tier==='detail'),'Adding geographic detail');
        postMessage({type:'detail-ready'});
      }catch(error){postMessage({type:'detail-error',message:error.message});}
      return;
    }
    if(!engine)return;
    if(data.type==='resize'){engine.resize(data.cols,data.rows);return;}
    if(data.type==='pause'){paused=data.paused;last=0;accumulator=0;return;}
    if(data.type==='command'){engine.command(data.key);return;}
    if(data.type==='pointer'){engine.pointer(data.kind,data.col,data.row);return;}
    if(data.type==='frame' && ready){
      if(last)accumulator+=Math.min(data.now-last,100);
      last=data.now;
      if(!paused){while(accumulator>=STEP){engine.tick();accumulator-=STEP;}}
      else accumulator=0;
      const start=performance.now();
      const cells=engine.render();
      const status=JSON.parse(engine.status());
      postMessage({type:'frame',cells,status,renderMs:performance.now()-start},[cells.buffer]);
    }
  }catch(error){postMessage({type:'error',message:error.message||String(error)});}
};
