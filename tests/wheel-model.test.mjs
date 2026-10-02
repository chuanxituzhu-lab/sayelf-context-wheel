// Unit tests for src/wheel-model.ts (pure logic). Run: node tests/wheel-model.test.mjs
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { build } from 'esbuild';

const out = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'cw-')), 'wheel-model.mjs');
await build({ entryPoints: ['src/wheel-model.ts'], bundle: true, format: 'esm', platform: 'node', outfile: out, logLevel: 'error' });
const m = await import(pathToFileURL(out).href);

// 1. Contrast: every theme keeps icons and selection readable (WCAG AA 4.5:1, selection 3:1 from idle).
for (const [id, t] of Object.entries(m.THEMES)) {
    const pairs = { 'icon/sector': [t.icon, t.sector], 'selected icon/selected': [t.selectedIcon, t.selected], 'label/sector': [t.label, t.sector], 'center/hub': [t.centerText, t.hub] };
    for (const [name, [a, b]] of Object.entries(pairs)) assert.ok(m.contrast(a, b) >= 4.5, `${id} ${name} ${m.contrast(a, b).toFixed(2)}:1`);
    assert.ok(m.contrast(t.selected, t.sector) >= 3, `${id} selected sector stands out from idle sectors`);
}
assert.ok(m.contrast('#d9efff', '#a9d2ef') < 1.5, 'regression reference: v0.1.1 ice_blue icons were 1.35:1');

// 2. Ring directions mirror core.rs ring_directions().
assert.deepEqual(m.ringDirections(0, 8), ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW']);
assert.equal(m.ringDirections(1, 16)[1], 'NNE');
assert.deepEqual(m.ringDirections(1, 3), ['P01', 'P02', 'P03']);

// 3. 16-slot ring: fill / clear one slot never moves other commands.
const ring = m.newOuterRing('acad.exe', [], 1, 16);
assert.equal(ring.length, 16);
assert.ok(ring.every(s => s.enabled !== false), 'new AutoCAD ring starts full');
const before = ring.map(s => `${s.direction}:${s.label}`);
m.clearSlot(ring[5]);
assert.equal(ring.length, 16);
assert.equal(ring[5].direction, before[5].split(':')[0], 'cleared slot keeps its position');
assert.ok(m.isEmptySlot(ring[5]));
ring.forEach((s, i) => { if (i !== 5) assert.equal(`${s.direction}:${s.label}`, before[i]); });
const polygon = m.catalog.flatMap(g => g.items).find(i => i.label === '多边形');
m.fillSlot(ring[5], m.sectorFromCatalog(polygon, 'acad.exe'));
assert.equal(ring[5].label, '多边形');
assert.equal(ring[5].icon, undefined, 'AutoCAD typed commands leave icon unset so native AutoCAD icons can show');
assert.equal(m.iconFor(ring[5], 'acad.exe'), 'polygon', 'fallback glyph comes from the library');

// 4. Fill empties uses commands not already on the wheel.
const sparse = m.ringDirections(1, 16).map(m.emptySector);
const inner = [{ direction: 'N', label: '推拉', action: { type: 'hotkey', keys: ['P'] } }];
const added = m.fillEmptySlots(sparse, 1, 'SketchUp.exe', inner);
assert.equal(added, 16);
assert.ok(!sparse.some(s => s.action.type === 'hotkey' && s.action.keys.join('+') === 'P'), 'no duplicate of inner-ring command');

// 5. Resize 8 <-> 16 keeps angles; 16 -> 8 reports dropped in-between commands.
const r16 = m.newOuterRing('acad.exe', [], 1, 16);
const { ring: r8, dropped } = m.resizeRing(r16, 8);
assert.equal(r8.length, 8);
assert.equal(dropped.length, 8);
assert.equal(r8[1].label, r16[2].label, 'NE stays NE');
const { ring: back16, dropped: none } = m.resizeRing(r8, 16);
assert.equal(back16.length, 16);
assert.equal(none.length, 0);
assert.ok(m.isEmptySlot(back16[1]) && back16[2].label === r16[2].label);

// 6. Icons for old configs without an icon field: by action in this app, then label.
assert.equal(m.iconFor({ direction: 'N', label: '推拉', action: { type: 'hotkey', keys: ['P'] } }, 'SketchUp.exe'), 'pushpull');
assert.equal(m.iconFor({ direction: 'N', label: '前视', action: { type: 'hotkey', keys: ['CTRL', '1'] } }, 'SLDWORKS.exe'), 'view_front');
assert.equal(m.iconFor({ direction: 'N', label: '特性', action: { type: 'hotkey', keys: ['CTRL', '1'] } }, 'acad.exe'), 'properties', 'same hotkey, other app, other icon');
assert.equal(m.iconFor({ direction: 'N', label: '直线', action: { type: 'keystroke', text: '_.LINE\n' } }, 'acad.exe'), 'line', 'legacy text without ^C^C');
assert.equal(m.commandName('^C^C_.ZOOM\n_E\n'), 'ZOOM');
assert.ok(Object.values(m.icons).every(i => /^[MmLlHhVvCcSsQqTtAaZz0-9 .,-]+$/.test(i.d)), 'icon paths are plain SVG path data');

console.log(`wheel-model: ${Object.keys(m.THEMES).length} themes pass contrast; 16-slot fill/clear/resize keep positions; icon matching ok`);
