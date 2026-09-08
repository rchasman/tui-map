// Network transport only. Rust owns parsing, polling deadlines and stale state.
export function createFeedTransport(engine, {fetcher=fetch, clock=()=>Date.now()/1000}={}) {
  const pending=new Map();
  function tick() {
    const status=JSON.parse(engine.status());
    for (const [id,request] of pending) {
      if (!status.feeds.find(layer=>layer.kind===request.kind)?.enabled) request.controller.abort();
    }
    for (const request of JSON.parse(engine.feed_requests(clock()))) {
      const controller=new AbortController();
      pending.set(request.id,{kind:request.kind,controller});
      const timeout=setTimeout(()=>controller.abort(),15000);
      const match=request.kind==='aircraft' && request.url.match(/\/lat\/(-?\d+)\/lon\/(-?\d+)\/dist\/250$/);
      const url=match ? `/api/aircraft?lat=${match[1]}&lon=${match[2]}` : request.url;
      (async()=>{
        try {
          const response=await fetcher(url,{signal:controller.signal});
          if (!response.ok) throw Error(`HTTP ${response.status}`);
          const reader=response.body.getReader();
          const chunks=[]; let size=0;
          while (true) {
            const {done,value}=await reader.read();
            if(done)break;
            size+=value.byteLength;
            if(size>8*1024*1024) {await reader.cancel();throw Error('Feed exceeds 8 MB limit');}
            chunks.push(value);
          }
          const bytes=new Uint8Array(size);let offset=0;
          for(const chunk of chunks){bytes.set(chunk,offset);offset+=chunk.length;}
          engine.feed_complete(request.id,new TextDecoder().decode(bytes),'',clock());
        } catch(error) {
          engine.feed_complete(request.id,'',error.name==='AbortError'?'Request cancelled or timed out':error.message,clock());
        } finally {clearTimeout(timeout);pending.delete(request.id);}
      })();
    }
  }
  return {tick};
}
