import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import init, { BrowserApp } from './pkg/tui_map.js';

await init({ module_or_path: await readFile(new URL('./pkg/tui_map_bg.wasm', import.meta.url)) });

test('WASM host renders, fires all effects, changes projection, and resets', () => {
  const app = new BrowserApp(120, 40);
  try {
    assert.equal(JSON.parse(app.status()).weapon, 'WATER');
    const geometry = { type: 'FeatureCollection', features: [{ type: 'Feature', properties: {},
      geometry: { type: 'LineString', coordinates: [[-30, 0], [0, 40], [30, 0]] } }] };
    app.load_layer('coastline', 0, new TextEncoder().encode(JSON.stringify(geometry)));
    app.finish_loading();
    assert.equal(app.render().length, 120 * 40 * 3);
    for (const [key, weapon] of [['1', 'WATER'], ['2', 'LIFE'], ['3', 'NUKE'], ['4', 'BIO'], ['5', 'EMP'], ['6', 'CHEM'], ['o', 'TORNADO'], ['f', 'FROST'], ['m', 'METEOR']]) {
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
    assert.equal(reset.weapon, 'WATER');
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

test('TUI menu clicks select effects without firing and control pause and help', () => {
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
    for (const [label, weapon] of [['o Tornado', 'TORNADO'], ['f Frost', 'FROST'], ['m Meteor', 'METEOR']]) {
      click(`[${label}]`);
      assert.equal(JSON.parse(app.status()).weapon, weapon);
    }
    assert.equal(JSON.parse(app.status()).effects, 0);
    click('[Esc Pause]');
    const frame = JSON.parse(app.status()).frame;
    app.tick();
    assert.equal(JSON.parse(app.status()).frame, frame);
    click('[? Help]');
    assert.equal(JSON.parse(app.status()).help, true);
    app.command('Escape');
    assert.equal(JSON.parse(app.status()).help, false);
    assert.equal(JSON.parse(app.status()).paused, true);
    app.command('Escape');
    app.tick();
    assert.equal(JSON.parse(app.status()).frame, frame + 1);
  } finally { app.free(); }
});

test('live layers ingest, inspect, expire and survive reset without duplicate requests',()=>{
  const app=new BrowserApp(120,50);const now=1788800000;
  try {
    assert.deepEqual(JSON.parse(app.feed_requests(now)),[]);
    app.command('7');app.command('8');app.command('9');app.command('t');
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
    const mapRows=JSON.parse(app.status()).mapRows;
    app.pointer('fire',60,Math.floor((mapRows-1)/2));
    assert.equal(JSON.parse(app.status()).effects,0);
    assert.ok(JSON.parse(app.status()).selected);
    assert.equal(app.render().length,120*50*3);
    app.command('r');assert.equal(JSON.parse(app.status()).feeds[0].count,1);
    assert.deepEqual(JSON.parse(app.feed_requests(now+2)),[]);
    app.command('7');assert.equal(JSON.parse(app.status()).feeds[0].state,'OFF');
    app.resize(40,16);assert.equal(app.render().length,40*16*3);
  } finally {app.free();}
});
