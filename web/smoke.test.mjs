import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import init, { BrowserApp } from './pkg/tui_map.js';

await init({ module_or_path: await readFile(new URL('./pkg/tui_map_bg.wasm', import.meta.url)) });

test('WASM host renders, fires all effects, changes projection, and resets', () => {
  const app = new BrowserApp(120, 40);
  try {
    assert.equal(JSON.parse(app.status()).weapon, 'CROSSHAIR');
    const geometry = { type: 'FeatureCollection', features: [{ type: 'Feature', properties: {},
      geometry: { type: 'LineString', coordinates: [[-30, 0], [0, 40], [30, 0]] } }] };
    app.load_layer('coastline', 0, new TextEncoder().encode(JSON.stringify(geometry)));
    app.finish_loading();
    assert.equal(app.render().length, 120 * 40 * 3);
    for (const [key, weapon] of [['1', 'WATER'], ['2', 'LIFE'], ['3', 'NUKE'], ['4', 'BIO'], ['5', 'EMP'], ['6', 'CHEM'], ['7', 'TORNADO'], ['8', 'FROST'], ['9', 'METEOR']]) {
      app.command(key);
      for (let frame = 0; frame < 16; frame++) app.tick();
      const before = JSON.parse(app.status()).effects;
      app.pointer('fire', 60, 19);
      const status = JSON.parse(app.status());
      assert.equal(status.weapon, weapon);
      assert.equal(status.effects, before + 1);
      const cells = app.render();
      assert.ok(cells.some((value, index) => index % 3 === 0 && value !== 32));
    }
    app.command('g');
    assert.equal(JSON.parse(app.status()).projection, 'Mercator');
    app.resize(55, 44);
    assert.equal(app.render().length, 55 * 44 * 3);
    app.command('r');
    const reset = JSON.parse(app.status());
    assert.equal(reset.weapon, 'CROSSHAIR');
    assert.equal(reset.effects, 0);
    assert.equal(reset.fires, 0);
    assert.equal(reset.casualties, 0);
    assert.equal(reset.projection, 'Globe');
  } finally { app.free(); }
});

test('WASM loader rejects malformed data and unknown layer kinds', () => {
  const app = new BrowserApp(40, 16);
  try {
    assert.throws(() => app.load_layer('coastline', 0, new TextEncoder().encode('invalid')));
    assert.throws(() => app.load_layer('unknown', 0, new TextEncoder().encode('{"type":"FeatureCollection","features":[]}')));
  } finally { app.free(); }
});

test('TUI menu clicks select effects without firing and Escape restores crosshair selection', () => {
  const app = new BrowserApp(55, 44);
  const click = text => {
    const cells = app.render();
    const chars = Array.from(cells).filter((_, i) => i % 3 === 0).map(c => String.fromCodePoint(c)).join('');
    const index = chars.indexOf(text);
    assert.ok(index >= 0, `Missing TUI control: ${text}`);
    const col = index % 55, row = Math.floor(index / 55);
    app.pointer('start', col, row);
    app.pointer('end', col, row);
    app.pointer('fire', col, row);
  };
  try {
    click('[1 Water]');
    assert.equal(JSON.parse(app.status()).weapon, 'WATER');
    click('[2 Life]');
    assert.equal(JSON.parse(app.status()).weapon, 'LIFE');
    click('[4 Bio]');
    assert.equal(JSON.parse(app.status()).weapon, 'BIO');
    for (const [label, weapon] of [['7 Tornado', 'TORNADO'], ['8 Frost', 'FROST'], ['9 Meteor', 'METEOR']]) {
      click(`[${label}]`);
      assert.equal(JSON.parse(app.status()).weapon, weapon);
    }
    assert.equal(JSON.parse(app.status()).effects, 0);
    click('[? Help]');
    assert.equal(JSON.parse(app.status()).help, true);
    app.command('Escape');
    assert.equal(JSON.parse(app.status()).help, false);
    assert.equal(JSON.parse(app.status()).weapon, 'CROSSHAIR');
    const frame=JSON.parse(app.status()).frame;
    app.tick();
    assert.equal(JSON.parse(app.status()).frame,frame+1);
  } finally { app.free(); }
});

test('live layers ingest, inspect, expire and survive reset without duplicate requests',()=>{
  const app=new BrowserApp(120,50);const now=1788800000;
  try {
    assert.deepEqual(JSON.parse(app.feed_requests(now)),[]);
    app.command('e');app.command('d');app.command('a');app.command('t');
    const requests=JSON.parse(app.feed_requests(now));assert.equal(requests.length,4);
    assert.deepEqual(JSON.parse(app.feed_requests(now+1)),[]);
    const quake=JSON.stringify({type:'FeatureCollection',features:[{id:'q',geometry:{type:'Point',coordinates:[0,20,10]},properties:{mag:4.5,place:'Test quake',time:now*1000}}]});
    for(const r of requests){
      if(r.kind==='quakes')app.feed_complete(r.id,quake,'',now);
      else if(r.kind==='hazards')app.feed_complete(r.id,'{"events":[]}','',now);
      else app.feed_complete(r.id,'','Test outage',now);
    }
    const status=JSON.parse(app.status());
    assert.equal(status.feeds[0].state,'LIVE');assert.equal(status.feeds[1].state,'EMPTY');assert.equal(status.feeds[2].state,'OFFLINE');
    app.command('i');app.render();
    const mapRows=JSON.parse(app.status()).mapAreaRows;
    app.pointer('fire',60,Math.floor(mapRows/2));
    assert.equal(JSON.parse(app.status()).effects,0);
    assert.ok(JSON.parse(app.status()).selected);
    assert.equal(app.render().length,120*50*3);
    app.command('r');assert.equal(JSON.parse(app.status()).feeds[0].count,1);
    assert.deepEqual(JSON.parse(app.feed_requests(now+2)),[]);
    app.command('e');assert.equal(JSON.parse(app.status()).feeds[0].state,'OFF');
    app.resize(40,16);assert.equal(app.render().length,40*16*3);
  } finally {app.free();}
});

test('crosshair is default, every live layer shares Labels, and details stay outside the map',()=>{
  const app=new BrowserApp(160,60),now=1788739200;
  const text=()=>Array.from(app.render()).filter((_,i)=>i%3===0).map(c=>String.fromCodePoint(c)).join('');
  try {
    app.finish_loading();app.command('g');app.pointer('fire',80,20);
    assert.equal(JSON.parse(app.status()).effects,0);
    assert.equal(JSON.parse(app.status()).weapon,'CROSSHAIR');
    for(const key of ['e','d','a','t'])app.command(key);
    const payloads={
      quakes:{type:'FeatureCollection',features:[{id:'quake-label',geometry:{type:'Point',coordinates:[-40,20,10]},properties:{mag:4.5,place:'QUAKE LABEL',time:now*1000}}]},
      hazards:{events:[{id:'hazard-label',title:'HAZARD LABEL',categories:[{title:'Wildfires'}],geometry:[{type:'Point',coordinates:[-10,-10],date:'2026-09-07T00:00:00Z'}]}]},
      aircraft:{ac:[{hex:'abcd',flight:'AIR LABEL',lat:30,lon:40,seen_pos:0}]},
      satellites:[{OBJECT_NAME:'SAT LABEL',OBJECT_ID:'1998-067A',EPOCH:'2026-09-07T00:00:00',MEAN_MOTION:15.5,ECCENTRICITY:0.0005,INCLINATION:0,RA_OF_ASC_NODE:0,ARG_OF_PERICENTER:0,MEAN_ANOMALY:0,EPHEMERIS_TYPE:0,CLASSIFICATION_TYPE:'U',NORAD_CAT_ID:25544,ELEMENT_SET_NO:999,REV_AT_EPOCH:100,BSTAR:0.0001,MEAN_MOTION_DOT:0,MEAN_MOTION_DDOT:0}]
    };
    for(const request of JSON.parse(app.feed_requests(now)))app.feed_complete(request.id,JSON.stringify(payloads[request.kind]),'',now);
    const before=text(),mapEnd=JSON.parse(app.status()).mapAreaRows*160;
    for(const label of ['QUAKE LABEL','HAZARD LABEL','AIR LABEL','SAT LABEL'])assert.ok(before.slice(0,mapEnd).includes(label),`missing ${label}`);
    assert.ok(!before.slice(0,mapEnd).includes('click marker'));
    app.command('L');
    for(const label of ['QUAKE LABEL','HAZARD LABEL','AIR LABEL','SAT LABEL'])assert.ok(!text().slice(0,mapEnd).includes(label));
    app.command('L');
    const index=text().indexOf('QUAKE LABEL');app.pointer('fire',index%160,Math.floor(index/160));
    assert.deepEqual(JSON.parse(app.status()).selected,['quakes','quake-label']);
    const rendered=text();
    assert.ok(!rendered.includes('Magnitude 4.5'));
    assert.match(JSON.parse(app.status()).details.detail,/Magnitude 4.5/);
    assert.equal(JSON.parse(app.status()).details.label,'M4.5 QUAKE LABEL');
    assert.equal(JSON.parse(app.status()).effects,0);
    app.command('1');assert.equal(JSON.parse(app.status()).inspect,false);
    app.command('Escape');assert.equal(JSON.parse(app.status()).weapon,'CROSSHAIR');
    app.pointer('fire',80,20);assert.equal(JSON.parse(app.status()).effects,0);
    app.command('i');assert.equal(JSON.parse(app.status()).weapon,'CROSSHAIR');
    app.command('i');assert.equal(JSON.parse(app.status()).inspect,true);
  } finally {app.free();}
});


test('TUI layer picker preserves the header, stacks toggles and consumes map input', () => {
  for (const [width,height] of [[120,40],[40,16]]) {
    const app=new BrowserApp(width,height);
    const lines=()=>{const cells=app.render();return Array.from({length:height},(_,row)=>
      Array.from({length:width},(_,col)=>String.fromCodePoint(cells[(row*width+col)*3])).join(''));};
    try {
      assert.match(lines()[0],/World Map/);
      assert.match(lines()[1],/Layers/);
      assert.doesNotMatch(lines()[1],/Borders/);
      assert.match(lines()[2],/Borders/);
      assert.match(lines()[3],/States/);
      const cells=app.render();
      assert.notEqual(cells[(2*width+4)*3+1],cells[(3*width+4)*3+1]);
      app.command('3');
      app.pointer('start',3,1);app.pointer('end',3,1);app.pointer('fire',3,1);
      assert.equal(JSON.parse(app.status()).layersOpen,true);
      assert.match(lines()[2],/b Borders/);
      assert.match(lines()[3],/s States/);
      app.pointer('fire',3,2);
      assert.equal(JSON.parse(app.status()).mapLayers.b,false);
      assert.equal(JSON.parse(app.status()).effects,0);
      if(height===16){
        for(let i=0;i<10;i++)app.pointer('out',3,2);
        assert.ok(lines().some(line=>line.includes('p Population')));
      }else{
        assert.match(lines()[11],/p Population/);
      }
      app.command('Escape');
      assert.equal(JSON.parse(app.status()).layersOpen,false);
      assert.equal(JSON.parse(app.status()).weapon,'CROSSHAIR');
    } finally {app.free();}
  }
});


test('compact controls keep navigation clickable and Help after effects', () => {
  for (const width of [40, 55, 120, 240]) {
    const height = 44, app = new BrowserApp(width, height);
    const lines = () => {
      const cells = app.render();
      return Array.from({ length: height }, (_, row) => Array.from({ length: width },
        (_, col) => String.fromCodePoint(cells[(row * width + col) * 3])).join(''));
    };
    const click = label => {
      const rendered = lines(), row = rendered.findIndex(line => line.includes(label));
      assert.ok(row >= 0, `Missing control: ${label}`);
      const col = rendered[row].indexOf(label);
      app.pointer('start', col, row);
      app.pointer('end', col, row);
      app.pointer('fire', col, row);
    };
    try {
      const rendered = lines();
      assert.ok(!rendered.join('').includes('Globe/map'));
      const navRow = JSON.parse(app.status()).mapAreaRows;
      assert.match(rendered[navRow], /\[G\]lobe \[-\] \[\+\] \[r Reset\]/);
      if (width >= 120) {
        assert.match(rendered[height - 2], /\[9 Meteor\] \[\? Help\]/);
        assert.match(rendered[height - 3], /Effects/);
      }
      const zoom = JSON.parse(app.status()).zoom;
      click('[+]');
      assert.notEqual(JSON.parse(app.status()).zoom, zoom);
      click('[-]');
      assert.equal(JSON.parse(app.status()).zoom, zoom);
      click('[G]lobe');
      assert.equal(JSON.parse(app.status()).projection, 'Mercator');
      click('[M]ap');
      assert.equal(JSON.parse(app.status()).projection, 'Globe');
      click('[1 Water]');
      click('[r Reset]');
      assert.equal(JSON.parse(app.status()).weapon, 'CROSSHAIR');
      assert.equal(JSON.parse(app.status()).effects, 0);
      click('[? Help]');
      assert.equal(JSON.parse(app.status()).help, true);
    } finally { app.free(); }
  }
});
