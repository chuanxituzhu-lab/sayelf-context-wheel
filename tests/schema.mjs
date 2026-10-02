import fs from 'node:fs';
import assert from 'node:assert/strict';
import YAML from 'yaml';
import Ajv from 'ajv';
const config=YAML.parse(fs.readFileSync('src-tauri/profiles/default.yaml','utf8'));
const schema=JSON.parse(fs.readFileSync('src-tauri/profiles/schema.json','utf8'));
const icons=JSON.parse(fs.readFileSync('src/icons.json','utf8'));
const catalog=JSON.parse(fs.readFileSync('src/command-catalog.json','utf8'));
const check=new Ajv({strict:false}).compile(schema);
const D8=['N','NE','E','SE','S','SW','W','NW'];
const D16=['N','NNE','NE','ENE','E','ESE','SE','SSE','S','SSW','SW','WSW','W','WNW','NW','NNW'];
const dirsFor=(i,n)=>i===0||n===8?D8:n===16?D16:Array.from({length:n},(_,k)=>`P${String(k+1).padStart(2,'0')}`);
assert.equal(check(config),true,JSON.stringify(check.errors));
assert.deepEqual(schema.definitions.sector.properties.icon.enum,Object.keys(icons),'schema icon enum matches src/icons.json');
for(const p of config.profiles){
  const rings=[p.wheel,...(p.outer_rings??[])];
  assert.ok(rings.length<=3,`${p.id} has at most three rings`);
  assert.equal(rings[0].length,8,`${p.id} inner ring has eight positions`);
  for(const [index,ring] of rings.entries()){
    assert.deepEqual([...ring.map(s=>s.direction)].sort(),[...dirsFor(index,ring.length)].sort(),`${p.id} ring ${index+1} directions`);
    if(index>0)assert.equal(ring.length,16,`${p.id} default outer rings are full 16-slot rings`);
    for(const s of ring)if(s.enabled!==false)assert.ok(s.label.trim(),`${p.id} enabled slot has a label`);
  }
}
// Command library: every entry has a known icon and an executable action.
const keyOk=k=>/^(CTRL|SHIFT|ALT|WIN|ESC|ENTER|TAB|SPACE|BACKSPACE|DELETE|INSERT|HOME|END|PGUP|PGDN|LEFT|UP|RIGHT|DOWN|[A-Z0-9]|F([1-9]|1[0-9]|2[0-4]))$/.test(k);
let commands=0;
for(const g of catalog){
  assert.ok(g.id&&g.name&&g.apps.length&&g.items.length,`group ${g.id}`);
  for(const it of g.items){
    commands++;
    assert.ok(icons[it.icon],`${g.id}/${it.label} icon ${it.icon}`);
    const a=it.action;
    if(a.type==='hotkey')assert.ok(a.keys.length&&a.keys.every(keyOk),`${g.id}/${it.label} hotkey ${a.keys}`);
    else if(a.type==='keystroke')assert.ok(a.text.length,`${g.id}/${it.label} text`);
    else if(a.type==='launch')assert.ok(a.program,`${g.id}/${it.label} program`);
    else assert.fail(`${g.id}/${it.label} unsupported action ${a.type}`);
    if(g.apps.includes('acad.exe')&&a.type==='keystroke')assert.match(a.text,/^\^C\^C_\./,`${it.label} cancels running AutoCAD command`);
  }
}
// Variants that must pass / fail.
const acad=c=>c.profiles.find(p=>p.id==='autocad.default');
const acadOuter=acad(config).outer_rings[0];
assert.equal(acadOuter.length,16,'AutoCAD second ring has 16 positions');
assert.ok(acadOuter.every(s=>s.enabled!==false&&s.icon&&icons[s.icon]),'all 16 AutoCAD second-ring slots have a bundled icon');
assert.equal(new Set(acadOuter.map(s=>s.icon)).size,16,'AutoCAD second-ring icons are distinct');
const modeProfile=structuredClone(acad(config));
modeProfile.id='autocad.layout';modeProfile.name='AutoCAD 布局';modeProfile.scope='mode';modeProfile.mode='layout';
const withMode=structuredClone(config);withMode.profiles.push(modeProfile);assert.equal(check(withMode),true,JSON.stringify(check.errors));
const withGap=structuredClone(config);Object.assign(acad(withGap).outer_rings[0][1],{enabled:false,label:'',action:{type:'adapter',id:'unconfigured'}});assert.equal(check(withGap),true,'16-slot ring with manual empty slot');
const twelve=structuredClone(config);acad(twelve).outer_rings[0]=acad(twelve).outer_rings[0].slice(0,12).map((s,i)=>({...s,direction:`P${String(i+1).padStart(2,'0')}`}));assert.equal(check(twelve),true,JSON.stringify(check.errors));
const invalidIcon=structuredClone(config);acad(invalidIcon).outer_rings[0][0].icon='missing';assert.equal(check(invalidIcon),false);
const invalid=structuredClone(config);invalid.trigger='left';assert.equal(check(invalid),false);
const invalidInner=structuredClone(config);invalidInner.profiles[0].wheel=structuredClone(acad(config).outer_rings[0]);assert.equal(check(invalidInner),false);
const tooBig=structuredClone(config);acad(tooBig).outer_rings[0]=[...acad(tooBig).outer_rings[0],...acad(tooBig).outer_rings[0]];assert.equal(check(tooBig),false);
const tooMany=structuredClone(config);for(let i=0;i<2;i++)acad(tooMany).outer_rings.push(structuredClone(acad(tooMany).outer_rings[0]));assert.equal(check(tooMany),false);
console.log(`Schema: default profiles valid (outer rings 16 slots); ${Object.keys(icons).length} icons in sync; ${commands} library commands valid; manual gaps and 1-24 slot rings accepted; invalid icon/trigger/inner/size/ring count rejected`);
