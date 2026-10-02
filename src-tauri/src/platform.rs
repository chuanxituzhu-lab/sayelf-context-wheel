use crate::core::{key_code, keystroke_steps, Action, Context, KeyStep, Trigger};
use std::{
    collections::HashSet,
    mem::size_of,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::Sender,
        OnceLock,
    },
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{LibraryLoader::*, Threading::*},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
#[derive(Debug)]
pub enum InputEvent {
    Down(i32, i32),
    Move(i32, i32),
    Up(i32, i32),
    Cancel,
}
static TX: OnceLock<Sender<InputEvent>> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);
static TRIGGER: AtomicU8 = AtomicU8::new(1);
static TEST_INPUT: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, serde::Serialize)]
pub struct RunningApplication {
    pub process: String,
    pub title: String,
}

struct WindowListContext {
    current_pid: u32,
    applications: Vec<RunningApplication>,
}

unsafe extern "system" fn collect_visible_application(hwnd: HWND, data: LPARAM) -> BOOL {
    let context = &mut *(data as *mut WindowListContext);
    if IsWindowVisible(hwnd) == 0 || GetWindowTextLengthW(hwnd) <= 0 {
        return 1;
    }

    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 || pid == context.current_pid {
        return 1;
    }

    let mut title = vec![0u16; 513];
    let title_len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
    if title_len <= 0 {
        return 1;
    }

    let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if process_handle.is_null() {
        return 1;
    }
    let mut path = [0u16; 32768];
    let mut path_len = path.len() as u32;
    let ok = QueryFullProcessImageNameW(process_handle, 0, path.as_mut_ptr(), &mut path_len);
    CloseHandle(process_handle);
    if ok == 0 {
        return 1;
    }

    let process_path = String::from_utf16_lossy(&path[..path_len as usize]);
    let process = process_path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let title = String::from_utf16_lossy(&title[..title_len as usize])
        .trim()
        .to_owned();
    if !process.is_empty() && !title.is_empty() {
        context
            .applications
            .push(RunningApplication { process, title });
    }
    1
}

fn normalize_running_applications(
    applications: Vec<RunningApplication>,
) -> Vec<RunningApplication> {
    let mut seen = HashSet::new();
    let mut unique: Vec<_> = applications
        .into_iter()
        .filter(|application| {
            !application.process.trim().is_empty()
                && !application.title.trim().is_empty()
                && seen.insert(application.process.to_lowercase())
        })
        .collect();
    unique.sort_by_key(|application| application.process.to_lowercase());
    unique.truncate(100);
    unique
}

/// Enumerate visible, titled applications on demand. Window titles are returned
/// only to the local Profile Studio picker and are never written to app state.
pub fn running_applications() -> Vec<RunningApplication> {
    let mut context = WindowListContext {
        current_pid: unsafe { GetCurrentProcessId() },
        applications: Vec::new(),
    };
    unsafe {
        EnumWindows(
            Some(collect_visible_application),
            &mut context as *mut WindowListContext as LPARAM,
        );
    }
    normalize_running_applications(context.applications)
}

#[cfg(test)]
mod running_application_tests {
    use super::*;

    #[test]
    fn candidates_are_deduplicated_sorted_and_bounded() {
        let candidates = normalize_running_applications(vec![
            RunningApplication {
                process: "acad.exe".into(),
                title: "AutoCAD".into(),
            },
            RunningApplication {
                process: "chrome.exe".into(),
                title: "Browser".into(),
            },
            RunningApplication {
                process: "ACAD.EXE".into(),
                title: "Second AutoCAD window".into(),
            },
            RunningApplication {
                process: "".into(),
                title: "No process".into(),
            },
            RunningApplication {
                process: "hidden.exe".into(),
                title: "  ".into(),
            },
        ]);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].process, "acad.exe");
        assert_eq!(candidates[1].process, "chrome.exe");

        let many = (0..105)
            .map(|index| RunningApplication {
                process: format!("app-{index:03}.exe"),
                title: format!("Window {index}"),
            })
            .collect();
        assert_eq!(normalize_running_applications(many).len(), 100);
    }
}

pub fn set_trigger(t: Trigger) {
    TRIGGER.store(
        match t {
            Trigger::Xbutton1 => 1,
            Trigger::Xbutton2 => 2,
            Trigger::Middle => 3,
        },
        Ordering::Relaxed,
    )
}
pub fn update_trigger(t: Trigger) {
    if !ACTIVE.load(Ordering::Relaxed) {
        set_trigger(t)
    }
}
pub fn single_instance() -> Result<(), String> {
    unsafe {
        let base = "Local\\ContextWheelEngine.v01";
        let name = if cfg!(debug_assertions) {
            match std::env::var("CWE_TEST_INSTANCE_ID") {
                Ok(id)
                    if !id.is_empty()
                        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') =>
                {
                    format!("{base}.test.{id}")
                }
                Ok(_) => return Err("无效的测试实例标识".into()),
                Err(_) => base.to_owned(),
            }
        } else {
            base.to_owned()
        };
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if h.is_null() {
            return Err("无法创建单实例保护".into());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(h);
            return Err("Context Wheel 已在运行，请使用托盘菜单".into());
        }
        Ok(())
    }
}
unsafe extern "system" fn mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 {
        let p = &*(l as *const MSLLHOOKSTRUCT);
        if p.flags & LLMHF_INJECTED == 0
            || (TEST_INPUT.load(Ordering::Relaxed) && p.dwExtraInfo == 0xC0E123)
        {
            let msg = w as u32;
            let b = (p.mouseData >> 16) as u8;
            let t = TRIGGER.load(Ordering::Relaxed);
            let down =
                (t == 3 && msg == WM_MBUTTONDOWN) || (t < 3 && msg == WM_XBUTTONDOWN && b == t);
            let up = (t == 3 && msg == WM_MBUTTONUP) || (t < 3 && msg == WM_XBUTTONUP && b == t);
            if down {
                ACTIVE.store(true, Ordering::Relaxed);
                let _ = TX.get().unwrap().send(InputEvent::Down(p.pt.x, p.pt.y));
                return 1;
            }
            if up && ACTIVE.swap(false, Ordering::Relaxed) {
                let _ = TX.get().unwrap().send(InputEvent::Up(p.pt.x, p.pt.y));
                return 1;
            }
            if msg == WM_MOUSEMOVE && ACTIVE.load(Ordering::Relaxed) {
                let _ = TX.get().unwrap().send(InputEvent::Move(p.pt.x, p.pt.y));
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w, l)
}
unsafe extern "system" fn keyboard(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0
        && ACTIVE.load(Ordering::Relaxed)
        && (w as u32 == WM_KEYDOWN || w as u32 == WM_SYSKEYDOWN)
    {
        let p = &*(l as *const KBDLLHOOKSTRUCT);
        if p.vkCode == VK_ESCAPE as u32 {
            let _ = TX.get().unwrap().send(InputEvent::Cancel);
            return 1;
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w, l)
}
pub fn install(tx: Sender<InputEvent>) -> Result<(), String> {
    TEST_INPUT.store(
        cfg!(debug_assertions) && std::env::var("CWE_TEST_INPUT").as_deref() == Ok("1"),
        Ordering::Relaxed,
    );
    TX.set(tx).map_err(|_| "Hook 已安装")?;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let mh = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), module, 0);
        let kh = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), module, 0);
        if mh.is_null() || kh.is_null() {
            let err = GetLastError();
            if !mh.is_null() {
                UnhookWindowsHookEx(mh);
            }
            if !kh.is_null() {
                UnhookWindowsHookEx(kh);
            }
            let _ = ready_tx.send(Err(format!("Hook 安装失败 {err}")));
            return;
        }
        let _ = ready_tx.send(Ok(()));
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(mh);
        UnhookWindowsHookEx(kh);
    });
    ready_rx.recv().map_err(|e| e.to_string())?
}
pub fn foreground() -> Result<Context, String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return Err("无前台窗口".into());
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return Err("无法读取前台进程".into());
        }
        let mut buf = [0u16; 32768];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return Err("无法读取进程名称".into());
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let name = path.rsplit(['\\', '/']).next().unwrap_or("").to_string();
        let mut cls = [0u16; 256];
        let n = GetClassNameW(hwnd, cls.as_mut_ptr(), 256);
        let class = String::from_utf16_lossy(&cls[..n.max(0) as usize]);
        let desktop = name.eq_ignore_ascii_case("explorer.exe")
            && matches!(class.as_str(), "Progman" | "WorkerW");
        Ok(Context {
            hwnd: hwnd as isize,
            pid,
            process: name,
            process_path: path,
            desktop,
            ..Default::default()
        })
    }
}
pub fn same_target(c: &Context) -> bool {
    foreground().is_ok_and(|now| now.hwnd == c.hwnd && now.pid == c.pid)
}
pub fn monitor(x: i32, y: i32) -> (f64, f64, f64, f64) {
    unsafe {
        let m = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(m, &mut info) != 0 {
            let r = info.rcWork;
            (r.left as f64, r.top as f64, r.right as f64, r.bottom as f64)
        } else {
            (0., 0., 1920., 1080.)
        }
    }
}
fn send(inputs: &[INPUT]) -> Result<(), String> {
    unsafe {
        if SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        ) == inputs.len() as u32
        {
            Ok(())
        } else {
            Err("SendInput 未完整执行（可能是权限等级/UIPI 或键盘状态）".into())
        }
    }
}
fn key(vk: u16, scan: u16, flags: u32) -> INPUT {
    let extended = if matches!(vk, 0x21..=0x28 | 0x2d | 0x2e | 0x5b) {
        KEYEVENTF_EXTENDEDKEY
    } else {
        0
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags | extended,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
pub fn execute(action: &Action, c: &Context) -> Result<String, String> {
    if !same_target(c) {
        return Err("前台窗口已变化，命令已取消".into());
    }
    match action {
        Action::Hotkey { keys } => {
            let codes: Vec<u16> = keys
                .iter()
                .map(|k| key_code(k).ok_or("未知快捷键"))
                .collect::<Result<_, _>>()?;
            // Never release physical modifiers that belong to the user.
            unsafe {
                if [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
                    .iter()
                    .any(|v| GetAsyncKeyState(*v as i32) < 0)
                    || codes.iter().any(|v| GetAsyncKeyState(*v as i32) < 0)
                {
                    return Err("请先松开正在使用的快捷键或修饰键".into());
                }
            }
            let mut inputs = Vec::new();
            for v in &codes {
                inputs.push(key(*v, 0, 0))
            }
            for v in codes.iter().rev() {
                inputs.push(key(*v, 0, KEYEVENTF_KEYUP))
            }
            if let Err(e) = send(&inputs) {
                let release: Vec<INPUT> = codes
                    .iter()
                    .rev()
                    .map(|v| key(*v, 0, KEYEVENTF_KEYUP))
                    .collect();
                let _ = send(&release);
                return Err(e);
            }
            Ok("快捷键已发送".into())
        }
        Action::Keystroke { text } => {
            let mut inputs = Vec::new();
            for step in keystroke_steps(text) {
                match step {
                    KeyStep::Virtual(vk) => {
                        inputs.push(key(vk, 0, 0));
                        inputs.push(key(vk, 0, KEYEVENTF_KEYUP));
                    }
                    KeyStep::Unicode(ch) => {
                        inputs.push(key(0, ch, KEYEVENTF_UNICODE));
                        inputs.push(key(0, ch, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                    }
                }
            }
            if !inputs.is_empty() {
                send(&inputs)?
            }
            Ok("文本已发送".into())
        }
        Action::Launch { program, args } => {
            std::process::Command::new(program)
                .args(args)
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok("程序已启动".into())
        }
        Action::Adapter { id } => Err(format!("Adapter {id} 尚未接入")),
    }
}
