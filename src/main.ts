import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import { parse as parseYaml, stringify as stringifyYaml } from 'yaml';
import brandLogo from './assets/sayelf-logo.webp';
import {
    type Action, type Sector, type CatalogItem, icons, iconFor, ringDirections, positionName, catalogFor, otherCatalog,
    sectorFromCatalog, sameAction, isEmptySlot, fillSlot, clearSlot, resizeRing, fillEmptySlots, newOuterRing, THEMES, themeStyle,
} from './wheel-model';
import './style.css';
type ThemeId = keyof typeof THEMES;
type Profile = {
    id: string;
    name: string;
    scope: 'application' | 'mode' | 'desktop' | 'default';
    application?: string;
    mode?: string;
    wheel: Sector[];
    outer_rings?: Sector[][];
};
type RunningApplication = {
    process: string;
    title: string;
};
type Config = {
    version: number;
    trigger: 'xbutton1' | 'xbutton2' | 'middle';
    profiles: Profile[];
};
type Frame = {
    profile: Profile;
    theme: ThemeId;
    selected: number | null;
    center: [
        number,
        number
    ];
    native_icons: (string | null)[];
};
type AdaptiveStatus = {
    enabled: boolean;
    threshold: number;
    theme: ThemeId;
    active_modes: Record<string, string>;
    profiles: { id: string; name: string; scope: string; application?: string; mode?: string; executions: number; learned: boolean; wheel: Sector[]; outer_rings: Sector[][] }[];
};
type HabitsImportReport = {
    matchedProfiles: number;
    skippedProfiles: number;
    matchedCommands: number;
    skippedCommands: number;
    importedLayouts: number;
    skippedLayouts: number;
    importedScenes: number;
    skippedScenes: number;
};
const root = document.querySelector<HTMLElement>('#app')!;
const dirs = ringDirections(0, 8);
const directionsForRing = (ring: Sector[], ringIndex: number) => ringDirections(ringIndex, ring.length);
const svgNS = 'http://www.w3.org/2000/svg';
let testEvidence = false;
let currentProfile = '';
let currentApplication: string | undefined;
let currentSectors: Sector[] = [];
let currentCenterLabel = '';
let currentHubRadius = 0;
let nativeIconFailures = 0;
function el(name: string, attrs: Record<string, string>, text?: string) { const e = document.createElementNS(svgNS, name); for (const [k, v] of Object.entries(attrs))
    e.setAttribute(k, v); if (text)
    e.textContent = text; return e; }
function polar(r: number, a: number) { return [180 + r * Math.sin(a), 180 - r * Math.cos(a)]; }
function addSectorIcon(svg: SVGSVGElement, sector: Sector, position: number[], size: number, index: number) {
    svg.append(createSectorIcon(sector, position, size, index));
}
function createSectorIcon(sector: Sector, position: number[], size: number, index: number): SVGElement {
    const icon = iconFor(sector, currentApplication);
    const group = el('g', { transform: `translate(${position[0] - size / 2} ${position[1] - size / 2}) scale(${size / 24})`, class: 'sector-icon', 'data-index': String(index), 'data-icon': icon, 'aria-label': sector.label, role: 'img' });
    group.append(el('title', {}, sector.label));
    group.append(el('path', { d: icons[icon].d, class: 'icon-mark' }));
    return group;
}
function centerVectorGlyphs(container: ParentNode) {
    container.querySelectorAll<SVGPathElement>('.sector-icon .icon-mark').forEach(path => {
        const bounds = path.getBBox();
        const dx = 12 - (bounds.x + bounds.width / 2);
        const dy = 12 - (bounds.y + bounds.height / 2);
        path.setAttribute('transform', `translate(${dx} ${dy})`);
    });
}
function addNativeSectorIcon(svg: SVGSVGElement, sector: Sector, position: number[], size: number, index: number, source: string) {
    // AutoCAD ribbon bitmaps are drawn for a light ribbon; a light plate keeps them readable on every theme.
    const pad = Math.max(2, size * 0.16);
    const chip = el('rect', { x: String(position[0] - size / 2 - pad), y: String(position[1] - size / 2 - pad), width: String(size + pad * 2), height: String(size + pad * 2), rx: String(pad + 1), class: 'native-chip', 'data-index': String(index) });
    svg.append(chip);
    const icon = el('image', { x: String(position[0] - size / 2), y: String(position[1] - size / 2), width: String(size), height: String(size), href: source, class: 'native-sector-icon', 'data-native-status': 'pending', 'data-index': String(index), 'data-icon': iconFor(sector, currentApplication), 'aria-label': sector.label, role: 'img' });
    icon.append(el('title', {}, sector.label));
    icon.addEventListener('load', () => {
        icon.dataset.nativeStatus = 'loaded';
        probeRenderState();
    }, { once: true });
    icon.addEventListener('error', () => {
        nativeIconFailures += 1;
        const fallback = createSectorIcon(sector, position, size, index);
        fallback.dataset.nativeFallback = 'true';
        chip.remove();
        icon.replaceWith(fallback);
        centerVectorGlyphs(fallback);
        probeRenderState();
    }, { once: true });
    svg.append(icon);
}
function fitCenterLabel() {
    const text = document.querySelector<SVGTextElement>('.center');
    if (!text) return;
    text.removeAttribute('textLength');
    let size = 13;
    const maxWidth = Math.max(20, currentHubRadius * 1.62);
    text.style.fontSize = `${size}px`;
    while (size > 8 && text.getComputedTextLength() > maxWidth) {
        size -= 1;
        text.style.fontSize = `${size}px`;
    }
    const measured = text.getComputedTextLength();
    const fits = measured <= maxWidth + 0.5;
    if (!fits) {
        text.setAttribute('textLength', String(maxWidth));
        text.setAttribute('lengthAdjust', 'spacingAndGlyphs');
    }
    text.dataset.fitted = 'true';
}
function fitSectorLabels(svg: SVGSVGElement) {
    svg.querySelectorAll<SVGTextElement>('.sector-label').forEach(text => {
        const segments = Number(text.dataset.segments || 8);
        const radius = Number(text.dataset.radius || 0);
        const maxWidth = Math.max(12, 2 * radius * Math.sin(Math.PI / segments) * 0.78);
        let size = 10;
        text.style.fontSize = `${size}px`;
        while (size > 7 && text.getComputedTextLength() > maxWidth) {
            size -= 0.5;
            text.style.fontSize = `${size}px`;
        }
        if (text.getComputedTextLength() > maxWidth + 0.5) {
            text.setAttribute('textLength', String(maxWidth));
            text.setAttribute('lengthAdjust', 'spacingAndGlyphs');
        }
        text.dataset.fitted = String(text.getComputedTextLength() <= maxWidth + 0.5);
    });
}
function draw(f: Frame) {
    currentProfile = f.profile.id;
    currentApplication = f.profile.application;
    nativeIconFailures = 0;
    const rings = [f.profile.wheel, ...(f.profile.outer_rings ?? [])];
    currentSectors = rings.flatMap((ring, ringIndex) => directionsForRing(ring, ringIndex).map(direction => ring.find(sector => sector.direction === direction) ?? { direction, label: '', action: { type: 'adapter', id: 'unconfigured' } as Action, enabled: false }));
    currentCenterLabel = f.profile.name;
    root.replaceChildren();
    const svg = el('svg', { viewBox: '0 0 360 360', width: '360', height: '360', 'aria-label': f.profile.name, class: `wheel theme-${f.theme}`, style: themeStyle(f.theme) }) as SVGSVGElement;
    const inner = rings.length === 1 ? 54 : 36;
    const ringWidth = (162 - inner) / rings.length;
    let indexOffset = 0;
    rings.forEach((ring, ringIndex) => {
        const directions = directionsForRing(ring, ringIndex);
        const segments = directions.length;
        const step = 2 * Math.PI / segments;
        const r0 = inner + ringWidth * ringIndex;
        const r1 = r0 + ringWidth;
        const middle = (r0 + r1) / 2;
        // Icon size follows the real arc length, so 3 or 24 commands both fit.
        const arc = middle * step;
        const size = Math.max(10, Math.min(23, ringWidth * 0.52, arc * 0.58));
        directions.forEach((d, i) => {
            const a = segments === 1 ? 0 : i * step - step / 2;
            const b = segments === 1 ? 2 * Math.PI - 0.0001 : i * step + step / 2;
            const p = polar(r1, a), q = polar(r1, b), u = polar(r0, b), v = polar(r0, a);
            const large = b - a > Math.PI ? 1 : 0;
            const sector = ring.find(s => s.direction === d);
            const index = indexOffset + i;
            const path = el('path', { d: `M ${p} A ${r1} ${r1} 0 ${large} 1 ${q} L ${u} A ${r0} ${r0} 0 ${large} 0 ${v} Z`, class: `sector${sector?.enabled === false ? ' empty' : ''}`, 'data-index': String(index) });
            svg.append(path);
            if (sector?.enabled !== false && sector?.label) {
                const monochrome = f.theme === 'cad_monochrome';
                const iconRadius = middle - (monochrome ? size * 0.58 + 3 : 0);
                const pos = polar(iconRadius, i * step);
                const nativeIcon = f.native_icons?.[index];
                if (nativeIcon && !sector.icon) addNativeSectorIcon(svg, sector, pos, size, index, nativeIcon);
                else addSectorIcon(svg, sector, pos, size, index);
                if (monochrome) {
                    const labelPosition = polar(middle + size * 0.58 + 3, i * step);
                    const labelRadius = Math.hypot(labelPosition[0] - 180, labelPosition[1] - 180);
                    svg.append(el('text', { x: String(labelPosition[0]), y: String(labelPosition[1]), 'text-anchor': 'middle', 'dominant-baseline': 'middle', class: 'sector-label', 'data-index': String(index), 'data-segments': String(segments), 'data-radius': String(labelRadius) }, sector.label));
                }
            }
        });
        indexOffset += segments;
    });
    const hubRadius = Math.min(48, inner - 6);
    currentHubRadius = hubRadius;
    svg.append(el('circle', { cx: '180', cy: '180', r: String(hubRadius), class: 'hub' }));
    const logoClipId = 'wheel-center-logo-clip';
    const logoClip = el('clipPath', { id: logoClipId });
    logoClip.append(el('circle', { cx: '180', cy: '180', r: String(Math.max(1, hubRadius - 2)) }));
    const defs = el('defs', {});
    defs.append(logoClip);
    svg.append(defs);
    const logoSize = Math.max(2, (hubRadius - 2) * 2);
    svg.append(el('image', {
        x: String(180 - logoSize / 2), y: String(180 - logoSize / 2),
        width: String(logoSize), height: String(logoSize), href: brandLogo,
        preserveAspectRatio: 'xMidYMid slice', 'clip-path': `url(#${logoClipId})`,
        class: 'center-logo', 'aria-label': 'SAYELF 山野精灵', role: 'img',
    }));
    svg.append(el('text', { x: '180', y: '180', 'text-anchor': 'middle', 'dominant-baseline': 'middle', class: 'center' }, f.profile.name));
    root.append(svg);
    centerVectorGlyphs(svg);
    fitSectorLabels(svg);
    highlight(f.selected);
}
function highlight(i: number | null) {
    document.querySelectorAll('.sector').forEach((p, n) => p.classList.toggle('selected', n === i));
    document.querySelectorAll('.sector-icon').forEach(icon => icon.classList.toggle('selected', Number(icon.getAttribute('data-index')) === i));
    document.querySelectorAll('.native-sector-icon, .native-chip').forEach(icon => icon.classList.toggle('selected', Number(icon.getAttribute('data-index')) === i));
    document.querySelectorAll('.sector-label').forEach(label => label.classList.toggle('selected', Number(label.getAttribute('data-index')) === i));
    const selected = i === null ? undefined : currentSectors[i];
    const hasSelection = selected?.enabled !== false && Boolean(selected?.label);
    currentCenterLabel = hasSelection ? selected?.label ?? '' : '';
    const center = document.querySelector<SVGTextElement>('.center');
    const logo = document.querySelector<SVGImageElement>('.center-logo');
    if (logo) {
        logo.style.display = hasSelection ? 'none' : '';
        logo.setAttribute('aria-hidden', String(hasSelection));
    }
    if (center) {
        center.textContent = hasSelection ? currentCenterLabel : '';
        center.style.display = hasSelection ? '' : 'none';
        center.setAttribute('aria-label', hasSelection ? currentCenterLabel : 'SAYELF 山野精灵');
        if (hasSelection) fitCenterLabel();
    }
    probeRenderState();
}
function probeRenderState() {
    if (!testEvidence) return;
    const nativeImages = Array.from(document.querySelectorAll<SVGImageElement>('.native-sector-icon'));
    void invoke('render_probe', {
        profile: currentProfile,
        sectors: document.querySelectorAll('.sector').length,
        selected: document.querySelector('.sector.selected')?.getAttribute('data-index') ?? 'none',
        icons: document.querySelectorAll('.sector-icon, .native-sector-icon').length,
        caption: currentCenterLabel,
        selectedIcon: document.querySelector<SVGElement>('.sector-icon.selected, .native-sector-icon.selected')?.dataset.icon ?? '',
        fitted: document.querySelector<SVGTextElement>('.center')?.dataset.fitted === 'true',
        nativeIcons: nativeImages.length,
        theme: document.querySelector('.wheel')?.classList.item(1) ?? '',
        sectorLabels: document.querySelectorAll('.sector-label').length,
        labelsFitted: Array.from(document.querySelectorAll<SVGTextElement>('.sector-label')).every(label => label.dataset.fitted === 'true'),
        centerLogo: document.querySelector<SVGImageElement>('.center-logo')?.style.display !== 'none',
        nativeIconsLoaded: nativeImages.filter(image => image.dataset.nativeStatus === 'loaded').length,
        nativeIconFallbacks: document.querySelectorAll('.sector-icon[data-native-fallback="true"]').length,
        nativeIconFailures,
    });
}
async function studio() {
    document.body.classList.add('studio');
    root.innerHTML = `<div class="studio-scroll">
<h1>Profile Studio</h1>
<section class="welcome" aria-labelledby="welcome-title"><h2 id="welcome-title">Context Wheel 设置</h2><p>按住触发键，向命令方向划动，再松开执行；回到中心或按 Esc 取消。</p></section>
<label>触发按钮 <select id="trigger"><option value="xbutton1">侧键 4</option><option value="xbutton2">侧键 5</option><option value="middle">中键</option></select></label>
<section class="wheel-editor">
  <h2>软件、场景与轮盘命令</h2>
  <div class="profile-toolbar"><label>编辑轮盘 <select id="profile-select"></select></label><button id="add-app" type="button">新增专业软件</button><button id="add-mode" type="button">新增场景轮盘</button><button id="delete-profile" type="button">删除</button></div>
  <div class="profile-properties"><label>轮盘名称 <input id="profile-name" maxlength="48"></label><span id="profile-application"></span></div>
  <label>当前运行场景 <select id="active-mode"></select></label>
  <p class="editor-help">场景轮盘按软件分别保存。当前场景在此手动切换并立即生效。内圈固定 8 格；外圈默认 16 格（可改 8 格），每格可单独放入或清空命令，最多三圈。移除命令只改轮盘，不会改专业软件自己的快捷键。</p>
  <div class="ring-toolbar"><label>编辑圈层 <select id="ring-select"></select></label><button id="add-ring" type="button">添加外圈</button><button id="remove-ring" type="button">删除此外圈</button></div>
  <p class="editor-help">拖动位置标记可交换同一圈命令；图标默认自动匹配，也可逐项替换。指向命令时，名称显示在中心圈。</p>
  <div id="sector-editor" class="sector-editor"></div>
  <div id="ring-catalog" class="ring-catalog"></div>
</section>
<section class="adaptive">
  <h2>轮盘外观与个人习惯</h2>
  <label>轮盘底色 <select id="wheel-theme">${Object.entries(THEMES).map(([id, t]) => `<option value="${id}">${t.name}</option>`).join('')}</select></label>
  <label>命令位置 <select id="layout-mode"><option value="frequency">按使用频率自动调整</option><option value="fixed">固定位置，保留方向记忆</option></select></label>
  <p>自动模式只在本机学习专业软件命令频率；达到门槛且频率有差异后稳定调整。固定模式保留方向并停止学习。学习只调整第一圈且不会移动已移除的命令。</p>
  <ul id="adaptive-status"></ul>
  <div id="manual-layout"><label>专业软件 <select id="layout-profile"></select></label><p id="layout-help">选择固定位置后，可用上下按钮调整第一圈命令方向。</p><ol id="layout-items"></ol></div>
  <div class="habit-transfer"><h3>迁移到另一台电脑</h3><p>导出一个习惯文件，在另一台电脑的这里导入。包含命令使用次数、自动布局、学习开关、配色和当前场景；不包含轮盘命令与快捷键设置。数据只保存在本地文件中。</p><div class="habit-transfer-buttons"><button id="export-habits" type="button">导出习惯文件</button><button id="import-habits" type="button">导入习惯文件</button><input id="habits-file" type="file" accept=".json,application/json" hidden></div></div>
  <button id="reset-adaptive" type="button">恢复固定布局并清除习惯</button>
</section>
<details><summary>高级：编辑本地 Profile</summary><p>可直接修改 YAML；新手通常只需使用上面的轮盘编辑器。支持热键、文本/命令、启动程序与预留 Adapter。这里的修改会实时同步到上面的编辑器；YAML 有语法错误时，上面的编辑器会暂停，不会覆盖你写的内容。</p><textarea spellcheck="false" id="yaml"></textarea></details>
</div><div class="save-bar"><output id="result" aria-live="polite"></output><button id="save" type="button">保存配置</button></div>
<dialog id="app-picker" aria-labelledby="app-picker-title"><div class="app-picker-content">
  <h2 id="app-picker-title">选择正在运行的软件</h2>
  <p>选择后自动填写进程名。窗口标题仅临时显示在本机，不会保存。</p>
  <label>运行中的软件<select id="running-apps"></select></label>
  <label>轮盘名称<input id="app-profile-name" maxlength="48" placeholder="例如 AutoCAD"></label>
  <output id="app-picker-status" aria-live="polite"></output>
  <div class="app-picker-actions"><button id="refresh-running-apps" type="button">刷新列表</button><button id="manual-app-entry" type="button">手动输入 EXE</button><span></span><button id="cancel-app-picker" type="button">取消</button><button id="create-app-profile" type="button">添加轮盘</button></div>
</div></dialog>`;
    const field = document.querySelector<HTMLTextAreaElement>('#yaml')!;
    const trigger = document.querySelector<HTMLSelectElement>('#trigger')!;
    const mode = document.querySelector<HTMLSelectElement>('#layout-mode')!;
    const theme = document.querySelector<HTMLSelectElement>('#wheel-theme')!;
    const profileSelect = document.querySelector<HTMLSelectElement>('#profile-select')!;
    const profileName = document.querySelector<HTMLInputElement>('#profile-name')!;
    const profileApplication = document.querySelector<HTMLElement>('#profile-application')!;
    const activeMode = document.querySelector<HTMLSelectElement>('#active-mode')!;
    const ringSelect = document.querySelector<HTMLSelectElement>('#ring-select')!;
    const sectorEditor = document.querySelector<HTMLDivElement>('#sector-editor')!;
    const ringCatalog = document.querySelector<HTMLDivElement>('#ring-catalog')!;
    const layoutProfileSelect = document.querySelector<HTMLSelectElement>('#layout-profile')!;
    const layoutItems = document.querySelector<HTMLOListElement>('#layout-items')!;
    const layoutHelp = document.querySelector<HTMLElement>('#layout-help')!;
    const adaptiveList = document.querySelector<HTMLUListElement>('#adaptive-status')!;
    const result = document.querySelector<HTMLOutputElement>('#result')!;
    const habitsFileInput = document.querySelector<HTMLInputElement>('#habits-file')!;
    const appPicker = document.querySelector<HTMLDialogElement>('#app-picker')!;
    const runningAppsSelect = document.querySelector<HTMLSelectElement>('#running-apps')!;
    const appProfileName = document.querySelector<HTMLInputElement>('#app-profile-name')!;
    const appPickerStatus = document.querySelector<HTMLOutputElement>('#app-picker-status')!;
    let status: AdaptiveStatus | null = null;
    let savedYaml = '';
    let config = parseYaml(await invoke<string>('get_config')) as Config;
    let selectedProfileId = '';
    const currentProfileDraft = () => config.profiles.find(p => p.id === selectedProfileId) ?? null;
    let runningApplications: RunningApplication[] = [];
    const refreshRunningApplications = async () => {
        appPickerStatus.textContent = '正在读取本机可见窗口…';
        try {
            runningApplications = await invoke<RunningApplication[]>('list_running_applications');
            runningAppsSelect.replaceChildren();
            if (runningApplications.length === 0) {
                const option = document.createElement('option');
                option.value = '';
                option.textContent = '没有可选窗口，可手动输入 EXE';
                runningAppsSelect.append(option);
                appPickerStatus.textContent = '未读取到可用窗口；可手动输入进程名。';
                appProfileName.value = '';
                return;
            }
            for (const application of runningApplications) {
                const option = document.createElement('option');
                option.value = application.process;
                option.textContent = `${application.process} · ${application.title}`;
                runningAppsSelect.append(option);
            }
            appPickerStatus.textContent = `找到 ${runningApplications.length} 个正在运行的软件。`;
            const first = runningApplications[0];
            appProfileName.value = first.process.replace(/\.exe$/i, '');
        } catch (error) {
            runningApplications = [];
            runningAppsSelect.replaceChildren();
            const option = document.createElement('option');
            option.value = '';
            option.textContent = '无法读取窗口，可手动输入 EXE';
            runningAppsSelect.append(option);
            appPickerStatus.textContent = `读取失败：${String(error)}`;
        }
    };
    const createApplicationProfile = (name: string, application: string): boolean => {
        const cleanName = name.trim();
        const cleanApplication = application.trim();
        if (!cleanName || !cleanApplication) {
            result.textContent = '请填写轮盘名称和软件进程名。';
            return false;
        }
        const existing = config.profiles.find(p => p.scope === 'application' && p.application?.toLowerCase() === cleanApplication.toLowerCase());
        if (existing) {
            selectedProfileId = existing.id;
            renderProfileEditor();
            result.textContent = `这个进程已有轮盘，已切换到“${existing.name}”。`;
            return true;
        }
        const fallback = config.profiles.find(p => p.id === 'default.safe') ?? config.profiles[0];
        if (!fallback) return false;
        let id = `${slug(cleanApplication)}.default`; let suffix = 2;
        while (config.profiles.some(p => p.id === id)) id = `${slug(cleanApplication)}-${suffix++}.default`;
        const profile: Profile = { id, name: cleanName, scope: 'application', application: cleanApplication, wheel: structuredClone(fallback.wheel), outer_rings: [] };
        config.profiles.push(profile); selectedProfileId = id; syncYaml(); renderProfileEditor();
        result.textContent = '已添加软件轮盘；编辑命令后保存配置。';
        return true;
    };
    let yamlBroken = false;
    const pendingModeResets = new Set<string>();
    const syncYaml = () => { if (!yamlBroken) field.value = stringifyYaml(config); };
    const slug = (value: string) => value.toLowerCase().replace(/\.exe$/i, '').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || `custom-${Date.now()}`;
    const professionalProfiles = () => config.profiles.filter(p => p.scope === 'application' || p.scope === 'mode');
    const sectorsFor = (profile: Profile, ringIndex: number): Sector[] => ringIndex === 0 ? profile.wheel : profile.outer_rings?.[ringIndex - 1] ?? [];
    const refreshProfileOptions = () => {
        const profiles = professionalProfiles();
        profileSelect.replaceChildren(...profiles.map(p => {
            const option = document.createElement('option');
            option.value = p.id;
            option.textContent = `${p.name} · ${p.application ?? ''}${p.scope === 'mode' ? ` / ${p.mode ?? ''}` : ''}`;
            return option;
        }));
        if (!profiles.some(p => p.id === selectedProfileId)) {
            selectedProfileId = profiles.find(p => p.id === 'autocad.default')?.id ?? profiles[0]?.id ?? '';
        }
        profileSelect.value = selectedProfileId;
    };
    const updateActiveModeOptions = (profile: Profile | null) => {
        activeMode.replaceChildren();
        const app = profile?.application;
        const base = document.createElement('option');
        base.value = '';
        base.textContent = '应用默认轮盘';
        activeMode.append(base);
        const appModes = app ? config.profiles.filter(p => p.scope === 'mode' && p.application?.toLowerCase() === app.toLowerCase()) : [];
        appModes.forEach(p => {
            const option = document.createElement('option');
            option.value = p.mode ?? '';
            option.textContent = p.name;
            activeMode.append(option);
        });
        activeMode.disabled = !app;
        activeMode.value = app ? status?.active_modes[app.toLowerCase()] ?? '' : '';
    };
    const iconOptions: [string, string][] = [['', '自动匹配'], ...Object.entries(icons).map(([key, value]) => [key, value.label] as [string, string]).sort((x, y) => x[1].localeCompare(y[1], 'zh'))];
    // Command library as one <select>: this software's groups first, general, then other software.
    const buildCatalogSelect = (profile: Profile, ring: Sector[], placeholder: string) => {
        const select = document.createElement('select');
        const entries: CatalogItem[] = [];
        const first = document.createElement('option');
        first.value = '';
        first.textContent = placeholder;
        select.append(first);
        const addGroups = (groups: ReturnType<typeof catalogFor>, prefix: string) => groups.forEach(group => {
            const optgroup = document.createElement('optgroup');
            optgroup.label = `${prefix}${group.name}`;
            group.items.forEach(item => {
                const option = document.createElement('option');
                option.value = String(entries.length);
                const already = ring.some(existing => !isEmptySlot(existing) && sameAction(existing.action, item.action));
                option.textContent = `${item.label}${already ? '（本圈已有）' : ''}`;
                entries.push(item);
                optgroup.append(option);
            });
            select.append(optgroup);
        });
        addGroups(catalogFor(profile.application), '');
        addGroups(otherCatalog(profile.application), '其他软件 · ');
        return { select, entries };
    };
    const wheelSectors = (profile: Profile) => [profile.wheel, ...(profile.outer_rings ?? [])].flat();
    const renderRingCatalog = (profile: Profile, ringIndex: number) => {
        ringCatalog.replaceChildren();
        const ring = sectorsFor(profile, ringIndex);
        const used = ring.filter(sector => !isEmptySlot(sector)).length;
        const heading = document.createElement('h3');
        heading.textContent = ringIndex === 0
            ? `第一圈（内圈）：固定 8 格，已用 ${used} 格`
            : `第 ${ringIndex + 1} 圈（外圈）：${ring.length} 格，已用 ${used} 格，空 ${ring.length - used} 格`;
        const bar = document.createElement('div');
        bar.className = 'catalog-bar';
        if (ringIndex > 0) {
            const sizeLabel = document.createElement('label');
            sizeLabel.textContent = '格数 ';
            const size = document.createElement('select');
            size.id = 'ring-size';
            size.setAttribute('aria-label', '外圈格数');
            for (const n of [16, 8]) { const option = document.createElement('option'); option.value = String(n); option.textContent = `${n} 格`; size.append(option); }
            size.value = ring.length === 8 ? '8' : '16';
            size.disabled = ring.length !== 8 && ring.length !== 16;
            size.addEventListener('change', () => {
                const { ring: next, dropped } = resizeRing(ring, Number(size.value) as 8 | 16);
                if (dropped.length && !confirm(`改为 8 格会删除位于斜向之间的 ${dropped.length} 个命令：${dropped.map(item => item.label).join('、')}。继续？`)) { size.value = String(ring.length); return; }
                profile.outer_rings![ringIndex - 1] = next;
                syncYaml(); renderSectorEditor();
                result.textContent = `第 ${ringIndex + 1} 圈改为 ${size.value} 格，原有命令角度不变；保存后生效。`;
            });
            sizeLabel.append(size);
            bar.append(sizeLabel);
        }
        const fill = document.createElement('button');
        fill.type = 'button';
        fill.id = 'fill-empty';
        fill.textContent = '用常用功能填满空格';
        fill.disabled = used === ring.length;
        fill.addEventListener('click', () => {
            const added = fillEmptySlots(ring, ringIndex, profile.application, wheelSectors(profile));
            syncYaml(); renderSectorEditor();
            result.textContent = added ? `已填入 ${added} 个本轮盘还没有的常用功能；不需要的可逐格清空。` : '命令库里已没有本轮盘未用的功能，可逐格选择或自定义。';
        });
        const clearAll = document.createElement('button');
        clearAll.type = 'button';
        clearAll.className = 'danger';
        clearAll.textContent = '清空本圈';
        clearAll.disabled = used === 0;
        clearAll.addEventListener('click', () => {
            if (!confirm(`清空第 ${ringIndex + 1} 圈全部 ${used} 个命令？格子位置保留。`)) return;
            ring.forEach(clearSlot);
            syncYaml(); renderSectorEditor();
        });
        bar.append(fill, clearAll);
        const help = document.createElement('p');
        help.className = 'editor-help';
        help.textContent = '每格独立：用左侧“选择功能”放入命令，“清空”只清这一格，其他格的位置不动。AutoCAD 命令执行前会按两次 Esc 取消正在进行的命令；本机能读到 AutoCAD 原生图标时优先显示原生图标。';
        ringCatalog.append(heading, bar, help);
    };
    let focusRing: number | null = null; // ring to show after it was just added/removed
    const renderSectorEditor = () => {
        const profile = currentProfileDraft();
        sectorEditor.replaceChildren();
        ringCatalog.replaceChildren();
        if (!profile) return;
        const ringCount = 1 + (profile.outer_rings?.length ?? 0);
        const priorRing = focusRing ?? Number(ringSelect.value || 0);
        focusRing = null;
        ringSelect.replaceChildren(...Array.from({ length: ringCount }, (_, i) => {
            const option = document.createElement('option');
            option.value = String(i);
            option.textContent = i === 0 ? '第一圈（内圈 · 8 格）' : `第 ${i + 1} 圈（外圈 · ${sectorsFor(profile, i).length} 格）`;
            return option;
        }));
        const ringIndex = Math.min(priorRing, ringCount - 1);
        ringSelect.value = String(ringIndex);
        document.querySelector<HTMLButtonElement>('#add-ring')!.disabled = ringCount >= 3;
        document.querySelector<HTMLButtonElement>('#remove-ring')!.disabled = ringIndex === 0;
        const ring = sectorsFor(profile, ringIndex);
        const directions = directionsForRing(ring, ringIndex);
        directions.forEach((direction, positionIndex) => {
            const sector = ring.find(item => item.direction === direction);
            if (!sector) return;
            const enabled = sector.enabled !== false;
            const position = positionName(direction, ringIndex, positionIndex, directions.length);
            const row = document.createElement('div');
            row.className = `sector-row${enabled ? '' : ' disabled'}`;
            const dir = document.createElement('strong');
            dir.textContent = position;
            const glyph = document.createElementNS(svgNS, 'svg');
            glyph.setAttribute('viewBox', '0 0 24 24');
            glyph.setAttribute('class', `slot-glyph${enabled ? '' : ' empty'}`);
            glyph.setAttribute('aria-hidden', 'true');
            const glyphPath = document.createElementNS(svgNS, 'path');
            glyphPath.setAttribute('d', enabled ? icons[iconFor(sector, profile.application)].d : 'M4 4h16v16H4z');
            glyph.append(glyphPath);
            dir.prepend(glyph);
            dir.draggable = enabled;
            dir.title = enabled ? '拖动此位置标记可交换命令位置' : '空位置';
            dir.setAttribute('aria-label', enabled ? `${position}，拖动换位` : `${position}，空位置`);
            dir.addEventListener('dragstart', event => {
                if (!enabled || !event.dataTransfer) { event.preventDefault(); return; }
                event.dataTransfer.effectAllowed = 'move';
                event.dataTransfer.setData('text/plain', direction);
                row.classList.add('dragging');
            });
            dir.addEventListener('dragend', () => {
                row.classList.remove('dragging');
                sectorEditor.querySelectorAll('.drop-target').forEach(target => target.classList.remove('drop-target'));
            });
            row.addEventListener('dragover', event => {
                if (!event.dataTransfer?.types.includes('text/plain')) return;
                event.preventDefault();
                event.dataTransfer.dropEffect = 'move';
                row.classList.add('drop-target');
            });
            row.addEventListener('dragleave', event => {
                if (!row.contains(event.relatedTarget as Node | null)) row.classList.remove('drop-target');
            });
            row.addEventListener('drop', event => {
                event.preventDefault();
                row.classList.remove('drop-target');
                const sourceDirection = event.dataTransfer?.getData('text/plain');
                const source = ring.find(item => item.direction === sourceDirection);
                if (!source || source === sector) return;
                const sourceContent = { enabled: source.enabled, label: source.label, icon: source.icon, action: source.action };
                const targetContent = { enabled: sector.enabled, label: sector.label, icon: sector.icon, action: sector.action };
                Object.assign(source, targetContent);
                Object.assign(sector, sourceContent);
                syncYaml();
                renderSectorEditor();
                result.textContent = `${profile.name}：命令已换位；点击“保存配置”后生效。`;
            });
            const { select: picker, entries: pickerEntries } = buildCatalogSelect(profile, ring, enabled ? '更换功能…' : '选择功能…');
            picker.className = 'slot-picker';
            picker.setAttribute('aria-label', `${position} 从命令库选择功能`);
            picker.addEventListener('change', () => {
                const item = pickerEntries[Number(picker.value)];
                if (!item) return;
                fillSlot(sector, sectorFromCatalog(item, profile.application));
                syncYaml(); renderSectorEditor();
                result.textContent = `${position}：已放入“${item.label}”；保存后生效。`;
            });
            const commandToggle = document.createElement('button');
            commandToggle.type = 'button';
            if (enabled) {
                commandToggle.textContent = '清空';
                commandToggle.className = 'danger';
                commandToggle.title = '只清空这一格，其他格位置不变';
                commandToggle.addEventListener('click', () => {
                    const removed = sector.label;
                    clearSlot(sector);
                    syncYaml(); renderSectorEditor();
                    result.textContent = `${position}：已清空“${removed}”；保存后生效。`;
                });
            } else {
                commandToggle.textContent = '自定义';
                commandToggle.className = 'secondary';
                commandToggle.title = '在这一格手动填写名称和快捷键';
                commandToggle.addEventListener('click', () => {
                    fillSlot(sector, { direction: sector.direction, label: '新命令', icon: 'command', action: { type: 'hotkey', keys: [] } });
                    syncYaml(); renderSectorEditor();
                });
            }
            const label = document.createElement('input');
            label.type = 'text';
            label.maxLength = 20;
            label.placeholder = enabled ? '显示名称' : '空位置';
            label.value = sector.label;
            label.disabled = !enabled;
            label.setAttribute('aria-label', `${position} 命令名称`);
            label.addEventListener('input', () => { sector.label = label.value; syncYaml(); });
            const iconSelect = document.createElement('select');
            iconSelect.setAttribute('aria-label', `${position} 命令图标`);
            iconOptions.forEach(([value, text]) => { const option = document.createElement('option'); option.value = value; option.textContent = value ? text : `自动匹配（${icons[iconFor(sector, profile.application)]?.label ?? '命令'}）`; iconSelect.append(option); });
            iconSelect.value = sector.icon ?? '';
            iconSelect.disabled = !enabled;
            iconSelect.addEventListener('change', () => { sector.icon = iconSelect.value || undefined; glyphPath.setAttribute('d', icons[iconFor(sector, profile.application)].d); syncYaml(); });
            const action = document.createElement('select');
            action.setAttribute('aria-label', `${position} 动作类型`);
            const actionOptions: [string, string][] = [['hotkey', '快捷键'], ['keystroke', '输入命令/文字'], ['launch', '启动程序']];
            if (sector.action.type === 'adapter') actionOptions.push(['adapter', '适配器（高级）']);
            for (const [value, text] of actionOptions) {
                const option = document.createElement('option'); option.value = value; option.textContent = text; action.append(option);
            }
            action.value = sector.action.type;
            action.disabled = !enabled;
            action.addEventListener('change', () => {
                switch (action.value) {
                    case 'hotkey': sector.action = { type: 'hotkey', keys: [] }; break;
                    case 'keystroke': sector.action = { type: 'keystroke', text: '' }; break;
                    case 'launch': sector.action = { type: 'launch', program: '', args: [] }; break;
                    default: sector.action = { type: 'adapter', id: '' };
                }
                syncYaml();
                renderSectorEditor();
            });
            const value = document.createElement(sector.action.type === 'keystroke' ? 'textarea' : 'input') as HTMLInputElement | HTMLTextAreaElement;
            value.className = 'action-value';
            value.disabled = !enabled;
            value.setAttribute('aria-label', `${position} 动作内容`);
            if (sector.action.type === 'hotkey') {
                value.placeholder = '如 CTRL+1、L、F8';
                value.value = sector.action.keys.join('+');
                value.addEventListener('input', () => { if (sector.action.type === 'hotkey') sector.action.keys = value.value.split('+').map(k => k.trim().toUpperCase()).filter(Boolean); syncYaml(); });
            } else if (sector.action.type === 'keystroke') {
                value.placeholder = '如 ^C^C_.LINE 后按回车';
                value.value = sector.action.text;
                value.addEventListener('input', () => { if (sector.action.type === 'keystroke') sector.action.text = value.value; syncYaml(); });
            } else if (sector.action.type === 'launch') {
                value.placeholder = '程序路径，如 notepad.exe';
                value.value = sector.action.program;
                value.addEventListener('input', () => { if (sector.action.type === 'launch') sector.action.program = value.value; syncYaml(); });
            } else {
                value.placeholder = '适配器 ID';
                value.value = sector.action.id;
                value.addEventListener('input', () => { if (sector.action.type === 'adapter') sector.action.id = value.value; syncYaml(); });
            }
            row.append(dir, picker, commandToggle, label, iconSelect, action, value);
            sectorEditor.append(row);
        });
        renderRingCatalog(profile, ringIndex);
    };
    const renderProfileEditor = () => {
        refreshProfileOptions();
        const profile = currentProfileDraft();
        profileName.value = profile?.name ?? '';
        profileName.disabled = !profile;
        profileApplication.textContent = profile?.application ? `匹配程序：${profile.application}` : '';
        document.querySelector<HTMLButtonElement>('#add-mode')!.disabled = !profile?.application;
        document.querySelector<HTMLButtonElement>('#delete-profile')!.disabled = !profile;
        updateActiveModeOptions(profile);
        renderSectorEditor();
    };
    const renderLayout = () => {
        layoutItems.replaceChildren();
        if (!status) return;
        const current = status.profiles.find(p => p.id === layoutProfileSelect.value);
        if (!current) return;
        const slots = dirs.map(direction => ({ direction, sector: current.wheel.find(s => s.direction === direction) }));
        if (mode.value !== 'fixed') {
            layoutHelp.textContent = '自动模式根据频率排列；切换到固定位置后才能手动微调。';
        } else {
            layoutHelp.textContent = '上下移动命令可调整轮盘方向；快捷键本身不会改变。';
        }
        slots.forEach((slot, index) => {
            const row = document.createElement('li');
            const label = document.createElement('span');
            label.textContent = `${slot.direction}　${slot.sector?.label ?? '未设置'}`;
            row.append(label);
            for (const [title, delta] of [['上移', -1], ['下移', 1]] as const) {
                const button = document.createElement('button');
                button.type = 'button';
                button.textContent = title;
                button.disabled = mode.value !== 'fixed' || index + delta < 0 || index + delta >= slots.length;
                button.addEventListener('click', async () => {
                    if (field.value !== savedYaml) {
                        result.textContent = '请先保存或撤销高级配置中的未保存修改，再调整位置。';
                        return;
                    }
                    const order = slots.map(item => item.sector?.direction ?? item.direction);
                    [order[index], order[index + delta]] = [order[index + delta], order[index]];
                    try {
                        await invoke('set_profile_order', { profileId: current.id, order });
                        field.value = await invoke<string>('get_config');
                        savedYaml = field.value;
                        config = parseYaml(field.value) as Config;
                        renderProfileEditor();
                        await refreshAdaptive();
                        result.textContent = `${current.name} 的轮盘位置已保存；快捷键未改动。`;
                    } catch (e) {
                        result.textContent = String(e);
                    }
                });
                row.append(button);
            }
            layoutItems.append(row);
        });
    };
    const refreshAdaptive = async () => {
        status = await invoke<AdaptiveStatus>('get_adaptive_status');
        mode.value = status.enabled ? 'frequency' : 'fixed';
        theme.value = status.theme;
        const profileIds = status.profiles.map(p => p.id).join('|');
        if (Array.from(layoutProfileSelect.options).map(o => o.value).join('|') !== profileIds) {
            layoutProfileSelect.replaceChildren(...status.profiles.map(p => {
                const option = document.createElement('option');
                option.value = p.id;
                option.textContent = `${p.name} · ${p.application ?? ''}`;
                return option;
            }));
        }
        adaptiveList.replaceChildren(...status.profiles.map(p => {
            const row = document.createElement('li');
            const progress = !status!.enabled ? '固定位置' : p.learned ? '已按习惯调整' : p.executions >= status!.threshold ? `观察中 ${p.executions} 次（频率尚接近）` : `学习中 ${p.executions}/${status!.threshold}`;
            row.textContent = `${p.name}：${progress}`;
            return row;
        }));
        updateActiveModeOptions(currentProfileDraft());
        renderLayout();
    };
    field.value = await invoke<string>('get_config');
    savedYaml = field.value;
    trigger.value = field.value.match(/^trigger:\s*(\w+)/m)?.[1] ?? 'xbutton1';
    await refreshAdaptive();
    renderProfileEditor();
    profileSelect.addEventListener('change', () => { selectedProfileId = profileSelect.value; renderProfileEditor(); });
    ringSelect.addEventListener('change', renderSectorEditor);
    profileName.addEventListener('input', () => {
        const profile = currentProfileDraft(); if (!profile) return;
        profile.name = profileName.value;
        const option = Array.from(profileSelect.options).find(o => o.value === profile.id);
        if (option) option.textContent = `${profile.name} · ${profile.application ?? ''}${profile.scope === 'mode' ? ` / ${profile.mode ?? ''}` : ''}`;
        syncYaml();
    });
    document.querySelector('#add-ring')!.addEventListener('click', () => {
        const profile = currentProfileDraft(); if (!profile) return;
        profile.outer_rings ??= [];
        if (profile.outer_rings.length >= 2) return;
        const ringIndex = profile.outer_rings.length + 1;
        const ring = newOuterRing(profile.application, wheelSectors(profile), ringIndex, 16);
        profile.outer_rings.push(ring);
        syncYaml(); focusRing = ringIndex;
        renderSectorEditor();
        const used = ring.filter(sector => !isEmptySlot(sector)).length;
        result.textContent = `已添加第 ${ringIndex + 1} 圈（16 格），预置 ${used} 个常用功能；可逐格更换或清空。`;
    });
    document.querySelector('#remove-ring')!.addEventListener('click', () => {
        const profile = currentProfileDraft(); const ringIndex = Number(ringSelect.value);
        if (!profile || ringIndex < 1 || !confirm(`删除第 ${ringIndex + 1} 圈及其中的命令配置？`)) return;
        profile.outer_rings?.splice(ringIndex - 1, 1);
        syncYaml(); focusRing = ringIndex - 1; renderSectorEditor();
    });
    document.querySelector('#add-app')!.addEventListener('click', async () => {
        appPicker.showModal();
        await refreshRunningApplications();
    });
    runningAppsSelect.addEventListener('change', () => {
        const selected = runningApplications.find(application => application.process === runningAppsSelect.value);
        if (selected) appProfileName.value = selected.process.replace(/\.exe$/i, '');
    });
    document.querySelector('#refresh-running-apps')!.addEventListener('click', () => void refreshRunningApplications());
    document.querySelector('#cancel-app-picker')!.addEventListener('click', () => appPicker.close());
    document.querySelector('#manual-app-entry')!.addEventListener('click', () => {
        const name = prompt('请输入软件名称，例如 Revit'); if (!name?.trim()) return;
        const application = prompt('请输入该软件进程名，例如 Revit.exe'); if (!application?.trim()) return;
        if (createApplicationProfile(name, application)) appPicker.close();
    });
    document.querySelector('#create-app-profile')!.addEventListener('click', () => {
        if (createApplicationProfile(appProfileName.value, runningAppsSelect.value)) appPicker.close();
    });
    document.querySelector('#add-mode')!.addEventListener('click', () => {
        const current = currentProfileDraft(); if (!current?.application) return;
        const label = prompt(`为 ${current.application} 输入场景名称，例如 布局`); if (!label?.trim()) return;
        const application = current.application;
        const keyBase = slug(label.trim()); let modeKey = keyBase; let suffix = 2;
        while (config.profiles.some(p => p.scope === 'mode' && p.application?.toLowerCase() === application.toLowerCase() && p.mode?.toLowerCase() === modeKey.toLowerCase())) modeKey = `${keyBase}-${suffix++}`;
        const appBase = config.profiles.find(p => p.scope === 'application' && p.application?.toLowerCase() === application.toLowerCase()) ?? current;
        let id = `${slug(application)}.${modeKey}`; suffix = 2;
        while (config.profiles.some(p => p.id === id)) id = `${slug(application)}.${modeKey}-${suffix++}`;
        const profile: Profile = { id, name: `${appBase.name} · ${label.trim()}`, scope: 'mode', application, mode: modeKey, wheel: structuredClone(appBase.wheel), outer_rings: structuredClone(appBase.outer_rings ?? []) };
        config.profiles.push(profile); selectedProfileId = id; syncYaml(); renderProfileEditor(); result.textContent = '场景轮盘已添加；编辑命令并保存后，可在“当前运行场景”中切换。';
    });
    document.querySelector('#delete-profile')!.addEventListener('click', async () => {
        const profile = currentProfileDraft(); if (!profile) return;
        const app = profile.application;
        const removeIds = profile.scope === 'application' ? config.profiles.filter(p => p.application?.toLowerCase() === app?.toLowerCase() && (p.scope === 'application' || p.scope === 'mode')).map(p => p.id) : [profile.id];
        if (!confirm(profile.scope === 'application' ? `删除 ${app} 的默认轮盘和所有场景轮盘？` : `删除“${profile.name}”？`)) return;
        // The scene reset is applied only after the deletion is saved.
        if (app && (profile.scope === 'application' || status?.active_modes[app.toLowerCase()] === profile.mode)) pendingModeResets.add(app);
        config.profiles = config.profiles.filter(p => !removeIds.includes(p.id));
        selectedProfileId = '';
        syncYaml(); renderProfileEditor(); result.textContent = '已删除轮盘草稿；点击保存配置后生效。';
    });
    activeMode.addEventListener('change', async () => {
        const app = currentProfileDraft()?.application; if (!app) return;
        try {
            await invoke('set_active_mode', { application: app, mode: activeMode.value });
            if (status) {
                if (activeMode.value) status.active_modes[app.toLowerCase()] = activeMode.value;
                else delete status.active_modes[app.toLowerCase()];
            }
            result.textContent = activeMode.value ? '场景轮盘已立即切换' : '已切回应用默认轮盘';
        } catch (e) {
            result.textContent = String(e);
            updateActiveModeOptions(currentProfileDraft());
        }
    });
    mode.addEventListener('change', async () => {
        try {
            await invoke('set_adaptive_enabled', { enabled: mode.value === 'frequency' });
            await refreshAdaptive();
            result.textContent = mode.value === 'frequency' ? '已开启频率学习' : '已固定方向并停止学习';
        } catch (e) {
            result.textContent = String(e);
            await refreshAdaptive();
        }
    });
    theme.addEventListener('change', async () => {
        try {
            await invoke('set_wheel_theme', { theme: theme.value });
            result.textContent = '轮盘底色已保存';
        } catch (e) {
            result.textContent = String(e);
            await refreshAdaptive();
        }
    });
    layoutProfileSelect.addEventListener('change', renderLayout);
    document.querySelector('#export-habits')!.addEventListener('click', async () => {
        try {
            const path = await save({
                title: '导出 Context Wheel 习惯',
                defaultPath: `context-wheel-habits-${new Date().toISOString().slice(0, 10)}.json`,
                filters: [{ name: '习惯数据 JSON', extensions: ['json'] }],
            });
            if (!path) return;
            await invoke('export_habits', { path });
            result.textContent = `已导出到：${path}。把此 JSON 文件复制到另一台电脑后即可导入。`;
        } catch (e) {
            result.textContent = `导出失败：${String(e)}`;
        }
    });
    document.querySelector('#import-habits')!.addEventListener('click', () => habitsFileInput.click());
    habitsFileInput.addEventListener('change', async () => {
        const file = habitsFileInput.files?.[0];
        if (!file) return;
        try {
            if (file.size > 1_048_576) throw Error('文件超过 1 MB，无法导入');
            const json = await file.text();
            const preview = await invoke<HabitsImportReport>('preview_habits_import', { json });
            const approved = confirm(`习惯文件预览：\n匹配轮盘 ${preview.matchedProfiles} 个，未匹配 ${preview.skippedProfiles} 个\n匹配命令频次 ${preview.matchedCommands} 项，跳过 ${preview.skippedCommands} 项\n可迁移自动布局 ${preview.importedLayouts} 个，跳过 ${preview.skippedLayouts} 个\n可迁移场景 ${preview.importedScenes} 个，跳过 ${preview.skippedScenes} 个\n\n继续后会采用文件中的学习开关和配色，并更新匹配到的自动布局与场景。命令次数按较大值合并，现有较高次数会保留；轮盘命令和快捷键不会导入。`);
            if (!approved) return;
            const report = await invoke<HabitsImportReport>('import_habits', { json });
            await refreshAdaptive();
            result.textContent = `习惯已导入：匹配轮盘 ${report.matchedProfiles} 个、命令频次 ${report.matchedCommands} 项、自动布局 ${report.importedLayouts} 个、场景 ${report.importedScenes} 个；未匹配轮盘 ${report.skippedProfiles} 个，其他不兼容项目已跳过。`;
        } catch (e) {
            result.textContent = `导入失败：${String(e)}`;
        } finally {
            habitsFileInput.value = '';
        }
    });
    document.querySelector('#reset-adaptive')!.addEventListener('click', async () => {
        if (!confirm('清除本机使用次数，并恢复 Profile 中已保存的固定位置？')) return;
        try {
            await invoke('reset_adaptive');
            await refreshAdaptive();
            result.textContent = '已恢复固定布局并清除习惯记录';
        } catch (e) {
            result.textContent = String(e);
        }
    });
    document.querySelector('#save')!.addEventListener('click', async () => { const out = document.querySelector('#result')!; try {
        if (yamlBroken) throw Error('YAML 有语法错误，请先修正“高级：编辑本地 Profile”中的内容');
        const parsed = parseYaml(field.value) as Config;
        parsed.trigger = trigger.value as Config['trigger'];
        const yaml = stringifyYaml(parsed);
        if (!/^trigger:/m.test(yaml))
            throw Error('缺少 trigger');
        await invoke('save_config', { yaml });
        for (const app of pendingModeResets) {
            try { await invoke('set_active_mode', { application: app, mode: '' }); } catch { /* No active mode was saved. */ }
        }
        pendingModeResets.clear();
        // The engine stores outer rings compacted; show exactly what was stored.
        field.value = await invoke<string>('get_config');
        config = parseYaml(field.value) as Config;
        savedYaml = field.value;
        renderProfileEditor();
        await refreshAdaptive();
        out.textContent = '已保存';
    }
    catch (e) {
        out.textContent = String(e);
    } });
    let yamlTimer = 0;
    field.addEventListener('input', () => {
        window.clearTimeout(yamlTimer);
        yamlTimer = window.setTimeout(() => {
            try {
                const next = parseYaml(field.value) as Config;
                if (!next || !Array.isArray(next.profiles)) throw Error('缺少 profiles');
                config = next;
                yamlBroken = false;
                sectorEditor.classList.remove('frozen');
                ringCatalog.classList.remove('frozen');
                renderProfileEditor();
                result.textContent = '';
            } catch (e) {
                yamlBroken = true;
                sectorEditor.classList.add('frozen');
                ringCatalog.classList.add('frozen');
                result.textContent = `YAML 有误，上方编辑器已暂停：${e instanceof Error ? e.message : String(e)}`;
            }
        }, 400);
    });
    trigger.addEventListener('change', () => {
        if (trigger.value !== 'middle') return;
        const ok = confirm('中键是 AutoCAD / SketchUp / SolidWorks 的平移与环绕键。\n设为触发键后，在这些软件里按住中键拖动会弹出轮盘，而不是平移视图。\n确定使用中键？');
        if (!ok) trigger.value = 'xbutton1';
    });
    await listen<string>('status', e => { result.textContent = e.payload; });
}
if (new URLSearchParams(location.search).has('studio'))
    void studio();
else {
    try {
        await listen<Frame>('wheel', e => draw(e.payload));
        await listen<number | null>('selection', e => highlight(e.payload));
        testEvidence = await invoke<boolean>('renderer_ready');
    } catch(e) {
        await invoke('render_probe',{profile:'frontend-error',sectors:0,selected:String(e),icons:0,caption:'',selectedIcon:'',fitted:false,nativeIcons:0,theme:'',sectorLabels:0,labelsFitted:false,centerLogo:false,nativeIconsLoaded:0,nativeIconFallbacks:0,nativeIconFailures:0});
    }
}
