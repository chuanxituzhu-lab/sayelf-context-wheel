use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const DIRECTIONS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
pub const DIRECTIONS_16: [&str; 16] = [
    "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW",
    "NNW",
];
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Hotkey {
        keys: Vec<String>,
    },
    Keystroke {
        text: String,
    },
    Launch {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Adapter {
        id: String,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sector {
    pub direction: String,
    #[serde(default = "enabled_by_default", skip_serializing_if = "is_true")]
    pub enabled: bool,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub action: Action,
}

fn is_true(value: &bool) -> bool {
    *value
}

fn enabled_by_default() -> bool {
    true
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub scope: Scope,
    #[serde(default)]
    pub application: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    pub wheel: Vec<Sector>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outer_rings: Vec<Vec<Sector>>,
}
impl Profile {
    pub fn ring_count(&self) -> usize {
        1 + self.outer_rings.len()
    }

    pub fn sector_at(&self, slot: usize) -> Option<&Sector> {
        let mut offset = 0;
        for (ring_index, ring) in std::iter::once(&self.wheel)
            .chain(self.outer_rings.iter())
            .enumerate()
        {
            if slot < offset + ring.len() {
                let directions = directions_for_ring(ring_index, ring);
                let direction = directions.get(slot - offset)?;
                return ring.iter().find(|sector| sector.direction == *direction);
            }
            offset += ring.len();
        }
        None
    }

    pub fn slot_count(&self) -> usize {
        self.wheel.len() + self.outer_rings.iter().map(Vec::len).sum::<usize>()
    }
}

/// Outer rings hold 1..=MAX_OUTER_SLOTS commands spread evenly around the circle.
pub const MAX_OUTER_SLOTS: usize = 24;

/// Direction ids for a ring. The inner ring and 8/16-slot outer rings keep compass
/// names (backward compatible); any other outer-ring size uses position ids P01..Pnn.
pub fn ring_directions(ring_index: usize, len: usize) -> Vec<String> {
    if ring_index == 0 || len == DIRECTIONS.len() {
        DIRECTIONS.iter().map(|d| d.to_string()).collect()
    } else if len == DIRECTIONS_16.len() {
        DIRECTIONS_16.iter().map(|d| d.to_string()).collect()
    } else {
        (1..=len).map(|i| format!("P{i:02}")).collect()
    }
}

pub fn directions_for_ring(ring_index: usize, ring: &[Sector]) -> Vec<String> {
    ring_directions(ring_index, ring.len())
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Object,
    Mode,
    Application,
    Desktop,
    Default,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub trigger: Trigger,
    pub profiles: Vec<Profile>,
}
fn default_action_matches(current: &Action, default: &Action) -> bool {
    match (current, default) {
        (Action::Keystroke { text: current }, Action::Keystroke { text: default }) => {
            fn command(text: &str) -> &str {
                let text = text.trim();
                if text
                    .get(..4)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("^C^C"))
                {
                    text[4..].trim()
                } else {
                    text
                }
            }
            command(current).eq_ignore_ascii_case(command(default))
        }
        _ => current == default,
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    Xbutton1,
    Xbutton2,
    Middle,
}
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub hwnd: isize,
    pub pid: u32,
    pub process: String,
    pub process_path: String,
    pub desktop: bool,
    pub mode: Option<String>,
    pub object: Option<String>,
}
impl Config {
    /// Add bundled vector icons to untouched AutoCAD default commands when upgrading
    /// profiles created before the defaults included explicit icons.
    pub fn fill_missing_default_autocad_icons(&mut self, defaults: &Config) -> usize {
        let Some(default_profile) = defaults.profiles.iter().find(|p| p.id == "autocad.default")
        else {
            return 0;
        };
        let Some(default_ring) = default_profile.outer_rings.first() else {
            return 0;
        };
        let Some(profile) = self.profiles.iter_mut().find(|p| {
            p.id == "autocad.default"
                && p.application
                    .as_deref()
                    .is_some_and(|app| app.eq_ignore_ascii_case("acad.exe"))
        }) else {
            return 0;
        };
        let Some(ring) = profile.outer_rings.first_mut() else {
            return 0;
        };

        let mut filled = 0;
        for source in default_ring {
            let Some(icon) = source.icon.as_ref() else {
                continue;
            };
            let Some(target) = ring.iter_mut().find(|s| s.direction == source.direction) else {
                continue;
            };
            if target.enabled
                && target.icon.is_none()
                && default_action_matches(&target.action, &source.action)
            {
                target.icon = Some(icon.clone());
                filled += 1;
            }
        }
        filled
    }

    /// Preserve the eight existing CAD commands at their original compass angles,
    /// adding disabled intermediate slots so older local profiles gain 16-way outer rings.
    pub fn expand_legacy_autocad_outer_rings(&mut self) {
        for profile in &mut self.profiles {
            if !profile
                .application
                .as_deref()
                .is_some_and(|app| app.eq_ignore_ascii_case("acad.exe"))
            {
                continue;
            }
            for ring in &mut profile.outer_rings {
                if ring.len() != DIRECTIONS.len() {
                    continue;
                }
                let prior = std::mem::take(ring);
                let expanded = DIRECTIONS_16
                    .iter()
                    .enumerate()
                    .map(|(index, direction)| {
                        if index % 2 == 0 {
                            prior
                                .iter()
                                .find(|sector| sector.direction == *direction)
                                .cloned()
                        } else {
                            Some(Sector {
                                direction: (*direction).into(),
                                enabled: false,
                                label: String::new(),
                                icon: None,
                                action: Action::Adapter {
                                    id: "unconfigured".into(),
                                },
                            })
                        }
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(expanded) = expanded {
                    *ring = expanded;
                } else {
                    *ring = prior;
                }
            }
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("只支持 version 1".into());
        }
        let mut ids = std::collections::HashSet::new();
        let mut apps = std::collections::HashSet::new();
        let mut modes = std::collections::HashSet::new();
        for p in &self.profiles {
            if p.id.is_empty() || p.name.is_empty() || !ids.insert(&p.id) {
                return Err("Profile id/name 为空或 id 重复".into());
            }
            if p.scope == Scope::Object {
                return Err("Object 模式识别暂未启用".into());
            }
            if matches!(p.scope, Scope::Application | Scope::Mode) {
                let app = p.application.as_ref().ok_or("Application 缺少进程名")?;
                if app.is_empty() {
                    return Err("应用映射为空".into());
                }
                if p.scope == Scope::Application && !apps.insert(app.to_lowercase()) {
                    return Err("应用默认 Profile 重复".into());
                }
                if p.scope == Scope::Mode {
                    let mode = p.mode.as_ref().ok_or("Mode Profile 缺少场景名称")?;
                    if mode.trim().is_empty()
                        || !modes.insert(format!("{}:{}", app.to_lowercase(), mode.to_lowercase()))
                    {
                        return Err("场景名称为空或在同一软件中重复".into());
                    }
                }
            } else if p.mode.is_some() {
                return Err("只有 Mode Profile 可以设置 mode".into());
            }
            if p.outer_rings.len() > 2 {
                return Err(format!("{} 最多支持三圈轮盘", p.id));
            }
            let rings = std::iter::once(&p.wheel).chain(p.outer_rings.iter());
            let mut configured = 0usize;
            for (ring_index, ring) in rings.enumerate() {
                let size_ok = if ring_index == 0 {
                    ring.len() == DIRECTIONS.len()
                } else {
                    (1..=MAX_OUTER_SLOTS).contains(&ring.len())
                };
                let valid_directions = ring_directions(ring_index, ring.len());
                if !size_ok
                    || valid_directions
                        .iter()
                        .any(|d| ring.iter().filter(|s| s.direction == *d).count() != 1)
                {
                    return Err(format!(
                        "{} 第 {} 圈方向数量不正确或存在重复（内圈固定 8 个，外圈 1–{} 个）",
                        p.id,
                        ring_index + 1,
                        MAX_OUTER_SLOTS
                    ));
                }
                for s in ring {
                    if !s.enabled {
                        continue;
                    }
                    configured += 1;
                    if s.label.trim().is_empty() {
                        return Err("已启用扇区的名称不能为空".into());
                    }
                    match &s.action {
                        Action::Hotkey { keys } => {
                            if keys.is_empty() || keys.iter().any(|k| key_code(k).is_none()) {
                                return Err("不支持的快捷键".into());
                            }
                        }
                        Action::Launch { program, .. } if program.is_empty() => {
                            return Err("启动程序为空".into())
                        }
                        Action::Adapter { id } if id.is_empty() || id == "unconfigured" => {
                            return Err("请先为已启用命令配置可执行动作".into())
                        }
                        _ => {}
                    }
                }
            }
            if configured == 0 {
                return Err(format!("{} 至少需要一个启用的命令", p.id));
            }
        }
        for profile in self.profiles.iter().filter(|p| p.scope == Scope::Mode) {
            if !profile
                .application
                .as_ref()
                .is_some_and(|app| apps.contains(&app.to_lowercase()))
            {
                return Err(format!("{} 的场景轮盘缺少应用默认 Profile", profile.id));
            }
        }
        for scope in [Scope::Desktop, Scope::Default] {
            if self.profiles.iter().filter(|p| p.scope == scope).count() != 1 {
                return Err("必须有唯一 Desktop 和 Default".into());
            }
        }
        Ok(())
    }
    pub fn resolve(&self, c: &Context) -> &Profile {
        self.profiles
            .iter()
            .find(|p| {
                p.scope == Scope::Mode
                    && c.mode.as_ref().is_some_and(|mode| {
                        p.mode
                            .as_ref()
                            .is_some_and(|p_mode| p_mode.eq_ignore_ascii_case(mode))
                    })
                    && p.application
                        .as_ref()
                        .is_some_and(|a| a.eq_ignore_ascii_case(&c.process))
            })
            .or_else(|| {
                self.profiles.iter().find(|p| {
                    p.scope == Scope::Application
                        && p.application
                            .as_ref()
                            .is_some_and(|a| a.eq_ignore_ascii_case(&c.process))
                })
            })
            .or_else(|| {
                self.profiles
                    .iter()
                    .find(|p| p.scope == Scope::Desktop && c.desktop)
            })
            .or_else(|| self.profiles.iter().find(|p| p.scope == Scope::Default))
            .expect("validated default")
    }
}

pub const ADAPTIVE_LEARNING_THRESHOLD: u64 = 40;
const EASY_DIRECTIONS: [&str; 8] = ["N", "E", "S", "W", "NE", "SE", "SW", "NW"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelTheme {
    #[default]
    TechBlue,
    DeepBlue,
    IceBlue,
    CadMonochrome,
    HighContrast,
}

impl WheelTheme {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "tech_blue" => Ok(Self::TechBlue),
            "deep_blue" => Ok(Self::DeepBlue),
            "ice_blue" => Ok(Self::IceBlue),
            "cad_monochrome" => Ok(Self::CadMonochrome),
            "high_contrast" => Ok(Self::HighContrast),
            _ => Err("不支持的轮盘配色".into()),
        }
    }
}

/// Local-only successful-execution counts and the one-time learned layout.
/// The file stores only opaque action fingerprints, never labels or action text.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UsageStore {
    pub version: u32,
    pub enabled: bool,
    pub theme: WheelTheme,
    pub counts: HashMap<String, u64>,
    pub layouts: HashMap<String, Vec<String>>,
    pub active_modes: HashMap<String, String>,
}

impl Default for UsageStore {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: true,
            theme: WheelTheme::TechBlue,
            counts: HashMap::new(),
            layouts: HashMap::new(),
            active_modes: HashMap::new(),
        }
    }
}

pub const PORTABLE_HABITS_FORMAT: &str = "context-wheel-habits";
pub const MAX_PORTABLE_HABITS_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableHabitsFile {
    pub format: String,
    pub version: u32,
    pub data: UsageStore,
}

impl PortableHabitsFile {
    pub fn new(data: UsageStore) -> Self {
        Self {
            format: PORTABLE_HABITS_FORMAT.into(),
            version: 1,
            data,
        }
    }

    pub fn parse(json: &str) -> Result<Self, String> {
        if json.len() > MAX_PORTABLE_HABITS_BYTES {
            return Err("习惯文件超过 1 MB，已拒绝导入".into());
        }
        let file: Self = serde_json::from_str(json).map_err(|_| "习惯文件格式无效或版本不兼容")?;
        if file.format != PORTABLE_HABITS_FORMAT || file.version != 1 {
            return Err("不是受支持的 Context Wheel 习惯文件".into());
        }
        file.data.validate()?;
        Ok(file)
    }
}

pub fn write_portable_habits(path: &std::path::Path, data: &UsageStore) -> Result<(), String> {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        return Err("请选择 .json 文件作为导出位置".into());
    }
    let json = serde_json::to_vec_pretty(&PortableHabitsFile::new(data.clone()))
        .map_err(|error| error.to_string())?;
    std::fs::write(path, json).map_err(|error| format!("保存习惯文件失败：{error}"))
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HabitsImportReport {
    pub matched_profiles: usize,
    pub skipped_profiles: usize,
    pub matched_commands: usize,
    pub skipped_commands: usize,
    pub imported_layouts: usize,
    pub skipped_layouts: usize,
    pub imported_scenes: usize,
    pub skipped_scenes: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdaptiveProfileStatus {
    pub id: String,
    pub name: String,
    pub scope: Scope,
    pub application: Option<String>,
    pub mode: Option<String>,
    pub executions: u64,
    pub learned: bool,
    pub wheel: Vec<Sector>,
    pub outer_rings: Vec<Vec<Sector>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdaptiveStatus {
    pub enabled: bool,
    pub threshold: u64,
    pub theme: WheelTheme,
    pub active_modes: HashMap<String, String>,
    pub profiles: Vec<AdaptiveProfileStatus>,
}

impl UsageStore {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("不支持的本地习惯数据版本".into());
        }
        Ok(())
    }

    /// Merges portable learning data without double-counting reimports.
    /// Profile definitions and executable actions always remain local.
    pub fn merge_portable(&mut self, incoming: &Self, config: &Config) -> HabitsImportReport {
        let professional_profiles: Vec<&Profile> = config
            .profiles
            .iter()
            .filter(|profile| matches!(profile.scope, Scope::Application | Scope::Mode))
            .collect();
        let mut referenced_profile_ids: HashSet<String> = incoming
            .counts
            .keys()
            .filter_map(|key| key.split_once(':').map(|(id, _)| id.to_owned()))
            .chain(incoming.layouts.keys().cloned())
            .collect();
        let mut matched_profile_ids = HashSet::new();
        let mut report = HabitsImportReport::default();

        for (key, count) in &incoming.counts {
            let profile = professional_profiles
                .iter()
                .filter(|profile| key.starts_with(&format!("{}:", profile.id)))
                .max_by_key(|profile| profile.id.len());
            let Some(profile) = profile else {
                report.skipped_commands += 1;
                continue;
            };
            let fingerprint = key
                .strip_prefix(&format!("{}:", profile.id))
                .unwrap_or_default();
            let known_command = profile
                .wheel
                .iter()
                .chain(profile.outer_rings.iter().flatten())
                .any(|sector| sector.enabled && slot_fingerprint(sector) == fingerprint);
            if !known_command {
                report.skipped_commands += 1;
                continue;
            }
            self.counts
                .entry(key.clone())
                .and_modify(|existing| *existing = (*existing).max(*count))
                .or_insert(*count);
            matched_profile_ids.insert(profile.id.clone());
            report.matched_commands += 1;
        }

        for (profile_id, layout) in &incoming.layouts {
            let profile = professional_profiles
                .iter()
                .find(|profile| profile.id == *profile_id);
            let Some(profile) = profile else {
                report.skipped_layouts += 1;
                continue;
            };
            let fingerprints: Vec<String> = profile.wheel.iter().map(slot_fingerprint).collect();
            if profile.wheel.iter().all(|sector| sector.enabled)
                && Self::layout_is_valid(layout, &fingerprints)
            {
                self.layouts.insert(profile_id.clone(), layout.clone());
                matched_profile_ids.insert(profile.id.clone());
                report.imported_layouts += 1;
            } else {
                report.skipped_layouts += 1;
            }
        }

        for (application, mode) in &incoming.active_modes {
            let app_profile = professional_profiles.iter().find(|profile| {
                profile.scope == Scope::Application
                    && profile
                        .application
                        .as_deref()
                        .is_some_and(|app| app.eq_ignore_ascii_case(application))
            });
            let mode_profile = professional_profiles.iter().find(|profile| {
                profile.scope == Scope::Mode
                    && profile
                        .application
                        .as_deref()
                        .is_some_and(|app| app.eq_ignore_ascii_case(application))
                    && profile.mode.as_deref() == Some(mode.as_str())
            });
            if let (Some(app_profile), Some(mode_profile)) = (app_profile, mode_profile) {
                self.active_modes
                    .insert(application.to_lowercase(), mode.clone());
                matched_profile_ids.insert(app_profile.id.clone());
                matched_profile_ids.insert(mode_profile.id.clone());
                report.imported_scenes += 1;
            } else {
                report.skipped_scenes += 1;
            }
        }

        report.matched_profiles = matched_profile_ids.len();
        referenced_profile_ids.retain(|id| !matched_profile_ids.contains(id));
        report.skipped_profiles = referenced_profile_ids.len();
        self.enabled = incoming.enabled;
        self.theme = incoming.theme;
        report
    }

    fn count_key(profile: &Profile, fingerprint: &str) -> String {
        format!("{}:{fingerprint}", profile.id)
    }

    pub fn record_success(&mut self, profile: &Profile, sector: &Sector) {
        if !self.enabled
            || !matches!(profile.scope, Scope::Application | Scope::Mode)
            || !sector.enabled
        {
            return;
        }
        let key = Self::count_key(profile, &slot_fingerprint(sector));
        let value = self.counts.entry(key).or_default();
        *value = value.saturating_add(1);
    }

    fn current_fingerprints(profile: &Profile) -> Vec<String> {
        profile.wheel.iter().map(slot_fingerprint).collect()
    }

    fn layout_is_valid(layout: &[String], fingerprints: &[String]) -> bool {
        if layout.len() != DIRECTIONS.len() || fingerprints.len() != DIRECTIONS.len() {
            return false;
        }
        let layout_set: HashSet<&String> = layout.iter().collect();
        let current_set: HashSet<&String> = fingerprints.iter().collect();
        layout_set.len() == layout.len()
            && current_set.len() == fingerprints.len()
            && layout_set == current_set
    }

    fn apply_layout(profile: &Profile, layout: &[String]) -> Profile {
        let mut adapted = profile.clone();
        for (fingerprint, direction) in layout.iter().zip(EASY_DIRECTIONS) {
            if let Some(sector) = adapted
                .wheel
                .iter_mut()
                .find(|sector| slot_fingerprint(sector) == *fingerprint)
            {
                sector.direction = direction.to_string();
            }
        }
        adapted
    }

    /// Learn a stable layout once, after enough accepted command executions.
    /// The original YAML is never modified; clearing learning restores it.
    pub fn adapt_profile(&mut self, profile: &Profile) -> Profile {
        if !self.enabled || !matches!(profile.scope, Scope::Application | Scope::Mode) {
            return profile.clone();
        }
        // Preserve empty directions as user-defined gaps instead of learning a
        // command back into a slot the user intentionally removed.
        if profile.wheel.iter().any(|sector| !sector.enabled) {
            return profile.clone();
        }
        let fingerprints = Self::current_fingerprints(profile);
        if !Self::layout_is_valid(&fingerprints, &fingerprints) {
            return profile.clone();
        }
        if let Some(layout) = self.layouts.get(&profile.id) {
            if Self::layout_is_valid(layout, &fingerprints) {
                return Self::apply_layout(profile, layout);
            }
        }
        let total = fingerprints.iter().fold(0u64, |total, id| {
            total.saturating_add(
                self.counts
                    .get(&Self::count_key(profile, id))
                    .copied()
                    .unwrap_or_default(),
            )
        });
        if total < ADAPTIVE_LEARNING_THRESHOLD {
            return profile.clone();
        }

        let original_order: HashMap<&str, usize> = profile
            .wheel
            .iter()
            .map(|sector| {
                (
                    sector.direction.as_str(),
                    DIRECTIONS
                        .iter()
                        .position(|d| *d == sector.direction)
                        .unwrap_or(usize::MAX),
                )
            })
            .collect();
        let mut ranked: Vec<(String, u64, usize)> = profile
            .wheel
            .iter()
            .map(|sector| {
                let id = slot_fingerprint(sector);
                let count = self
                    .counts
                    .get(&Self::count_key(profile, &id))
                    .copied()
                    .unwrap_or_default();
                let order = original_order
                    .get(sector.direction.as_str())
                    .copied()
                    .unwrap_or(usize::MAX);
                (id, count, order)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
        // Avoid changing learned directions when the observed frequencies are
        // effectively tied. Keep collecting until one command is clearly used
        // more often than the middle of the distribution.
        if ranked[0].1 < ranked[4].1.saturating_mul(2) {
            return profile.clone();
        }
        let layout: Vec<String> = ranked.into_iter().map(|(id, _, _)| id).collect();
        if !Self::layout_is_valid(&layout, &fingerprints) {
            return profile.clone();
        }
        self.layouts.insert(profile.id.clone(), layout.clone());
        Self::apply_layout(profile, &layout)
    }

    pub fn reset(&mut self) {
        self.counts.clear();
        self.layouts.clear();
    }

    pub fn forget_profile(&mut self, profile_id: &str) {
        let prefix = format!("{profile_id}:");
        self.counts.retain(|key, _| !key.starts_with(&prefix));
        self.layouts.remove(profile_id);
    }

    pub fn status(&self, config: &Config) -> AdaptiveStatus {
        let profiles = config
            .profiles
            .iter()
            .filter(|profile| matches!(profile.scope, Scope::Application | Scope::Mode))
            .map(|profile| {
                let fingerprints: Vec<String> = profile
                    .wheel
                    .iter()
                    .filter(|sector| sector.enabled)
                    .map(slot_fingerprint)
                    .collect();
                let executions = fingerprints.iter().fold(0u64, |executions, id| {
                    executions.saturating_add(
                        self.counts
                            .get(&Self::count_key(profile, id))
                            .copied()
                            .unwrap_or_default(),
                    )
                });
                let learned = self.layouts.get(&profile.id).is_some_and(|layout| {
                    profile.wheel.iter().all(|sector| sector.enabled)
                        && Self::layout_is_valid(layout, &fingerprints)
                });
                AdaptiveProfileStatus {
                    id: profile.id.clone(),
                    name: profile.name.clone(),
                    scope: profile.scope.clone(),
                    application: profile.application.clone(),
                    mode: profile.mode.clone(),
                    executions,
                    learned,
                    wheel: profile.wheel.clone(),
                    outer_rings: profile.outer_rings.clone(),
                }
            })
            .collect();
        AdaptiveStatus {
            enabled: self.enabled,
            threshold: ADAPTIVE_LEARNING_THRESHOLD,
            theme: self.theme,
            active_modes: self.active_modes.clone(),
            profiles,
        }
    }
}

pub fn reorder_profile(profile: &Profile, order: &[String]) -> Result<Profile, String> {
    if !matches!(profile.scope, Scope::Application | Scope::Mode) {
        return Err("只有专业软件 Profile 支持这里的位置调整".into());
    }
    let unique: HashSet<&str> = order.iter().map(String::as_str).collect();
    if order.len() != DIRECTIONS.len()
        || unique.len() != DIRECTIONS.len()
        || DIRECTIONS.iter().any(|d| !unique.contains(d))
    {
        return Err("请为八条命令各指定一个唯一的原始方向".into());
    }
    let mut reordered = profile.clone();
    let mut wheel = Vec::with_capacity(DIRECTIONS.len());
    for (source, destination) in order.iter().zip(DIRECTIONS) {
        let mut sector = profile
            .wheel
            .iter()
            .find(|sector| sector.direction == *source)
            .ok_or("Profile 方向无效")?
            .clone();
        sector.direction = destination.to_string();
        wheel.push(sector);
    }
    reordered.wheel = wheel;
    Ok(reordered)
}

fn slot_fingerprint(sector: &Sector) -> String {
    // FNV-1a is used only as a compact, stable local key, not for security.
    let action = serde_json::to_vec(&sector.action).unwrap_or_default();
    let mut hash = 0xcbf29ce484222325u64;
    for byte in sector
        .label
        .as_bytes()
        .iter()
        .copied()
        .chain([0])
        .chain(action)
    {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
pub fn key_code(k: &str) -> Option<u16> {
    let u = k.to_uppercase();
    Some(match u.as_str() {
        "CTRL" => 0x11,
        "SHIFT" => 0x10,
        "ALT" => 0x12,
        "WIN" => 0x5b,
        "ESC" => 0x1b,
        "ENTER" => 13,
        "TAB" => 9,
        "SPACE" => 32,
        "BACKSPACE" => 8,
        "DELETE" => 46,
        "INSERT" => 45,
        "HOME" => 36,
        "END" => 35,
        "PGUP" => 33,
        "PGDN" => 34,
        "LEFT" => 37,
        "UP" => 38,
        "RIGHT" => 39,
        "DOWN" => 40,
        _ if u.len() == 1 && u.as_bytes()[0].is_ascii_alphanumeric() => u.as_bytes()[0] as u16,
        _ if u.len() >= 2 && u.starts_with('F') => {
            let n: u16 = u[1..].parse().ok()?;
            if (1..=24).contains(&n) {
                0x6f + n
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

/// One step of a typed text action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStep {
    /// Press and release a virtual key (Enter, Esc, Tab).
    Virtual(u16),
    /// Type one UTF-16 unit with KEYEVENTF_UNICODE (bypasses the IME).
    Unicode(u16),
}

/// Translate keystroke text into key steps. AutoCAD macro convention: every
/// leading `^C` becomes one Esc press, so `^C^C_.LINE\n` first cancels any
/// running command. A literal ESC character (\u001b) is also sent as Esc.
pub fn keystroke_steps(text: &str) -> Vec<KeyStep> {
    let mut steps = Vec::new();
    let mut rest = text;
    while rest.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("^c")) {
        steps.push(KeyStep::Virtual(0x1b));
        rest = &rest[2..];
    }
    let mut units = rest.encode_utf16().peekable();
    while let Some(ch) = units.next() {
        match ch {
            0x0d | 0x0a => {
                if ch == 0x0d && units.peek() == Some(&0x0a) {
                    units.next();
                }
                steps.push(KeyStep::Virtual(0x0d));
            }
            0x1b => steps.push(KeyStep::Virtual(0x1b)),
            0x09 => steps.push(KeyStep::Virtual(0x09)),
            _ => steps.push(KeyStep::Unicode(ch)),
        }
    }
    steps
}
pub fn direction(dx: f64, dy: f64, dead: f64) -> Option<usize> {
    direction_for_segments(dx, dy, dead, DIRECTIONS.len())
}
fn direction_for_segments(dx: f64, dy: f64, dead: f64, segments: usize) -> Option<usize> {
    if dx.hypot(dy) < dead {
        None
    } else {
        let step = 360.0 / segments as f64;
        Some((((dx.atan2(-dy).to_degrees() + 360.0 + step / 2.0) % 360.0) / step).floor() as usize)
    }
}
pub fn wheel_dead_zone(ring_count: usize) -> f64 {
    if ring_count <= 1 {
        54.
    } else {
        36.
    }
}
pub fn wheel_slot(dx: f64, dy: f64, dead: f64, profile: &Profile, outer: f64) -> Option<usize> {
    let count = profile.ring_count().clamp(1, 3);
    let radius = dx.hypot(dy);
    let ring_width = (outer - dead) / count as f64;
    let ring_index = (((radius - dead) / ring_width).floor() as usize).min(count - 1);
    let ring = if ring_index == 0 {
        &profile.wheel
    } else {
        profile.outer_rings.get(ring_index - 1)?
    };
    let direction_index = if ring.len() == DIRECTIONS.len() {
        direction(dx, dy, dead)?
    } else {
        direction_for_segments(dx, dy, dead, ring.len())?
    };
    let offset = if ring_index == 0 {
        0
    } else {
        profile.wheel.len()
            + profile.outer_rings[..ring_index - 1]
                .iter()
                .map(Vec::len)
                .sum::<usize>()
    };
    Some(offset + direction_index)
}
pub fn place(
    x: f64,
    y: f64,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    size: f64,
) -> (f64, f64) {
    (
        (x - size / 2.0).clamp(left, (right - size).max(left)),
        (y - size / 2.0).clamp(top, (bottom - size).max(top)),
    )
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum WheelState {
    Idle,
    Tracking,
    Visible,
    Selected,
    Executing,
    Cancelled,
}
pub struct Gesture {
    pub state: WheelState,
    pub origin: (f64, f64),
    pub selected: Option<usize>,
    pub visible: bool,
}
impl Default for Gesture {
    fn default() -> Self {
        Self {
            state: WheelState::Idle,
            origin: (0., 0.),
            selected: None,
            visible: false,
        }
    }
}
impl Gesture {
    pub fn down(&mut self, x: f64, y: f64) {
        self.state = WheelState::Tracking;
        self.origin = (x, y);
        self.selected = None;
        self.visible = false
    }
    pub fn show(&mut self) {
        self.visible = true;
        self.state = if self.selected.is_some() {
            WheelState::Selected
        } else {
            WheelState::Visible
        }
    }
    pub fn moved(&mut self, x: f64, y: f64, dead: f64, profile: &Profile, outer: f64) {
        self.selected = wheel_slot(x - self.origin.0, y - self.origin.1, dead, profile, outer);
        self.state = if self.selected.is_some() {
            WheelState::Selected
        } else if self.visible {
            WheelState::Visible
        } else {
            WheelState::Tracking
        }
    }
    pub fn release(&mut self) -> Option<usize> {
        let s = self.selected;
        self.state = if s.is_some() {
            WheelState::Executing
        } else {
            WheelState::Cancelled
        };
        s
    }
    pub fn reset(&mut self) {
        *self = Self::default()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        let c: Config = serde_yaml::from_str(include_str!("../profiles/default.yaml")).unwrap();
        c.validate().unwrap();
        c
    }
    #[test]
    fn profiles_resolve() {
        let c = config();
        assert_eq!(
            c.resolve(&Context {
                process: "SKETCHUP.EXE".into(),
                desktop: true,
                ..Default::default()
            })
            .id,
            "sketchup.default"
        );
        assert_eq!(
            c.resolve(&Context {
                desktop: true,
                ..Default::default()
            })
            .id,
            "desktop.default"
        );
        assert_eq!(c.resolve(&Context::default()).id, "default.safe")
    }
    #[test]
    fn mode_profile_precedes_application_and_legacy_profiles_keep_one_ring() {
        let mut legacy = config();
        for profile in &mut legacy.profiles {
            profile.outer_rings.clear();
        }
        let legacy_yaml = serde_yaml::to_string(&legacy).unwrap();
        let legacy_loaded: Config = serde_yaml::from_str(&legacy_yaml).unwrap();
        legacy_loaded.validate().unwrap();
        assert!(legacy_loaded
            .profiles
            .iter()
            .all(|p| p.outer_rings.is_empty()));
        assert!(legacy_loaded
            .profiles
            .iter()
            .flat_map(|p| &p.wheel)
            .all(|sector| sector.enabled));

        let mut c = config();
        let base = c
            .profiles
            .iter()
            .find(|p| p.id == "autocad.default")
            .unwrap()
            .clone();
        assert!(base.wheel.iter().all(|sector| sector.enabled));
        c.profiles.push(Profile {
            id: "autocad.model".into(),
            name: "AutoCAD 模型".into(),
            scope: Scope::Mode,
            application: Some("acad.exe".into()),
            mode: Some("model".into()),
            ..base
        });
        c.validate().unwrap();
        assert_eq!(
            c.resolve(&Context {
                process: "ACAD.EXE".into(),
                mode: Some("MODEL".into()),
                ..Default::default()
            })
            .id,
            "autocad.model"
        );
        assert_eq!(
            c.resolve(&Context {
                process: "acad.exe".into(),
                ..Default::default()
            })
            .id,
            "autocad.default"
        );
    }

    #[test]
    fn autocad_default_second_ring_has_sixteen_icons_and_upgrade_only_fills_matching_slots() {
        let defaults = config();
        let default_ring = &defaults
            .profiles
            .iter()
            .find(|p| p.id == "autocad.default")
            .unwrap()
            .outer_rings[0];
        assert_eq!(default_ring.len(), 16);
        assert!(default_ring.iter().all(|sector| sector.icon.is_some()));
        let expected_last_icon = default_ring[15].icon.clone();
        let mut existing = defaults.clone();
        {
            let ring = &mut existing
                .profiles
                .iter_mut()
                .find(|p| p.id == "autocad.default")
                .unwrap()
                .outer_rings[0];
            for sector in ring.iter_mut() {
                sector.icon = None;
                if let Action::Keystroke { text } = &mut sector.action {
                    *text = text.strip_prefix("^C^C").unwrap_or(text).to_string();
                }
            }
            ring[1].action = Action::Keystroke {
                text: "^C^C_.CUSTOM\\n".into(),
            };
            ring[2].icon = Some("ellipse".into());
        }
        let filled = existing.fill_missing_default_autocad_icons(&defaults);
        assert_eq!(filled, 14);
        let ring = &existing
            .profiles
            .iter()
            .find(|p| p.id == "autocad.default")
            .unwrap()
            .outer_rings[0];
        assert_eq!(ring[0].icon.as_deref(), Some("line"));
        assert_eq!(
            ring[1].icon, None,
            "custom command action is left untouched"
        );
        assert_eq!(
            ring[2].icon.as_deref(),
            Some("ellipse"),
            "user-selected icon is preserved"
        );
        assert_eq!(ring[15].icon, expected_last_icon);
    }
    #[test]
    fn autocad_default_includes_editable_line_and_circle_commands_on_outer_ring() {
        let config = config();
        let profile = config
            .profiles
            .iter()
            .find(|p| p.id == "autocad.default")
            .unwrap();
        assert_eq!(profile.ring_count(), 2);
        assert_eq!(profile.sector_at(8).unwrap().label, "直线");
        assert_eq!(profile.sector_at(10).unwrap().label, "圆");
        assert_eq!(profile.sector_at(12).unwrap().label, "多段线");
        assert_eq!(profile.slot_count(), 24);
        assert!(matches!(
            &profile.sector_at(8).unwrap().action,
            Action::Keystroke { text } if text == "^C^C_.LINE\n"
        ));
    }
    #[test]
    fn radial_slot_uses_direction_and_ring_radius() {
        let c = config();
        let profile = c
            .profiles
            .iter()
            .find(|p| p.id == "autocad.default")
            .unwrap();
        assert_eq!(wheel_slot(60., 0., 36., profile, 162.), Some(2));
        assert_eq!(wheel_slot(100., 0., 36., profile, 162.), Some(12));
        assert_eq!(wheel_slot(100., -100., 36., profile, 162.), Some(10));
        assert_eq!(wheel_slot(38.268, -92.388, 36., profile, 162.), Some(9));
        assert_eq!(wheel_slot(30., 0., 36., profile, 162.), None);
        assert_eq!(wheel_slot(300., 0., 36., profile, 162.), Some(12));
        assert_eq!(wheel_dead_zone(1), 54.);
        assert_eq!(wheel_dead_zone(3), 36.);
    }
    fn autocad(c: &mut Config) -> &mut Profile {
        c.profiles
            .iter_mut()
            .find(|p| p.id == "autocad.default")
            .unwrap()
    }
    #[test]
    fn legacy_autocad_outer_rings_expand_without_moving_existing_commands() {
        let mut c = config();
        let old = {
            let profile = c
                .profiles
                .iter_mut()
                .find(|p| p.id == "autocad.default")
                .unwrap();
            let old = profile.outer_rings[0]
                .iter()
                .step_by(2)
                .cloned()
                .collect::<Vec<_>>();
            profile.outer_rings[0] = old.clone();
            old
        };
        c.profiles
            .iter_mut()
            .find(|p| p.id == "sketchup.default")
            .unwrap()
            .outer_rings
            .push(old.clone());
        c.validate().unwrap();
        c.expand_legacy_autocad_outer_rings();
        c.validate().unwrap();
        let migrated = c
            .profiles
            .iter()
            .find(|p| p.id == "autocad.default")
            .unwrap();
        assert_eq!(migrated.outer_rings[0].len(), 16);
        for (old_sector, index) in old.iter().zip([0usize, 2, 4, 6, 8, 10, 12, 14]) {
            assert_eq!(
                migrated.sector_at(8 + index).unwrap().label,
                old_sector.label
            );
        }
        assert!(migrated.outer_rings[0]
            .iter()
            .enumerate()
            .filter(|(index, _)| index % 2 == 1)
            .all(|(_, sector)| !sector.enabled && sector.label.is_empty()));
        assert_eq!(
            c.profiles
                .iter()
                .find(|p| p.id == "sketchup.default")
                .unwrap()
                .outer_rings
                .last()
                .unwrap()
                .len(),
            8,
            "non-CAD legacy rings stay eight-way"
        );
    }
    #[test]
    fn outer_ring_size_is_free_between_one_and_twenty_four() {
        let mut c = config();
        let template = autocad(&mut c).outer_rings[0][0].clone();
        let make = |n: usize| -> Vec<Sector> {
            ring_directions(1, n)
                .into_iter()
                .map(|direction| Sector {
                    direction,
                    ..template.clone()
                })
                .collect()
        };
        for n in [1usize, 3, 5, 8, 12, 16, 20, 24] {
            autocad(&mut c).outer_rings[0] = make(n);
            c.validate().unwrap_or_else(|e| panic!("{n}: {e}"));
        }
        autocad(&mut c).outer_rings[0] = make(25);
        assert!(c.validate().is_err());
        // a 12-slot ring must use position ids, not compass names
        let mut wrong = make(12);
        wrong[0].direction = "N".into();
        autocad(&mut c).outer_rings[0] = wrong;
        assert!(c.validate().is_err());
    }
    #[test]
    fn twelve_slot_outer_ring_hit_testing() {
        let mut c = config();
        let template = autocad(&mut c).outer_rings[0][0].clone();
        autocad(&mut c).outer_rings[0] = ring_directions(1, 12)
            .into_iter()
            .enumerate()
            .map(|(i, direction)| Sector {
                direction,
                label: format!("C{i}"),
                ..template.clone()
            })
            .collect();
        c.validate().unwrap();
        let p = autocad(&mut c).clone();
        assert_eq!(p.slot_count(), 20);
        // straight up on the outer ring -> first outer slot
        assert_eq!(wheel_slot(0., -120., 36., &p, 162.), Some(8));
        // 90 degrees clockwise -> slot 3 of 12
        assert_eq!(wheel_slot(120., 0., 36., &p, 162.), Some(11));
        assert_eq!(p.sector_at(11).unwrap().label, "C3");
        assert_eq!(p.sector_at(19).unwrap().label, "C11");
    }
    #[test]
    fn keystroke_plan_maps_autocad_cancel_prefix_and_control_characters() {
        let steps = keystroke_steps("^C^C_.LINE\n");
        assert_eq!(steps[0], KeyStep::Virtual(0x1b));
        assert_eq!(steps[1], KeyStep::Virtual(0x1b));
        assert_eq!(steps[2], KeyStep::Unicode('_' as u16));
        assert_eq!(*steps.last().unwrap(), KeyStep::Virtual(0x0d));
        assert_eq!(steps.len(), 2 + "_.LINE".len() + 1);
        assert_eq!(
            keystroke_steps("A\r\nB\u{1b}\t"),
            vec![
                KeyStep::Unicode('A' as u16),
                KeyStep::Virtual(0x0d),
                KeyStep::Unicode('B' as u16),
                KeyStep::Virtual(0x1b),
                KeyStep::Virtual(0x09)
            ]
        );
        // ^C only counts at the start; Revit-style text stays literal
        assert_eq!(keystroke_steps("WA").len(), 2);
        assert_eq!(keystroke_steps("a^C")[1], KeyStep::Unicode('^' as u16));
    }
    #[test]
    fn function_and_navigation_keys_are_supported() {
        assert_eq!(key_code("F1"), Some(0x70));
        assert_eq!(key_code("f8"), Some(0x77));
        assert_eq!(key_code("F12"), Some(0x7b));
        assert_eq!(key_code("F24"), Some(0x87));
        assert_eq!(key_code("F25"), None);
        assert_eq!(key_code("F"), Some(b'F' as u16));
        assert_eq!(key_code("PGDN"), Some(34));
        assert_eq!(key_code("FOO"), None);
    }
    #[test]
    fn disabled_sector_cannot_be_executed_or_learned() {
        let mut c = config();
        c.profiles[1].wheel[0].enabled = false;
        assert!(c.validate().is_ok());
        c.profiles[1].wheel[0].label.clear();
        c.profiles[1].wheel[0].action = Action::Hotkey { keys: vec![] };
        assert!(c.validate().is_ok());
        let mut usage = UsageStore::default();
        usage.record_success(&c.profiles[1], &c.profiles[1].wheel[0]);
        assert!(usage.counts.is_empty());
        assert_eq!(usage.adapt_profile(&c.profiles[1]).wheel[0].enabled, false);
    }
    #[test]
    fn eight_directions_and_dead_zone() {
        for (i, (x, y)) in [
            (0., -100.),
            (100., -100.),
            (100., 0.),
            (100., 100.),
            (0., 100.),
            (-100., 100.),
            (-100., 0.),
            (-100., -100.),
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(direction(*x, *y, 32.), Some(i))
        }
        assert_eq!(direction(31.9, 0., 32.), None);
        assert_eq!(direction(32., 0., 32.), Some(2))
    }
    #[test]
    fn edge_and_negative_monitor() {
        assert_eq!(place(-1920., 0., -1920., 0., 0., 1080., 360.), (-1920., 0.));
        assert_eq!(
            place(1920., 1080., 0., 0., 1920., 1080., 360.),
            (1560., 720.)
        )
    }
    #[test]
    fn fast_mark_and_cancel() {
        let c = config();
        let profile = c
            .profiles
            .iter()
            .find(|p| p.id == "desktop.default")
            .unwrap();
        let mut g = Gesture::default();
        g.down(10., 10.);
        g.moved(110., 10., 32., profile, 162.);
        assert!(!g.visible);
        assert_eq!(g.release(), Some(2));
        g.reset();
        g.down(0., 0.);
        g.show();
        g.moved(90., 0., 32., profile, 162.);
        g.moved(0., 0., 32., profile, 162.);
        assert_eq!(g.release(), None);
        g.reset();
        assert_eq!(g.state, WheelState::Idle)
    }
    #[test]
    fn reject_invalid() {
        let mut c = config();
        c.profiles[0].wheel[0].direction = "E".into();
        assert!(c.validate().is_err());
        let mut c = config();
        c.profiles[0].scope = Scope::Object;
        assert!(c.validate().is_err())
    }
    #[test]
    fn adaptive_learning_waits_for_threshold_then_freezes_layout() {
        let c = config();
        let profile = c
            .profiles
            .iter()
            .find(|p| p.id == "sketchup.default")
            .unwrap();
        let mut usage = UsageStore::default();
        let counts = [
            ("移动", 19),
            ("旋转", 8),
            ("推拉", 4),
            ("偏移", 4),
            ("卷尺", 2),
            ("材质", 1),
            ("选择", 1),
        ];
        for (label, count) in counts {
            let sector = profile.wheel.iter().find(|s| s.label == label).unwrap();
            for _ in 0..count {
                usage.record_success(profile, sector);
            }
        }
        assert_eq!(
            usage
                .status(&c)
                .profiles
                .iter()
                .find(|p| p.id == profile.id)
                .unwrap()
                .executions,
            39
        );
        assert!(usage.layouts.get(&profile.id).is_none());
        let sector = profile.wheel.iter().find(|s| s.label == "移动").unwrap();
        usage.record_success(profile, sector);
        let adapted = usage.adapt_profile(profile);
        assert_eq!(
            adapted
                .wheel
                .iter()
                .find(|s| s.direction == "N")
                .unwrap()
                .label,
            "移动"
        );
        assert_eq!(
            adapted
                .wheel
                .iter()
                .find(|s| s.direction == "E")
                .unwrap()
                .label,
            "旋转"
        );

        let locked = usage.layouts[&profile.id].clone();
        let frequent_after_learning = profile.wheel.iter().find(|s| s.label == "推拉").unwrap();
        for _ in 0..100 {
            usage.record_success(profile, frequent_after_learning);
        }
        let still_adapted = usage.adapt_profile(profile);
        assert_eq!(usage.layouts[&profile.id], locked);
        assert_eq!(
            still_adapted
                .wheel
                .iter()
                .find(|s| s.direction == "N")
                .unwrap()
                .label,
            "移动"
        );

        usage.enabled = false;
        let prior_count: u64 = usage.counts.values().copied().sum();
        usage.record_success(profile, &profile.wheel[0]);
        assert_eq!(usage.counts.values().copied().sum::<u64>(), prior_count);
        assert_eq!(
            usage.adapt_profile(profile).wheel[0].direction,
            profile.wheel[0].direction
        );
        usage.reset();
        assert!(usage.layouts.is_empty() && usage.counts.is_empty());
    }
    #[test]
    fn adaptive_counts_are_application_scoped_and_persistable() {
        let c = config();
        let desktop = c
            .profiles
            .iter()
            .find(|p| p.scope == Scope::Desktop)
            .unwrap();
        let mut usage = UsageStore::default();
        usage.record_success(desktop, &desktop.wheel[0]);
        assert!(usage.counts.is_empty());
        let profile = c
            .profiles
            .iter()
            .find(|p| p.id == "sketchup.default")
            .unwrap();
        usage.record_success(profile, &profile.wheel[0]);
        let bytes = serde_json::to_vec(&usage).unwrap();
        let restored: UsageStore = serde_json::from_slice(&bytes).unwrap();
        restored.validate().unwrap();
        assert_eq!(
            restored
                .status(&c)
                .profiles
                .iter()
                .find(|p| p.id == profile.id)
                .unwrap()
                .executions,
            1
        );
    }
    #[test]
    fn tied_frequency_keeps_the_original_direction_memory() {
        let c = config();
        let profile = c
            .profiles
            .iter()
            .find(|p| p.id == "autocad.default")
            .unwrap();
        let mut usage = UsageStore::default();
        for sector in &profile.wheel {
            for _ in 0..5 {
                usage.record_success(profile, sector);
            }
        }
        assert_eq!(
            usage.adapt_profile(profile).wheel[0].direction,
            profile.wheel[0].direction
        );
        assert!(!usage.layouts.contains_key(&profile.id));
    }
    #[test]
    fn manual_reorder_changes_positions_but_preserves_shortcuts() {
        let c = config();
        let profile = c
            .profiles
            .iter()
            .find(|p| p.id == "autocad.default")
            .unwrap();
        let order = vec!["E", "N", "NE", "SE", "S", "SW", "W", "NW"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let reordered = reorder_profile(profile, &order).unwrap();
        let north = reordered
            .wheel
            .iter()
            .find(|sector| sector.direction == "N")
            .unwrap();
        assert_eq!(north.label, "工具选项板");
        assert_eq!(
            north.action,
            profile
                .wheel
                .iter()
                .find(|s| s.direction == "E")
                .unwrap()
                .action
        );
        assert_eq!(
            profile
                .wheel
                .iter()
                .find(|s| s.direction == "N")
                .unwrap()
                .label,
            "特性"
        );
        assert_eq!(
            reordered
                .wheel
                .iter()
                .map(|s| s.direction.as_str())
                .collect::<HashSet<_>>()
                .len(),
            8
        );
        assert!(reorder_profile(profile, &order[..7]).is_err());
    }
    #[test]
    fn wheel_themes_are_valid_and_default_is_tech_blue() {
        assert_eq!(WheelTheme::default(), WheelTheme::TechBlue);
        assert_eq!(
            WheelTheme::parse("deep_blue").unwrap(),
            WheelTheme::DeepBlue
        );
        assert_eq!(WheelTheme::parse("ice_blue").unwrap(), WheelTheme::IceBlue);
        assert_eq!(
            WheelTheme::parse("cad_monochrome").unwrap(),
            WheelTheme::CadMonochrome
        );
        assert_eq!(
            WheelTheme::parse("high_contrast").unwrap(),
            WheelTheme::HighContrast
        );
        assert_eq!(
            serde_json::to_string(&WheelTheme::HighContrast).unwrap(),
            "\"high_contrast\""
        );
        assert!(WheelTheme::parse("red").is_err());
    }

    #[test]
    fn portable_habits_roundtrip_and_reject_unknown_versions() {
        let mut data = UsageStore::default();
        data.enabled = false;
        data.theme = WheelTheme::DeepBlue;
        data.counts
            .insert("autocad.default:0123456789abcdef".into(), 12);
        let json = serde_json::to_string(&PortableHabitsFile::new(data)).unwrap();
        let restored = PortableHabitsFile::parse(&json).unwrap();
        assert_eq!(restored.data.counts["autocad.default:0123456789abcdef"], 12);
        assert!(!restored.data.enabled);
        assert_eq!(restored.data.theme, WheelTheme::DeepBlue);
        assert!(PortableHabitsFile::parse(
            &json.replace("context-wheel-habits", "other-app-habits")
        )
        .is_err());
        assert!(
            PortableHabitsFile::parse(&json.replacen("\"version\":1", "\"version\":2", 1)).is_err()
        );
        assert!(PortableHabitsFile::parse(&" ".repeat(MAX_PORTABLE_HABITS_BYTES + 1)).is_err());
    }

    #[test]
    fn portable_habits_export_writes_one_self_contained_json_file() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("context-wheel-habits-{unique}.json"));
        let mut data = UsageStore::default();
        data.counts
            .insert("autocad.default:0123456789abcdef".into(), 7);
        write_portable_habits(&path, &data).unwrap();
        let json = std::fs::read_to_string(&path).unwrap();
        let imported = PortableHabitsFile::parse(&json).unwrap();
        assert_eq!(imported.data.counts["autocad.default:0123456789abcdef"], 7);
        assert!(write_portable_habits(&path.with_extension("txt"), &data).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn portable_habits_merge_is_compatible_idempotent_and_non_destructive() {
        let mut c = config();
        let autocad = c
            .profiles
            .iter()
            .find(|profile| profile.id == "autocad.default")
            .unwrap()
            .clone();
        c.profiles.push(Profile {
            id: "autocad.model".into(),
            name: "AutoCAD 模型".into(),
            scope: Scope::Mode,
            application: Some("acad.exe".into()),
            mode: Some("model".into()),
            ..autocad.clone()
        });
        c.validate().unwrap();

        let fingerprint = slot_fingerprint(&autocad.wheel[0]);
        let key = format!("{}:{fingerprint}", autocad.id);
        let layout: Vec<String> = autocad.wheel.iter().rev().map(slot_fingerprint).collect();
        let mut incoming = UsageStore::default();
        incoming.enabled = false;
        incoming.theme = WheelTheme::DeepBlue;
        incoming.counts.insert(key.clone(), 40);
        incoming
            .counts
            .insert(format!("{}:ffffffffffffffff", autocad.id), 9);
        incoming
            .counts
            .insert("missing.profile:0123456789abcdef".into(), 3);
        incoming.layouts.insert(autocad.id.clone(), layout.clone());
        incoming
            .layouts
            .insert("missing.profile".into(), layout.clone());
        incoming
            .active_modes
            .insert("acad.exe".into(), "model".into());

        let json = serde_json::to_string(&PortableHabitsFile::new(incoming.clone())).unwrap();
        assert!(
            !json.contains("特性"),
            "transfer does not expose command labels"
        );
        let imported = PortableHabitsFile::parse(&json).unwrap().data;
        let mut local = UsageStore::default();
        local.counts.insert(key.clone(), 60);
        let first_report = local.merge_portable(&imported, &c);
        let first_counts = local.counts.clone();
        let second_report = local.merge_portable(&imported, &c);

        assert_eq!(local.counts[&key], 60, "higher local count is preserved");
        assert_eq!(
            local.counts, first_counts,
            "reimport does not inflate counts"
        );
        assert_eq!(local.layouts[&autocad.id], layout);
        assert_eq!(local.active_modes["acad.exe"], "model");
        assert!(!local.enabled);
        assert_eq!(local.theme, WheelTheme::DeepBlue);
        assert_eq!(first_report.matched_commands, 1);
        assert_eq!(first_report.skipped_commands, 2);
        assert_eq!(first_report.imported_layouts, 1);
        assert_eq!(first_report.skipped_layouts, 1);
        assert_eq!(first_report.imported_scenes, 1);
        assert_eq!(first_report.skipped_profiles, 1);
        assert_eq!(second_report.matched_commands, 1);
    }
}
