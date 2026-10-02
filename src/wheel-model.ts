// Pure wheel logic shared by the overlay renderer and Profile Studio.
// No DOM access here, so tests/wheel-model.test.mjs can run it under Node.
import iconLibrary from './icons.json';
import commandCatalog from './command-catalog.json';

export type Action =
    | { type: 'hotkey'; keys: string[] }
    | { type: 'keystroke'; text: string }
    | { type: 'launch'; program: string; args: string[] }
    | { type: 'adapter'; id: string };
export type Sector = { direction: string; label: string; action: Action; icon?: string; enabled?: boolean };
export type CatalogItem = { label: string; icon: string; action: Action };
export type CatalogGroup = { id: string; name: string; apps: string[]; items: CatalogItem[] };

export const icons = iconLibrary as Record<string, { label: string; d: string }>;
export const catalog = commandCatalog as CatalogGroup[];

export const INNER_DIRECTIONS = ['N', 'NE', 'E', 'SE', 'S', 'SW', 'W', 'NW'];
export const DIRECTIONS_16 = ['N', 'NNE', 'NE', 'ENE', 'E', 'ESE', 'SE', 'SSE', 'S', 'SSW', 'SW', 'WSW', 'W', 'WNW', 'NW', 'NNW'];
/** Must match MAX_OUTER_SLOTS in src-tauri/src/core.rs. */
export const MAX_OUTER_SLOTS = 24;

/** Mirrors core.rs ring_directions(): compass names for 8/16, P01..Pnn otherwise. */
export function ringDirections(ringIndex: number, length: number): string[] {
    if (ringIndex === 0 || length === 8) return INNER_DIRECTIONS;
    if (length === 16) return DIRECTIONS_16;
    return Array.from({ length }, (_, i) => `P${String(i + 1).padStart(2, '0')}`);
}

/** A slot with no command: kept in place so the other commands never move. */
export function emptySector(direction: string): Sector {
    return { direction, enabled: false, label: '', action: { type: 'adapter', id: 'unconfigured' } };
}

export const isEmptySlot = (sector: Sector | undefined) => !sector || sector.enabled === false;

/** Put a command into one slot, keeping the slot's direction. */
export function fillSlot(slot: Sector, from: Sector): Sector {
    slot.enabled = true;
    slot.label = from.label;
    slot.action = structuredClone(from.action);
    if (from.icon) slot.icon = from.icon; else delete slot.icon;
    return slot;
}

/** Clear one slot in place (position stays, so neighbours keep their angles). */
export function clearSlot(slot: Sector): Sector {
    slot.enabled = false;
    slot.label = '';
    slot.action = { type: 'adapter', id: 'unconfigured' };
    delete slot.icon;
    return slot;
}

/**
 * Change an outer ring between 8 and 16 slots without moving any command:
 * 8 -> 16 inserts empty slots between existing ones; 16 -> 8 keeps the eight
 * main compass slots and reports how many commands in between would be dropped.
 */
export function resizeRing(ring: Sector[], length: 8 | 16): { ring: Sector[]; dropped: Sector[] } {
    const byDirection = new Map(ring.map(s => [s.direction, s]));
    const target = length === 16 ? DIRECTIONS_16 : INNER_DIRECTIONS;
    const next = target.map(d => byDirection.get(d) ?? emptySector(d));
    const kept = new Set(next);
    const dropped = ring.filter(s => !kept.has(s) && !isEmptySlot(s));
    return { ring: next, dropped };
}

/** Fill empty slots (clockwise from top) with catalog commands not yet on this wheel. */
export function fillEmptySlots(ring: Sector[], ringIndex: number, application: string | undefined, wheelSectors: Sector[]): number {
    const empties = orderedRing(ring, ringIndex).filter(isEmptySlot);
    const picks = seedRing(application, wheelSectors.filter(s => !isEmptySlot(s)), empties.length);
    picks.forEach((pick, i) => fillSlot(empties[i], pick));
    return picks.length;
}

/** New outer ring: `length` fixed slots, pre-filled where the catalog has commands. */
export function newOuterRing(application: string | undefined, wheelSectors: Sector[], ringIndex: number, length: 8 | 16 = 16): Sector[] {
    const ring = ringDirections(ringIndex, length).map(emptySector);
    fillEmptySlots(ring, ringIndex, application, wheelSectors);
    return ring;
}

/** Re-spread an outer ring evenly after commands were added or removed. Keeps list order. */
export function renumberRing(ring: Sector[], ringIndex: number): Sector[] {
    const directions = ringDirections(ringIndex, ring.length);
    ring.forEach((sector, i) => { sector.direction = directions[i]; });
    return ring;
}

/** Outer ring in angular (clockwise from top) order, whatever the YAML order is. */
export function orderedRing(ring: Sector[], ringIndex: number): Sector[] {
    const directions = ringDirections(ringIndex, ring.length);
    return [...ring].sort((a, b) => directions.indexOf(a.direction) - directions.indexOf(b.direction));
}

/** Human position name shown in the editor. */
export function positionName(direction: string, ringIndex: number, index: number, length: number): string {
    if (ringIndex === 0 || length === 8 || length === 16) return direction;
    const degrees = Math.round((360 / length) * index);
    return `${index + 1}号 · ${degrees}°`;
}

/** Same rule as native_icons.rs command_name(): ^C^C_.LINE\n -> LINE */
export function commandName(text: string): string | null {
    let command = text.trim();
    while (/^\^c/i.test(command)) command = command.slice(2).trimStart();
    command = command.replace(/^[_.]+/, '');
    const token = command.split(/[\s;\\]/)[0] ?? '';
    return /^[A-Za-z0-9]+$/.test(token) ? token.toUpperCase() : null;
}

const appMatches = (group: CatalogGroup, application?: string) =>
    group.apps.includes('*') || (!!application && group.apps.includes(application.toLowerCase()));

const actionKey = (action: Action): string => {
    switch (action.type) {
        case 'hotkey': return `hotkey:${action.keys.map(k => k.toUpperCase()).join('+')}`;
        case 'keystroke': { const name = commandName(action.text); return name ? `cmd:${name}` : `text:${action.text}`; }
        case 'launch': return `launch:${action.program.toLowerCase()}`;
        default: return `adapter:${action.id}`;
    }
};

/** Catalog groups for one application: its own groups first, then the general group. */
export function catalogFor(application?: string): CatalogGroup[] {
    const own = catalog.filter(g => !g.apps.includes('*') && appMatches(g, application));
    const general = catalog.filter(g => g.apps.includes('*'));
    return [...own, ...general];
}

/** Groups of other software, offered last so any profile can borrow commands. */
export function otherCatalog(application?: string): CatalogGroup[] {
    return catalog.filter(g => !g.apps.includes('*') && !appMatches(g, application));
}

const legacyAliases: [string, RegExp][] = [
    ['polyline', /多段线|pline/], ['line', /直线|(^|\W)line(\W|$)/], ['circle', /圆$|圆形|circle/],
    ['rectangle', /矩形|rectang/], ['arc', /圆弧|arc/], ['move', /移动|move/], ['offset', /偏移|offset/],
    ['trim', /修剪|trim/], ['copy', /复制|copy/], ['rotate', /旋转|rotate/], ['scale', /比例缩放|scale/],
    ['delete', /删除|erase/], ['layer', /图层|layer/], ['text', /文字|文本|text/], ['properties', /特性|属性|properties/],
    ['select', /选择|select/], ['orbit', /环绕|orbit/], ['pan', /平移|pan/], ['zoom', /缩放|zoom/], ['measure', /卷尺|测量|measure|tape/],
    ['material', /材质|material/], ['view', /视图|view/], ['folder', /资源管理器|文件夹|folder/], ['run', /运行|run/],
    ['paste', /粘贴|paste/], ['undo', /撤销|undo/], ['redo', /重做|redo/], ['save', /保存|save/], ['search', /搜索|查找|search/],
    ['window', /切换窗口|window/], ['cut', /剪切|cut/], ['designcenter', /designcenter|设计中心/], ['palette', /工具选项板|palette/],
    ['mirror', /镜像|mirror/], ['extend', /延伸|extend/], ['fillet', /圆角|fillet/], ['chamfer', /倒角|chamfer/], ['array', /阵列|array/],
    ['pushpull', /推拉/], ['selectall', /全选/], ['command', /命令行|命令|command/],
];

/**
 * Icon for a sector: explicit icon > catalog entry with the same action in this
 * application (so old configs without an icon field still get the right glyph)
 * > catalog entry with the same label > legacy keyword rules > generic command.
 */
export function iconFor(sector: Sector, application?: string): string {
    if (sector.icon && icons[sector.icon]) return sector.icon;
    const groups = catalogFor(application);
    const key = actionKey(sector.action);
    const byAction = groups.flatMap(g => g.items).find(item => actionKey(item.action) === key);
    if (byAction && icons[byAction.icon]) return byAction.icon;
    const byLabel = groups.flatMap(g => g.items).find(item => item.label === sector.label.trim());
    if (byLabel && icons[byLabel.icon]) return byLabel.icon;
    const source = `${sector.label} ${sector.action.type === 'keystroke' ? sector.action.text : ''}`.toLowerCase();
    return legacyAliases.find(([, pattern]) => pattern.test(source))?.[0] ?? 'command';
}

/**
 * New sector from a catalog item. AutoCAD typed commands keep icon unset so the
 * real AutoCAD ribbon icon (read from the local CUIx) is shown when available;
 * iconFor() still finds the catalog glyph as fallback.
 */
export function sectorFromCatalog(item: CatalogItem, application?: string): Sector {
    const isAutoCadCommand = application?.toLowerCase() === 'acad.exe' && item.action.type === 'keystroke';
    const sector: Sector = { direction: '', label: item.label, action: structuredClone(item.action) };
    if (!isAutoCadCommand) sector.icon = item.icon;
    return sector;
}

/** Commands to seed a newly added outer ring, so it never starts with empty slots. */
export function seedRing(application: string | undefined, existing: Sector[], count = 8): Sector[] {
    const taken = new Set(existing.map(s => actionKey(s.action)));
    const picks: CatalogItem[] = [];
    for (const item of catalogFor(application).flatMap(g => g.items)) {
        if (picks.length >= count) break;
        const key = actionKey(item.action);
        if (taken.has(key)) continue;
        taken.add(key);
        picks.push(item);
    }
    return picks.map(item => sectorFromCatalog(item, application));
}

export const sameAction = (a: Action, b: Action) => actionKey(a) === actionKey(b);

// ---- contrast (WCAG 2.x relative luminance) ----
function channel(v: number) { const c = v / 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; }
export function luminance(hex: string): number {
    const m = hex.replace('#', '');
    const full = m.length === 3 ? m.split('').map(c => c + c).join('') : m;
    const [r, g, b] = [0, 2, 4].map(i => parseInt(full.slice(i, i + 2), 16));
    return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}
export function contrast(a: string, b: string): number {
    const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
    return (x + 0.05) / (y + 0.05);
}

/**
 * Wheel themes. Every foreground/background pair is checked by
 * tests/wheel-model.test.mjs to be >= 4.5:1, selected state included.
 * chip = backing plate behind native AutoCAD bitmaps (they are drawn for a light ribbon).
 */
export const THEMES = {
    tech_blue: { name: '科技蓝', sector: '#0b355c', stroke: '#3c91d0', icon: '#ffffff', selected: '#9bd5ff', selectedIcon: '#06243f', selectedStroke: '#ffffff', hub: '#061b30', label: '#ffffff', centerText: '#ffffff', chip: '#eef6fc' },
    deep_blue: { name: '深海蓝', sector: '#061c35', stroke: '#235b91', icon: '#e8f4ff', selected: '#7cc4ff', selectedIcon: '#03101f', selectedStroke: '#e8f4ff', hub: '#03101f', label: '#e8f4ff', centerText: '#e8f4ff', chip: '#e8f1f8' },
    ice_blue: { name: '冰川蓝', sector: '#cfe6f6', stroke: '#4686b5', icon: '#0b2a44', selected: '#0b4f86', selectedIcon: '#ffffff', selectedStroke: '#0b2a44', hub: '#eef7fd', label: '#0b2a44', centerText: '#0b2a44', chip: '#ffffff' },
    cad_monochrome: { name: 'CAD 黑白', sector: '#0a0a0a', stroke: '#858585', icon: '#ffffff', selected: '#f2f2f2', selectedIcon: '#000000', selectedStroke: '#ffffff', hub: '#030303', label: '#ffffff', centerText: '#ffffff', chip: '#f2f2f2' },
    high_contrast: { name: '高对比 黑黄', sector: '#000000', stroke: '#ffd400', icon: '#ffd400', selected: '#ffd400', selectedIcon: '#000000', selectedStroke: '#ffffff', hub: '#000000', label: '#ffd400', centerText: '#ffd400', chip: '#ffffff' },
} as const;
export type ThemeId = keyof typeof THEMES;

/** CSS custom properties for one theme, applied on the wheel <svg>. */
export function themeStyle(id: string): string {
    const t = THEMES[(id in THEMES ? id : 'tech_blue') as ThemeId];
    return [
        `--cw-sector:${t.sector}`, `--cw-stroke:${t.stroke}`, `--cw-icon:${t.icon}`,
        `--cw-selected:${t.selected}`, `--cw-selected-icon:${t.selectedIcon}`, `--cw-selected-stroke:${t.selectedStroke}`,
        `--cw-hub:${t.hub}`, `--cw-label:${t.label}`, `--cw-center:${t.centerText}`, `--cw-chip:${t.chip}`,
    ].join(';');
}
