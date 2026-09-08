import { mkdir, readFile, writeFile, copyFile, cp } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const dist = path.join(root, 'web/dist');
await mkdir(path.join(dist, 'data'), { recursive: true });
for (const file of ['index.html', 'app.js', 'worker.js', 'style.css', 'favicon.svg', 'vercel.json', '.vercelignore']) {
  await copyFile(path.join(root, 'web', file), path.join(dist, file));
}
await cp(path.join(root, 'web/pkg'), path.join(dist, 'pkg'), { recursive: true });

// Package only public Natural Earth geography, never local GADM files or caches.
const layers = [
  ['ne_110m_coastline', 'ne_110m_coastline', 'coastline', 0, 'base'],
  ['ne_110m_land', 'ne_110m_land', 'land', 0, 'base'],
  ['ne_10m_cities', 'ne_10m_populated_places', 'cities', 2, 'base'],
  ['natural-earth', 'ne_50m_coastline', 'coastline', 1, 'detail'],
  ['ne_50m_borders', 'ne_50m_admin_0_boundary_lines_land', 'borders', 1, 'detail'],
  ['ne_50m_land', 'ne_50m_land', 'land', 1, 'detail'],
  ['ne_10m_coastline', 'ne_10m_coastline', 'coastline', 2, 'detail'],
  ['ne_10m_borders', 'ne_10m_admin_0_boundary_lines_land', 'borders', 2, 'detail'],
  ['ne_10m_land', 'ne_10m_land', 'land', 2, 'detail'],
  ['ne_10m_states', 'ne_10m_admin_1_states_provinces_lines', 'states', 2, 'detail'],
  ['ne_10m_admin_2_counties', 'ne_10m_admin_2_counties', 'counties', 2, 'detail'],
];
const manifest = [];
const cityKeys = new Set(['name', 'pop_max', 'pop_min', 'population', 'adm0cap', 'megacity']);
const round = value => Array.isArray(value) ? value.map(round) : Math.round(value * 1e5) / 1e5;
for (const [local, upstream, kind, lod, tier] of layers) {
  let source;
  try { source = await readFile(path.join(root, 'data', `${local}.json`), 'utf8'); }
  catch (error) {
    if (error.code !== 'ENOENT') throw error;
    const response = await fetch(`https://raw.githubusercontent.com/nvkelso/natural-earth-vector/v5.1.2/geojson/${upstream}.geojson`);
    if (!response.ok) throw Error(`Download failed for ${upstream}: ${response.status}`);
    source = await response.text();
  }
  const geo = JSON.parse(source);
  const compact = { type: 'FeatureCollection', features: geo.features.map(feature => ({
    type: 'Feature',
    properties: kind === 'cities' ? Object.fromEntries(Object.entries(feature.properties || {})
      .filter(([key]) => cityKeys.has(key.toLowerCase())).map(([key, value]) => [key.toLowerCase(), value])) : {},
    geometry: feature.geometry && { type: feature.geometry.type, coordinates: round(feature.geometry.coordinates) },
  })) };
  const file = `./data/${local}.json`;
  const json = JSON.stringify(compact);
  await writeFile(path.join(dist, file), json);
  manifest.push({ file, kind, lod, tier });
  console.log(`${local}: ${(Buffer.byteLength(json)/1e6).toFixed(2)} MB`);
}
await writeFile(path.join(dist, 'data/manifest.json'), JSON.stringify(manifest));
console.log('Browser bundle ready in web/dist');
