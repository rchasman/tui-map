import test from 'node:test';
import assert from 'node:assert/strict';
import {createFeedTransport} from './feeds.mjs';
import aircraft from './api/aircraft.js';
const settle=()=>new Promise(resolve=>setImmediate(resolve));

test('browser transport routes aircraft through same-origin proxy and reports failures independently',async()=>{
  let requests=[{id:1,kind:'aircraft',url:'https://api.adsb.lol/v2/lat/-34/lon/151/dist/250'},{id:2,kind:'quakes',url:'https://earthquake.usgs.gov/test'}];
  const completed=[],urls=[];
  const engine={status:()=>JSON.stringify({feeds:[{kind:'aircraft',enabled:true},{kind:'quakes',enabled:true}]}),feed_requests:()=>JSON.stringify(requests.splice(0)),feed_complete:(...args)=>completed.push(args)};
  const transport=createFeedTransport(engine,{clock:()=>100,fetcher:async url=>{urls.push(url);return url.startsWith('/api/')?new Response('{"ac":[]}'):new Response('',{status:503});}});
  transport.tick();await settle();
  assert.equal(urls[0],'/api/aircraft?lat=-34&lon=151');
  assert.deepEqual(completed.find(r=>r[0]===1),[1,'{"ac":[]}','',100]);
  assert.equal(completed.find(r=>r[0]===2)[2],'HTTP 503');
});

test('disabling a browser layer aborts its in-flight request',async()=>{
  let enabled=true,once=true;const completed=[];
  const engine={status:()=>JSON.stringify({feeds:[{kind:'quakes',enabled}]}),feed_requests:()=>JSON.stringify(once?(once=false,[{id:1,kind:'quakes',url:'https://example.test'}]):[]),feed_complete:(...args)=>completed.push(args)};
  const transport=createFeedTransport(engine,{fetcher:(_url,{signal})=>new Promise((_resolve,reject)=>signal.addEventListener('abort',()=>reject(new DOMException('Aborted','AbortError'))))});
  transport.tick();enabled=false;transport.tick();await settle();
  assert.match(completed[0][2],/cancelled/);
});

function response(){return {statusCode:200,headers:{},setHeader(k,v){this.headers[k]=v;},end(body){this.body=body;}};}
test('aircraft proxy validates input, bounds upstream and coalesces repeated requests',async()=>{
  const original=globalThis.fetch;const calls=[];
  globalThis.fetch=async url=>{calls.push(url);return new Response('{"ac":[]}');};
  try {
    for(const url of ['/api/aircraft','/api/aircraft?lat=91&lon=0','/api/aircraft?lat=0&lon=https://evil.test','/api/aircraft?lat=0.5&lon=0']){
      const res=response();await aircraft({url,method:'GET'},res);assert.equal(res.statusCode,400);
    }
    assert.equal(calls.length,0);
    const a=response(),b=response();
    await Promise.all([aircraft({url:'/api/aircraft?lat=-34&lon=151',method:'GET'},a),aircraft({url:'/api/aircraft?lat=-34&lon=151',method:'GET'},b)]);
    assert.deepEqual(calls,['https://api.adsb.lol/v2/lat/-34/lon/151/dist/250']);
    assert.equal(a.body,'{"ac":[]}');assert.equal(a.body,b.body);
    globalThis.fetch=async()=>new Response('',{status:429});
    const fail=response();await aircraft({url:'/api/aircraft?lat=1&lon=2',method:'GET'},fail);
    assert.equal(fail.statusCode,502);assert.equal(fail.headers['Cache-Control'],'no-store');
  } finally {globalThis.fetch=original;}
});
