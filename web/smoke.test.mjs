import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import init, { BrowserApp } from './pkg/tui_map.js';

await init({ module_or_path: await readFile(new URL('./pkg/tui_map_bg.wasm', import.meta.url)) });

test('WASM host renders, fires all effects, changes projection, and resets', () => {
  const app = new BrowserApp(120, 40);
  try {
    const geometry = { type: 'FeatureCollection', features: [{ type: 'Feature', properties: {},
      geometry: { type: 'LineString', coordinates: [[-30, 0], [0, 40], [30, 0]] } }] };
    app.load_layer('coastline', 0, new TextEncoder().encode(JSON.stringify(geometry)));
    app.finish_loading();
    assert.equal(app.render().length, 120 * 40 * 3);
    for (const [i, weapon] of ['WATER', 'LIFE', 'NUKE', 'BIO', 'EMP', 'CHEM'].entries()) {
      app.command(String(i + 1));
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
