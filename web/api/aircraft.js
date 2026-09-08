// Fixed upstream, fixed 250 nautical mile radius; never accept a caller URL.
// Shared by Vercel's Node function and the local development server.
const cache=new Map();
const pending=new Map();
async function handler(req,res) {
  res.setHeader('Content-Type','application/json');
  if(req.method!=='GET') {res.statusCode=405;res.setHeader('Allow','GET');res.end('{"error":"GET required"}');return;}
  const url=new URL(req.url,'http://localhost');
  const latText=url.searchParams.get('lat'),lonText=url.searchParams.get('lon');
  const lat=Number(latText),lon=Number(lonText);
  if(!/^-?\d{1,3}$/.test(latText??'') || !/^-?\d{1,3}$/.test(lonText??'') || Math.abs(lat)>90 || Math.abs(lon)>180) {
    res.statusCode=400;res.end('{"error":"Integer lat [-90,90] and lon [-180,180] required"}');return;
  }
  const key=`${lat},${lon}`;
  try {
    let entry=cache.get(key);
    if(!entry || Date.now()-entry.time>=15000) {
      if(!pending.has(key)) {
        pending.set(key,(async()=>{
          const upstream=await fetch(`https://api.adsb.lol/v2/lat/${lat}/lon/${lon}/dist/250`,{headers:{'User-Agent':'tui-map/0.1 (+https://github.com/rchasman/tui-map)'},signal:AbortSignal.timeout(12000),redirect:'error'});
          if(!upstream.ok) throw Error(`Upstream HTTP ${upstream.status}`);
          const reader=upstream.body.getReader();const chunks=[];let size=0;
          while(true){const {done,value}=await reader.read();if(done)break;size+=value.byteLength;if(size>8*1024*1024){await reader.cancel();throw Error('Response too large');}chunks.push(value);}
          const body=Buffer.concat(chunks).toString('utf8');
          if(!Array.isArray(JSON.parse(body).ac)) throw Error('Invalid aircraft response');
          const fresh={body,time:Date.now()};
          if(cache.size>=128)cache.delete(cache.keys().next().value);
          cache.set(key,fresh);return fresh;
        })().finally(()=>pending.delete(key)));
      }
      entry=await pending.get(key);
    }
    res.setHeader('Cache-Control','public, max-age=0, s-maxage=15');
    res.end(entry.body);
  } catch {
    res.statusCode=502;res.setHeader('Cache-Control','no-store');
    res.end('{"error":"Aircraft source unavailable"}');
  }
}
module.exports=handler;
