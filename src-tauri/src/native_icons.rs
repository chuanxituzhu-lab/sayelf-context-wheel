use crate::core::{directions_for_ring, Action, Profile};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, TryLockError},
};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    System::LibraryLoader::{
        FindResourceW, LoadLibraryExW, LoadResource, LockResource, SizeofResource,
        LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
    },
    System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE,
        KEY_READ,
    },
};
use zip::ZipArchive;

const MAX_CUIX_BYTES: u64 = 96 * 1024 * 1024;
const MAX_TOTAL_CUIX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_XML_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_XML_BYTES: u64 = 16 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TOTAL_IMAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_CACHED_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4096;
const MAX_ICON_COUNT: usize = 256;
const MAX_AUTOCAD_RESOURCE_BYTES: u64 = 64 * 1024 * 1024;

static AUTOCAD_IMAGES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
static SCANNED_INSTALL_COMMANDS: OnceLock<Mutex<HashMap<PathBuf, HashSet<String>>>> =
    OnceLock::new();

#[derive(Default)]
struct CommandImage {
    command: String,
    small: Option<String>,
    large: Option<String>,
}

#[derive(Clone, Copy)]
enum Capture {
    Command,
}

pub fn warm() {
    if cfg!(debug_assertions) && std::env::var_os("CWE_TEST_CUIX").is_some() {
        let _ = icon_cache();
        return;
    }
    let Ok(config): Result<crate::core::Config, _> =
        serde_yaml::from_str(include_str!("../profiles/default.yaml"))
    else {
        let _ = icon_cache();
        return;
    };
    let Some(profile) = config
        .profiles
        .iter()
        .find(|profile| profile.id == "autocad.default")
    else {
        let _ = icon_cache();
        return;
    };
    let requested = profile_commands(profile);
    let roots = discover_registry_autocad_roots();
    let mut candidates = discover_paths();
    let mut resource_paths = Vec::new();
    for root in &roots {
        add_cuix_files(&root.join("Support"), &mut candidates);
        add_cuix_files(&root.join("UserDataCache").join("Support"), &mut candidates);
        add_autocad_resource_files(root, &mut resource_paths);
    }
    candidates.sort();
    candidates.dedup();
    resource_paths.sort();
    resource_paths.dedup();
    {
        let mut scanned = SCANNED_INSTALL_COMMANDS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for root in &roots {
            scanned
                .entry(root.clone())
                .or_default()
                .extend(requested.iter().cloned());
        }
    }
    let discovered = read_candidates(candidates, &resource_paths, Some(&requested));
    let mut cache = icon_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    merge_images(&mut cache, discovered);
}

pub fn for_profile(process: &str, process_path: &str, profile: &Profile) -> Vec<Option<String>> {
    let mut result = vec![None; profile.slot_count()];
    if !is_autocad(process) {
        return result;
    }
    scan_process_install(process_path, profile);
    let Some(cache) = AUTOCAD_IMAGES.get() else {
        return result;
    };
    let images = match cache.try_lock() {
        Ok(images) => images,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(TryLockError::WouldBlock) => return result,
    };
    let rings = std::iter::once(&profile.wheel).chain(profile.outer_rings.iter());
    let mut offset = 0;
    for (ring_index, ring) in rings.enumerate() {
        for (direction_index, direction) in directions_for_ring(ring_index, ring).iter().enumerate()
        {
            let Some(sector) = ring.iter().find(|sector| sector.direction == *direction) else {
                continue;
            };
            if !sector.enabled || sector.icon.is_some() {
                continue;
            }
            let Action::Keystroke { text } = &sector.action else {
                continue;
            };
            let Some(command) = command_name(text) else {
                continue;
            };
            if let Some(image) = images.get(&command) {
                result[offset + direction_index] = Some(image.clone());
            }
        }
        offset += ring.len();
    }
    result
}

fn icon_cache() -> &'static Mutex<HashMap<String, String>> {
    AUTOCAD_IMAGES.get_or_init(|| {
        let initial = if cfg!(debug_assertions) && std::env::var_os("CWE_TEST_CUIX").is_some() {
            discover_autocad_images()
        } else {
            HashMap::new()
        };
        Mutex::new(initial)
    })
}

fn scan_process_install(process_path: &str, profile: &Profile) {
    if process_path.trim().is_empty()
        || (cfg!(debug_assertions) && std::env::var_os("CWE_TEST_CUIX").is_some())
    {
        return;
    }
    let Some(executable_dir) = Path::new(process_path).parent() else {
        return;
    };
    let requested = profile_commands(profile);
    if requested.is_empty() {
        return;
    }
    let executable_dir = executable_dir.to_path_buf();
    let scan_state = SCANNED_INSTALL_COMMANDS.get_or_init(|| Mutex::new(HashMap::new()));
    let missing = {
        let mut scanned = scan_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let missing = requested
            .difference(scanned.get(&executable_dir).unwrap_or(&HashSet::new()))
            .cloned()
            .collect::<HashSet<_>>();
        scanned
            .entry(executable_dir.clone())
            .or_default()
            .extend(missing.iter().cloned());
        missing
    };
    if missing.is_empty() {
        return;
    }

    // Read once for the active command set so native images are ready on the
    // very first visible wheel frame, rather than racing an async scan.
    let mut candidates = discover_paths();
    let mut resource_paths = Vec::new();
    for root in
        std::iter::once(executable_dir.as_path()).chain(executable_dir.ancestors().skip(1).take(3))
    {
        add_cuix_files(&root.join("Support"), &mut candidates);
        add_cuix_files(&root.join("UserDataCache").join("Support"), &mut candidates);
        add_autocad_resource_files(root, &mut resource_paths);
    }
    candidates.sort();
    candidates.dedup();
    resource_paths.sort();
    resource_paths.dedup();
    let discovered = read_candidates(candidates, &resource_paths, Some(&missing));
    if !discovered.is_empty() {
        let mut cache = icon_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        merge_images(&mut cache, discovered);
    }
}

fn profile_commands(profile: &Profile) -> HashSet<String> {
    std::iter::once(&profile.wheel)
        .chain(profile.outer_rings.iter())
        .flat_map(|ring| ring.iter())
        .filter(|sector| sector.enabled)
        .filter_map(|sector| match &sector.action {
            Action::Keystroke { text } => command_name(text),
            _ => None,
        })
        .collect()
}

fn merge_images(cache: &mut HashMap<String, String>, discovered: HashMap<String, String>) {
    let mut cached_bytes = cache.values().map(String::len).sum::<usize>();
    for (command, image) in discovered {
        if cache.contains_key(&command) {
            continue;
        }
        if cached_bytes.saturating_add(image.len()) > MAX_CACHED_IMAGE_BYTES {
            break;
        }
        cached_bytes += image.len();
        cache.insert(command, image);
    }
}

fn read_candidates(
    candidates: impl IntoIterator<Item = PathBuf>,
    resource_paths: &[PathBuf],
    requested_commands: Option<&HashSet<String>>,
) -> HashMap<String, String> {
    let mut commands = HashMap::new();
    let mut total_cuix_bytes = 0u64;
    let mut total_icon_bytes = 0usize;
    for path in candidates {
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.len() == 0 || metadata.len() > MAX_CUIX_BYTES {
            continue;
        }
        if total_cuix_bytes.saturating_add(metadata.len()) > MAX_TOTAL_CUIX_BYTES {
            break;
        }
        total_cuix_bytes += metadata.len();
        let Ok(images) = read_cuix_file(&path, resource_paths, requested_commands) else {
            continue;
        };
        for (command, image) in images {
            if commands.contains_key(&command) {
                continue;
            }
            if total_icon_bytes.saturating_add(image.len()) > MAX_CACHED_IMAGE_BYTES {
                return commands;
            }
            total_icon_bytes += image.len();
            commands.insert(command, image);
        }
    }
    commands
}

fn is_autocad(process: &str) -> bool {
    let name = process.rsplit(['\\', '/']).next().unwrap_or(process);
    name.to_ascii_lowercase().starts_with("acad") && name.to_ascii_lowercase().ends_with(".exe")
}

fn command_name(raw: &str) -> Option<String> {
    let mut command = raw.trim();
    if command
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("$M="))
    {
        command = command.rsplit_once(',')?.1.trim();
        command = command.trim_end_matches(')').trim();
    }
    loop {
        if command
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("^c"))
        {
            command = command[2..].trim_start();
        } else {
            break;
        }
    }
    loop {
        let Some(first) = command.chars().next() else {
            return None;
        };
        if first == '_' || first == '.' {
            command = &command[first.len_utf8()..];
        } else {
            break;
        }
    }
    let token = command
        .split(|character: char| character.is_whitespace() || matches!(character, ';' | '\\'))
        .next()?
        .trim();
    if token.is_empty() || !token.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return None;
    }
    Some(token.to_ascii_uppercase())
}

fn local_name(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

fn image_reference(tag: &BytesStart<'_>) -> Option<String> {
    tag.attributes()
        .filter_map(Result::ok)
        .find(|attribute| {
            attribute
                .key
                .local_name()
                .as_ref()
                .eq_ignore_ascii_case("name")
        })
        .and_then(|attribute| {
            attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
        })
        .map(|value| value.into_owned())
        .filter(|value| !value.trim().is_empty())
}

fn parse_cui(xml: &[u8]) -> Vec<CommandImage> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut current: Option<CommandImage> = None;
    let mut capture: Option<Capture> = None;
    let mut records = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(tag)) => match local_name(tag.local_name().as_ref()).as_str() {
                "menumacro" => current = Some(CommandImage::default()),
                "command" if current.is_some() => capture = Some(Capture::Command),
                "smallimage" => {
                    if let (Some(record), Some(image)) = (current.as_mut(), image_reference(&tag)) {
                        record.small = Some(image);
                    }
                }
                "largeimage" => {
                    if let (Some(record), Some(image)) = (current.as_mut(), image_reference(&tag)) {
                        record.large = Some(image);
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(tag)) => match local_name(tag.local_name().as_ref()).as_str() {
                "smallimage" => {
                    if let (Some(record), Some(image)) = (current.as_mut(), image_reference(&tag)) {
                        record.small = Some(image);
                    }
                }
                "largeimage" => {
                    if let (Some(record), Some(image)) = (current.as_mut(), image_reference(&tag)) {
                        record.large = Some(image);
                    }
                }
                _ => {}
            },
            Ok(Event::Text(text)) if capture.is_some() => {
                if let Some(record) = current.as_mut() {
                    if let Ok(unescaped) = quick_xml::escape::unescape(text.as_ref()) {
                        record.command.push_str(&unescaped);
                    } else {
                        record.command.push_str(text.as_ref());
                    }
                }
            }
            Ok(Event::CData(text)) if capture.is_some() => {
                if let Some(record) = current.as_mut() {
                    record.command.push_str(text.as_ref());
                }
            }
            Ok(Event::End(tag)) => match local_name(tag.local_name().as_ref()).as_str() {
                "command" => capture = None,
                "menumacro" => {
                    if let Some(record) = current.take() {
                        if command_name(&record.command).is_some()
                            && (record.small.is_some() || record.large.is_some())
                        {
                            records.push(record);
                        }
                    }
                    capture = None;
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => return Vec::new(),
            _ => {}
        }
        buffer.clear();
    }
    records
}

fn image_key(reference: &str) -> String {
    let normalized = reference.trim().replace('\\', "/");
    let name = normalized.rsplit('/').next().unwrap_or(&normalized);
    Path::new(name)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(reference)
        .to_ascii_lowercase()
}

fn image_mime(name: &str) -> Option<&'static str> {
    match Path::new(name)
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "bmp" => Some("image/bmp"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        _ => None,
    }
}

fn valid_image(name: &str, bytes: &[u8]) -> bool {
    match image_mime(name) {
        Some("image/bmp") => bytes.len() >= 54 && bytes.starts_with(b"BM"),
        Some("image/png") => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        Some("image/gif") => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        Some("image/jpeg") => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        _ => false,
    }
}

fn bmp_to_rgba(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if bytes.get(..2)? != b"BM" || bytes.len() < 54 {
        return None;
    }
    let declared_size = u32::from_le_bytes(bytes.get(2..6)?.try_into().ok()?) as usize;
    let pixel_offset = u32::from_le_bytes(bytes.get(10..14)?.try_into().ok()?) as usize;
    let dib_size = u32::from_le_bytes(bytes.get(14..18)?.try_into().ok()?) as usize;
    if dib_size < 40 || 14usize.checked_add(dib_size)? > bytes.len() {
        return None;
    }
    if declared_size != 0 && declared_size > bytes.len() {
        return None;
    }
    let width = i32::from_le_bytes(bytes.get(18..22)?.try_into().ok()?);
    let signed_height = i32::from_le_bytes(bytes.get(22..26)?.try_into().ok()?);
    let planes = u16::from_le_bytes(bytes.get(26..28)?.try_into().ok()?);
    let bits_per_pixel = u16::from_le_bytes(bytes.get(28..30)?.try_into().ok()?);
    let compression = u32::from_le_bytes(bytes.get(30..34)?.try_into().ok()?);
    if width <= 0
        || signed_height == 0
        || planes != 1
        || !matches!(bits_per_pixel, 24 | 32)
        || compression != 0
    {
        return None;
    }
    let width = width as usize;
    let height = signed_height.unsigned_abs() as usize;
    if width > 256 || height > 256 {
        return None;
    }
    let bytes_per_pixel = (bits_per_pixel / 8) as usize;
    let row_stride = width
        .checked_mul(bytes_per_pixel)?
        .checked_add(3)?
        .checked_div(4)?
        .checked_mul(4)?;
    let pixel_bytes = row_stride.checked_mul(height)?;
    let pixel_end = pixel_offset.checked_add(pixel_bytes)?;
    if pixel_offset < 14 + dib_size
        || pixel_end > bytes.len()
        || (declared_size != 0 && pixel_end > declared_size)
        || width.checked_mul(height)?.checked_mul(4)? > MAX_IMAGE_BYTES as usize
    {
        return None;
    }

    let mut rgba = Vec::with_capacity(width * height * 4);
    for output_y in 0..height {
        let source_y = if signed_height > 0 {
            height - output_y - 1
        } else {
            output_y
        };
        let row_start = pixel_offset.checked_add(source_y.checked_mul(row_stride)?)?;
        for x in 0..width {
            let pixel = row_start.checked_add(x.checked_mul(bytes_per_pixel)?)?;
            rgba.extend_from_slice(&[
                *bytes.get(pixel + 2)?,
                *bytes.get(pixel + 1)?,
                *bytes.get(pixel)?,
                if bits_per_pixel == 32 {
                    *bytes.get(pixel + 3)?
                } else {
                    255
                },
            ]);
        }
    }
    if bits_per_pixel == 32 && rgba.chunks_exact(4).all(|pixel| pixel[3] == 0) {
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
    }
    Some((width, height, rgba))
}

fn image_data_uri(name: &str, bytes: &[u8]) -> Option<String> {
    let mime = image_mime(name)?;
    if !valid_image(name, bytes) {
        return None;
    }
    if mime == "image/bmp" {
        let (width, height, rgba) = bmp_to_rgba(bytes)?;
        return png_data_uri(&rgba, width, height);
    }
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn archive_images<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    references: &HashSet<String>,
) -> HashMap<String, String> {
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return HashMap::new();
    }
    let mut results = HashMap::new();
    let mut total_bytes = 0usize;
    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else {
            continue;
        };
        let name = entry.name().to_string();
        if image_mime(&name).is_none() {
            continue;
        }
        let stem = image_key(&name);
        if !references.contains(&stem)
            || entry.size() == 0
            || entry.size() > MAX_IMAGE_BYTES
            || results.len() >= MAX_ICON_COUNT
        {
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        if entry
            .by_ref()
            .take(MAX_IMAGE_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() as u64 > MAX_IMAGE_BYTES
            || !valid_image(&name, &bytes)
            || total_bytes.saturating_add(bytes.len()) > MAX_TOTAL_IMAGE_BYTES
        {
            continue;
        }
        let Some(data_uri) = image_data_uri(&name, &bytes) else {
            continue;
        };
        total_bytes += bytes.len();
        results.insert(stem, data_uri);
    }
    results
}

#[cfg(test)]
fn read_cuix<R: Read + Seek>(reader: R) -> Result<HashMap<String, String>, String> {
    read_cuix_with_resources(reader, &[], None)
}

fn read_cuix_with_resources<R: Read + Seek>(
    reader: R,
    resource_paths: &[PathBuf],
    requested_commands: Option<&HashSet<String>>,
) -> Result<HashMap<String, String>, String> {
    let mut archive = ZipArchive::new(reader).map_err(|error| error.to_string())?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err("CUIx 包含过多条目".into());
    }
    let mut records = Vec::new();
    let mut total_xml_bytes = 0u64;
    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else {
            continue;
        };
        let is_cui = Path::new(entry.name())
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("cui"));
        if !is_cui
            || entry.size() == 0
            || entry.size() > MAX_XML_BYTES
            || total_xml_bytes.saturating_add(entry.size()) > MAX_TOTAL_XML_BYTES
        {
            continue;
        }
        total_xml_bytes += entry.size();
        let mut xml = Vec::with_capacity(entry.size() as usize);
        if entry
            .by_ref()
            .take(MAX_XML_BYTES + 1)
            .read_to_end(&mut xml)
            .is_ok()
            && xml.len() as u64 <= MAX_XML_BYTES
        {
            records.extend(parse_cui(&xml));
        }
    }
    if let Some(requested) = requested_commands {
        records.retain(|record| {
            command_name(&record.command).is_some_and(|command| requested.contains(&command))
        });
    }
    let references = records
        .iter()
        .flat_map(|record| record.small.iter().chain(record.large.iter()))
        .map(|reference| image_key(reference))
        .collect::<HashSet<_>>();
    if references.is_empty() {
        return Ok(HashMap::new());
    }
    let mut reference_names = HashMap::new();
    for reference in records
        .iter()
        .flat_map(|record| record.small.iter().chain(record.large.iter()))
    {
        reference_names
            .entry(image_key(reference))
            .or_insert_with(|| reference.clone());
    }
    let mut images = archive_images(&mut archive, &references);
    if images.len() < references.len() {
        let unresolved = reference_names
            .into_iter()
            .filter(|(key, _)| !images.contains_key(key))
            .collect::<HashMap<_, _>>();
        for path in resource_paths {
            if images.len() >= MAX_ICON_COUNT || unresolved.is_empty() {
                break;
            }
            for (key, image) in read_native_resource_images(path, &unresolved) {
                if images.len() >= MAX_ICON_COUNT {
                    break;
                }
                images.entry(key).or_insert(image);
            }
        }
    }
    let mut commands = HashMap::new();
    for record in records {
        let Some(command) = command_name(&record.command) else {
            continue;
        };
        let image = record
            .large
            .iter()
            .chain(record.small.iter())
            .find_map(|reference| images.get(&image_key(reference)));
        if let Some(image) = image {
            commands.entry(command).or_insert_with(|| image.clone());
        }
    }
    Ok(commands)
}

fn read_cuix_file(
    path: &Path,
    resource_paths: &[PathBuf],
    requested_commands: Option<&HashSet<String>>,
) -> Result<HashMap<String, String>, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_CUIX_BYTES {
        return Err("CUIx 文件超过本地读取上限".into());
    }
    read_cuix_with_resources(
        File::open(path).map_err(|error| error.to_string())?,
        resource_paths,
        requested_commands,
    )
}

fn read_native_resource_images(
    path: &Path,
    references: &HashMap<String, String>,
) -> HashMap<String, String> {
    if references.is_empty()
        || fs::metadata(path)
            .map(|metadata| metadata.len() == 0 || metadata.len() > MAX_AUTOCAD_RESOURCE_BYTES)
            .unwrap_or(true)
    {
        return HashMap::new();
    }
    let wide_path = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let module = unsafe {
        LoadLibraryExW(
            wide_path.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    };
    if module.is_null() {
        return HashMap::new();
    }
    let _module = ResourceModule(module);
    let mut images = HashMap::new();
    let mut total_bytes = 0usize;
    for (key, name) in references {
        if images.len() >= MAX_ICON_COUNT || total_bytes >= MAX_TOTAL_IMAGE_BYTES {
            break;
        }
        if !name.to_ascii_uppercase().starts_with("RCDATA_") {
            continue;
        }
        let wide_name = name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let resource = unsafe { FindResourceW(module, wide_name.as_ptr(), 10usize as *const u16) };
        if resource.is_null() {
            continue;
        }
        let size = unsafe { SizeofResource(module, resource) } as usize;
        if size < 16 || size > MAX_IMAGE_BYTES as usize {
            continue;
        }
        let loaded = unsafe { LoadResource(module, resource) };
        if loaded.is_null() {
            continue;
        }
        let data = unsafe { LockResource(loaded) };
        if data.is_null() {
            continue;
        }
        let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) };
        let Some(image) = autocad_resource_png(bytes) else {
            continue;
        };
        total_bytes = total_bytes.saturating_add(image.len());
        if total_bytes > MAX_TOTAL_IMAGE_BYTES {
            break;
        }
        images.insert(key.clone(), image);
    }
    images
}

struct ResourceModule(HMODULE);

impl Drop for ResourceModule {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}

fn autocad_resource_png(resource: &[u8]) -> Option<String> {
    let (width, height, ifd_offset, little_endian) = tiff_dimensions(resource)?;
    let compressed = resource.get(8..ifd_offset)?;
    let pixel_bytes = width.checked_mul(height)?.checked_mul(4)?;
    if pixel_bytes == 0 || pixel_bytes > MAX_IMAGE_BYTES as usize {
        return None;
    }
    let mut decoder = ZlibDecoder::new(compressed);
    let mut rgba = Vec::with_capacity(pixel_bytes);
    decoder
        .by_ref()
        .take((pixel_bytes + 1) as u64)
        .read_to_end(&mut rgba)
        .ok()?;
    if rgba.len() != pixel_bytes || !little_endian {
        return None;
    }
    png_data_uri(&rgba, width, height)
}

fn tiff_dimensions(resource: &[u8]) -> Option<(usize, usize, usize, bool)> {
    let little_endian = match resource.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if read_tiff_u16(resource, 2, little_endian)? != 42 {
        return None;
    }
    let ifd_offset = read_tiff_u32(resource, 4, little_endian)? as usize;
    let count = read_tiff_u16(resource, ifd_offset, little_endian)? as usize;
    if count > 128 {
        return None;
    }
    let mut width = None;
    let mut height = None;
    for index in 0..count {
        let offset = ifd_offset.checked_add(2 + index.checked_mul(12)?)?;
        let tag = read_tiff_u16(resource, offset, little_endian)?;
        if !matches!(tag, 256 | 257) {
            continue;
        }
        let field_type = read_tiff_u16(resource, offset + 2, little_endian)?;
        let value_count = read_tiff_u32(resource, offset + 4, little_endian)?;
        if value_count != 1 {
            continue;
        }
        let value = match field_type {
            3 => read_tiff_u16(resource, offset + 8, little_endian)? as usize,
            4 => read_tiff_u32(resource, offset + 8, little_endian)? as usize,
            _ => continue,
        };
        if tag == 256 {
            width = Some(value);
        } else {
            height = Some(value);
        }
    }
    let width = width?;
    let height = height?;
    if width == 0 || height == 0 || width > 64 || height > 64 {
        return None;
    }
    Some((width, height, ifd_offset, little_endian))
}

fn read_tiff_u16(bytes: &[u8], offset: usize, little_endian: bool) -> Option<u16> {
    let value: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(if little_endian {
        u16::from_le_bytes(value)
    } else {
        u16::from_be_bytes(value)
    })
}

fn read_tiff_u32(bytes: &[u8], offset: usize, little_endian: bool) -> Option<u32> {
    let value: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(if little_endian {
        u32::from_le_bytes(value)
    } else {
        u32::from_be_bytes(value)
    })
}

fn png_data_uri(rgba: &[u8], width: usize, height: usize) -> Option<String> {
    if width == 0 || height == 0 || rgba.len() != width.checked_mul(height)?.checked_mul(4)? {
        return None;
    }
    let mut scanlines = Vec::with_capacity(height.checked_mul(width * 4 + 1)?);
    for row in rgba.chunks_exact(width * 4) {
        scanlines.push(0);
        scanlines.extend_from_slice(row);
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&scanlines).ok()?;
    let compressed = encoder.finish().ok()?;
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    append_png_chunk(&mut png, b"IHDR", &header);
    append_png_chunk(&mut png, b"IDAT", &compressed);
    append_png_chunk(&mut png, b"IEND", &[]);
    Some(format!("data:image/png;base64,{}", STANDARD.encode(png)))
}

fn append_png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let mut crc = !0u32;
    for byte in kind.iter().chain(data) {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    png.extend_from_slice(&(!crc).to_be_bytes());
}

fn child_directories(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .collect()
}

fn add_cuix_files(directory: &Path, candidates: &mut Vec<PathBuf>) {
    if candidates.len() >= 128 {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    candidates.extend(entries.filter_map(Result::ok).filter_map(|entry| {
        let path = entry.path();
        let is_file = entry.file_type().ok().is_some_and(|kind| kind.is_file());
        (is_file
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("cuix")))
        .then_some(path)
    }));
    candidates.truncate(128);
}

fn add_autocad_roaming_paths(root: &Path, candidates: &mut Vec<PathBuf>) {
    for product in child_directories(root) {
        let product_name = product.file_name().unwrap_or_default().to_string_lossy();
        if !product_name.to_ascii_lowercase().contains("autocad") {
            continue;
        }
        for release in child_directories(&product) {
            for language in child_directories(&release) {
                for child in child_directories(&language) {
                    if child
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("support"))
                    {
                        add_cuix_files(&child, candidates);
                    }
                }
            }
        }
    }
}

fn add_autocad_install_paths(root: &Path, candidates: &mut Vec<PathBuf>) {
    let autodesk = root.join("Autodesk");
    for product in child_directories(&autodesk) {
        let name = product.file_name().unwrap_or_default().to_string_lossy();
        if !name.to_ascii_lowercase().contains("autocad") {
            continue;
        }
        for directory in [
            product.join("UserDataCache").join("Support"),
            product.join("Support"),
        ] {
            add_cuix_files(&directory, candidates);
        }
    }
}

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn open_registry_key(parent: HKEY, path: &str) -> Option<RegistryKey> {
    let path = path
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut key = std::ptr::null_mut();
    let status = unsafe { RegOpenKeyExW(parent, path.as_ptr(), 0, KEY_READ, &mut key) };
    (status == 0 && !key.is_null()).then_some(RegistryKey(key))
}

fn registry_subkeys(key: HKEY) -> Vec<String> {
    let mut names = Vec::new();
    for index in 0..512 {
        let mut buffer = [0u16; 260];
        let mut length = buffer.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key,
                index,
                buffer.as_mut_ptr(),
                &mut length,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status == 259 {
            break;
        }
        if status != 0 {
            continue;
        }
        names.push(String::from_utf16_lossy(&buffer[..length as usize]));
    }
    names
}

fn registry_string_value(key: HKEY, name: &str) -> Option<String> {
    let name = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut value_type = 0u32;
    let mut byte_len = 0u32;
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut value_type,
            std::ptr::null_mut(),
            &mut byte_len,
        )
    };
    if status != 0 || byte_len < 2 || byte_len > 64 * 1024 {
        return None;
    }
    let mut buffer = vec![0u16; byte_len.div_ceil(2) as usize];
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut value_type,
            buffer.as_mut_ptr().cast::<u8>(),
            &mut byte_len,
        )
    };
    if status != 0 || !matches!(value_type, 1 | 2) {
        return None;
    }
    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    let value = String::from_utf16_lossy(&buffer[..length]);
    (!value.trim().is_empty()).then_some(value)
}

fn discover_registry_autocad_roots() -> Vec<PathBuf> {
    let Some(autocad) = open_registry_key(HKEY_LOCAL_MACHINE, "SOFTWARE\\Autodesk\\AutoCAD") else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    for version in registry_subkeys(autocad.0) {
        let Some(version_key) = open_registry_key(autocad.0, &version) else {
            continue;
        };
        for product in registry_subkeys(version_key.0) {
            let Some(product_key) = open_registry_key(version_key.0, &product) else {
                continue;
            };
            let install = ["GlobUPILocation", "AcadLocation", "InstallLocation"]
                .iter()
                .find_map(|name| registry_string_value(product_key.0, name));
            if let Some(install) = install {
                let root = PathBuf::from(install);
                if root.join("acadbtn.xmx").is_file() || root.join("acadbtn_light.xmx").is_file() {
                    roots.push(root);
                }
            }
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

fn add_autocad_resource_files(root: &Path, paths: &mut Vec<PathBuf>) {
    for directory in [root.to_path_buf(), root.join("Support"), root.join("zh-CN")] {
        for name in ["acadbtn.xmx", "acadbtn_light.xmx"] {
            if paths.len() >= 16 {
                return;
            }
            let path = directory.join(name);
            if fs::metadata(&path).is_ok_and(|metadata| {
                metadata.is_file()
                    && metadata.len() > 0
                    && metadata.len() <= MAX_AUTOCAD_RESOURCE_BYTES
            }) {
                paths.push(path);
            }
        }
    }
}

fn discover_paths() -> Vec<PathBuf> {
    if cfg!(debug_assertions) {
        if let Some(test_cuix) = std::env::var_os("CWE_TEST_CUIX") {
            return vec![PathBuf::from(test_cuix)];
        }
    }
    let mut paths = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        add_autocad_roaming_paths(&PathBuf::from(appdata).join("Autodesk"), &mut paths);
    }
    for variable in ["PROGRAMW6432", "PROGRAMFILES", "PROGRAMFILES(X86)"] {
        if let Some(root) = std::env::var_os(variable) {
            add_autocad_install_paths(Path::new(&root), &mut paths);
        }
    }
    paths.sort();
    paths.dedup();
    paths.truncate(128);
    paths
}

fn discover_autocad_images() -> HashMap<String, String> {
    read_candidates(discover_paths(), &[], None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    fn cuix_fixture() -> Vec<u8> {
        let mut bmp = vec![0u8; 58];
        bmp[..2].copy_from_slice(b"BM");
        bmp[2..6].copy_from_slice(&(58u32).to_le_bytes());
        bmp[10..14].copy_from_slice(&(54u32).to_le_bytes());
        bmp[14..18].copy_from_slice(&(40u32).to_le_bytes());
        bmp[18..22].copy_from_slice(&(1i32).to_le_bytes());
        bmp[22..26].copy_from_slice(&(1i32).to_le_bytes());
        bmp[26..28].copy_from_slice(&(1u16).to_le_bytes());
        bmp[28..30].copy_from_slice(&(24u16).to_le_bytes());
        bmp[34..38].copy_from_slice(&(4u32).to_le_bytes());
        bmp[54..58].copy_from_slice(&[0x40, 0x80, 0xff, 0]);
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file("MenuGroup.cui", options).unwrap();
        writer
            .write_all(
                br#"<MenuGroup><MenuMacro><Macro><Command>^C^C_.CIRCLE</Command><SmallImage Name="circle-native"/><LargeImage Name="circle-large"/></Macro></MenuMacro></MenuGroup>"#,
            )
            .unwrap();
        writer
            .start_file("icons/circle-native.bmp", options)
            .unwrap();
        writer.write_all(&bmp).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn resolves_cuix_macro_to_a_local_embedded_image_and_command() {
        let icons = read_cuix(Cursor::new(cuix_fixture())).unwrap();
        let circle = icons.get("CIRCLE").unwrap();
        assert!(circle.starts_with("data:image/png;base64,"));
        let png = STANDARD
            .decode(circle.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        let mut decoder = ZlibDecoder::new(&png[41..41 + idat_len]);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, [0, 0xff, 0x80, 0x40, 0xff]);
        assert_eq!(command_name("^C^C_.LINE\n"), Some("LINE".into()));
        assert_eq!(command_name("._CIRCLE "), Some("CIRCLE".into()));
        assert_eq!(
            command_name("$M=$(if,$(eq,$(substr,$(getvar,cmdnames),1,4),GRIP),_move,^C^C_move) "),
            Some("MOVE".into())
        );
        assert_eq!(command_name("直线"), None);
        assert_eq!(command_name("(c:MYCOMMAND)"), None);
    }

    #[test]
    fn rejects_malformed_or_unsupported_bmp_images() {
        assert!(image_data_uri("broken.bmp", b"BM not a bitmap").is_none());
        let mut bmp = vec![0u8; 58];
        bmp[..2].copy_from_slice(b"BM");
        bmp[10..14].copy_from_slice(&(54u32).to_le_bytes());
        bmp[14..18].copy_from_slice(&(40u32).to_le_bytes());
        bmp[18..22].copy_from_slice(&(1i32).to_le_bytes());
        bmp[22..26].copy_from_slice(&(1i32).to_le_bytes());
        bmp[26..28].copy_from_slice(&(1u16).to_le_bytes());
        bmp[28..30].copy_from_slice(&(8u16).to_le_bytes());
        assert!(image_data_uri("unsupported.bmp", &bmp).is_none());
    }

    #[test]
    fn png_conversion_preserves_autocad_rgba_pixels() {
        let pixels = [0x73, 0xc5, 0xff, 0xff];
        let data_uri = png_data_uri(&pixels, 1, 1).unwrap();
        let png = STANDARD
            .decode(data_uri.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 1);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 1);
        let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        let mut decoder = ZlibDecoder::new(&png[41..41 + idat_len]);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, [0, 0x73, 0xc5, 0xff, 0xff]);
    }

    #[test]
    fn resolves_icons_from_the_local_autocad_cuix_and_xmx_when_configured() {
        let (Some(cuix), Some(xmx)) = (
            std::env::var_os("CWE_TEST_AUTOCAD_CUIX"),
            std::env::var_os("CWE_TEST_AUTOCAD_XMX"),
        ) else {
            eprintln!("set CWE_TEST_AUTOCAD_CUIX and CWE_TEST_AUTOCAD_XMX to verify a local AutoCAD install");
            return;
        };
        let commands = [
            "LINE", "CIRCLE", "RECTANG", "ARC", "TRIM", "OFFSET", "ERASE",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<HashSet<_>>();
        let icons = read_cuix_with_resources(
            File::open(&cuix).unwrap(),
            &[PathBuf::from(xmx.clone())],
            Some(&commands),
        )
        .unwrap();
        for command in commands {
            assert!(
                icons
                    .get(&command)
                    .is_some_and(|image| image.starts_with("data:image/png;base64,")),
                "AutoCAD resource icon was not matched for {command}"
            );
        }

        let config: crate::core::Config =
            serde_yaml::from_str(include_str!("../profiles/default.yaml")).unwrap();
        let profile = config
            .profiles
            .iter()
            .find(|profile| profile.id == "autocad.default")
            .unwrap();
        let profile_command_set = profile_commands(profile);
        let all_profile_icons = read_cuix_with_resources(
            File::open(&cuix).unwrap(),
            &[PathBuf::from(xmx.clone())],
            Some(&profile_command_set),
        )
        .unwrap();
        let matched_commands = all_profile_icons.keys().cloned().collect::<HashSet<_>>();
        let missing_commands = profile_command_set
            .difference(&matched_commands)
            .cloned()
            .collect::<Vec<_>>();
        eprintln!("unmatched commands in this AutoCAD profile: {missing_commands:?}");
        let executable = PathBuf::from(xmx)
            .parent()
            .unwrap()
            .join("acad.exe")
            .to_string_lossy()
            .into_owned();
        warm();
        let resolved = for_profile("acad.exe", &executable, profile);
        let native_count = resolved.iter().filter(|image| image.is_some()).count();
        assert!(
            native_count >= 15,
            "expected at least fifteen locally matched AutoCAD icons, found {native_count}"
        );
        eprintln!("local AutoCAD profile resolved {native_count} native command icons");
    }

    #[test]
    fn registry_discovery_finds_the_configured_local_autocad_install() {
        let Some(xmx) = std::env::var_os("CWE_TEST_AUTOCAD_XMX") else {
            eprintln!("set CWE_TEST_AUTOCAD_XMX to verify local AutoCAD registry discovery");
            return;
        };
        let expected = PathBuf::from(xmx).parent().unwrap().to_path_buf();
        assert!(discover_registry_autocad_roots().contains(&expected));
    }

    #[test]
    fn native_images_are_only_selected_for_autocad_auto_icons() {
        let config: crate::core::Config =
            serde_yaml::from_str(include_str!("../profiles/default.yaml")).unwrap();
        let profile = config
            .profiles
            .iter()
            .find(|profile| profile.id == "autocad.default")
            .unwrap();
        let mut cache = icon_cache().lock().unwrap();
        cache.insert("CIRCLE".into(), "data:image/png;base64,AA==".into());
        drop(cache);
        let resolved = for_profile("acad.exe", "", profile);
        assert!(
            resolved[8..].iter().all(Option::is_none),
            "default ring explicitly uses bundled vector icons"
        );
        let mut auto_profile = profile.clone();
        auto_profile.outer_rings[0]
            .iter_mut()
            .find(|sector| sector.label == "圆")
            .unwrap()
            .icon = None;
        let auto_icons = for_profile("acad.exe", "", &auto_profile);
        assert_eq!(
            auto_icons[10].as_deref(),
            Some("data:image/png;base64,AA==")
        );
        assert!(for_profile("SketchUp.exe", "", profile)
            .iter()
            .all(Option::is_none));
    }
}
