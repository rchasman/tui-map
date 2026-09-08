import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import aircraft from './api/aircraft.js';
const root=fileURLToPath(new URL('./dist/',import.meta.url));
const types={'.html':'text/html','.js':'text/javascript','.mjs':'text/javascript','.css':'text/css','.wasm':'application/wasm','.json':'application/json','.svg':'image/svg+xml'};
createServer(async(req,res)=>{
  const url=new URL(req.url,'http://localhost');
  if(url.pathname==='/api/aircraft'){await aircraft(req,res);return;}
  let decoded;
  try {decoded=decodeURIComponent(url.pathname);} catch {res.writeHead(400).end('Invalid path');return;}
  const name=decoded==='/'?'index.html':decoded.replace(/^\//,'');
  const file=path.resolve(root,name);
  if(!file.startsWith(root) || name.startsWith('api/')){res.writeHead(404).end();return;}
  try {const data=await readFile(file);res.setHeader('Content-Type',types[path.extname(file)]||'application/octet-stream');res.end(data);}
  catch {res.writeHead(404).end('Not found');}
}).listen(4173,'127.0.0.1',()=>console.log('tui-map: http://127.0.0.1:4173'));
