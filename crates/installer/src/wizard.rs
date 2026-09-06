//! Native, accessible Windows setup. No web runtime, elevation, or installer scripts.
use super::{Options, Result, wide};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::BST_CHECKED;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const LOCATION: i32 = 101;
const BROWSE: i32 = 102;
const STARTUP: i32 = 103;
const LAUNCH: i32 = 104;
const INSTALL: i32 = 105;
const CLOSE: i32 = 106;
const STATUS: i32 = 107;
#[derive(Default)]
struct Work {
    percent: u32,
    message: String,
    result: Option<std::result::Result<(), String>>,
}
struct State {
    work: Arc<Mutex<Work>>,
    busy: bool,
    finished: bool,
    launch: bool,
    path: PathBuf,
    product: &'static super::Product,
}

unsafe fn control(
    parent: HWND,
    class: &str,
    text: &str,
    style: u32,
    id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> HWND {
    unsafe {
        let handle = CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            x,
            y,
            width,
            height,
            parent,
            id as usize as HMENU,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        SendMessageW(
            handle,
            WM_SETFONT,
            GetStockObject(DEFAULT_GUI_FONT) as usize,
            1,
        );
        handle
    }
}
unsafe fn text(window: HWND, id: i32, value: &str) {
    unsafe {
        SetWindowTextW(GetDlgItem(window, id), wide(value).as_ptr());
    }
}
unsafe fn checked(window: HWND, id: i32) -> bool {
    unsafe { SendMessageW(GetDlgItem(window, id), BM_GETCHECK, 0, 0) == BST_CHECKED as isize }
}

pub fn run(options: Options) -> Result<()> {
    let executable = std::env::current_exe()?;
    let manifest = super::read_manifest(&super::read_payload(&executable)?)?;
    let product = super::product(&manifest.name)?;
    let path = options.install_dir.unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join(product.directory)
    });
    let mut state = Box::new(State {
        work: Arc::new(Mutex::new(Work::default())),
        busy: false,
        finished: false,
        launch: true,
        path: path.clone(),
        product,
    });
    unsafe {
        let _ = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let class = wide("EEFSetupWindow");
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let window = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.as_ptr(),
            wide(&format!("Install {} {}", product.display, manifest.version)).as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            680,
            520,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if window.is_null() {
            anyhow::bail!("Windows could not open the setup window")
        }
        SetWindowLongPtrW(window, GWLP_USERDATA, (&mut *state as *mut State) as isize);
        control(
            window,
            "STATIC",
            &format!("Welcome to {}", product.display),
            0,
            201,
            28,
            25,
            610,
            32,
        );
        let description = if manifest.name == "eef" {
            "EEF connects your devices and puts their AI models to work.\r\nInstall the EEF device app on this PC too if you want this PC to help."
        } else {
            "Let this PC help your EEF network. You choose its permissions and models.\r\nIf EEF runs on this PC, the device app connects automatically."
        };
        control(window, "STATIC", description, 0, 202, 28, 65, 610, 54);
        control(
            window,
            "STATIC",
            "Install location",
            0,
            203,
            28,
            135,
            600,
            24,
        );
        control(
            window,
            "EDIT",
            &path.display().to_string(),
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            LOCATION,
            28,
            166,
            475,
            30,
        );
        control(
            window,
            "BUTTON",
            "Browse…",
            WS_TABSTOP,
            BROWSE,
            515,
            165,
            105,
            32,
        );
        control(
            window,
            "STATIC",
            "Installs for your Windows account. No administrator access needed.\r\nExisting settings are kept. No AI model is included or downloaded.",
            0,
            204,
            28,
            210,
            600,
            48,
        );
        control(
            window,
            "BUTTON",
            "Start automatically when I sign in",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            STARTUP,
            28,
            270,
            595,
            28,
        );
        control(
            window,
            "BUTTON",
            "Open the app when installation finishes",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            LAUNCH,
            28,
            306,
            595,
            28,
        );
        let startup = options
            .startup
            .unwrap_or_else(|| super::startup_path(product).is_ok_and(|p| p.is_file()));
        SendMessageW(
            GetDlgItem(window, STARTUP),
            BM_SETCHECK,
            if startup { BST_CHECKED as usize } else { 0 },
            0,
        );
        SendMessageW(
            GetDlgItem(window, LAUNCH),
            BM_SETCHECK,
            if options.launch.unwrap_or(true) {
                BST_CHECKED as usize
            } else {
                0
            },
            0,
        );
        control(
            window,
            "STATIC",
            "Ready to install. Choose Install to continue.",
            0,
            STATUS,
            28,
            354,
            605,
            52,
        );
        control(
            window,
            "BUTTON",
            "Install",
            WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            INSTALL,
            386,
            423,
            110,
            36,
        );
        control(
            window, "BUTTON", "Cancel", WS_TABSTOP, CLOSE, 510, 423, 110, 36,
        );
        SetTimer(window, 1, 150, None);
        ShowWindow(window, SW_SHOW);
        UpdateWindow(window);
        let mut message = MSG::default();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(window, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        CoUninitialize();
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut State;
        if pointer.is_null() {
            return DefWindowProcW(window, message, wparam, lparam);
        }
        let state = &mut *pointer;
        match message {
            WM_COMMAND => match (wparam & 0xffff) as i32 {
                BROWSE if !state.busy => {
                    let mut display = [0u16; 260];
                    let title = wide("Choose a dedicated folder for this app");
                    let info = BROWSEINFOW {
                        hwndOwner: window,
                        pszDisplayName: display.as_mut_ptr(),
                        lpszTitle: title.as_ptr(),
                        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
                        ..Default::default()
                    };
                    let item = SHBrowseForFolderW(&info);
                    if !item.is_null() {
                        let mut path = [0u16; 260];
                        if SHGetPathFromIDListW(item, path.as_mut_ptr()) != 0 {
                            let end = path.iter().position(|c| *c == 0).unwrap_or(path.len());
                            let selected = PathBuf::from(String::from_utf16_lossy(&path[..end]))
                                .join(state.product.directory);
                            text(window, LOCATION, &selected.display().to_string());
                        }
                        CoTaskMemFree(item.cast());
                    }
                    0
                }
                INSTALL if !state.busy => {
                    if state.finished {
                        if let Err(error) = super::launch_installed(
                            &state.path.join(state.product.executable),
                            &state.path.join(state.product.config),
                        ) {
                            text(window, STATUS, &format!("Could not open the app: {error}"));
                        }
                        return 0;
                    }
                    let mut path =
                        vec![0u16; GetWindowTextLengthW(GetDlgItem(window, LOCATION)) as usize + 1];
                    GetWindowTextW(
                        GetDlgItem(window, LOCATION),
                        path.as_mut_ptr(),
                        path.len() as i32,
                    );
                    let path = String::from_utf16_lossy(&path[..path.len() - 1]);
                    if path.trim().is_empty() {
                        text(window, STATUS, "Choose an installation folder first.");
                        return 0;
                    }
                    state.path = PathBuf::from(path);
                    state.launch = checked(window, LAUNCH);
                    state.busy = true;
                    let startup = checked(window, STARTUP);
                    for id in [INSTALL, BROWSE, LOCATION, STARTUP, LAUNCH, CLOSE] {
                        EnableWindow(GetDlgItem(window, id), 0);
                    }
                    let work = state.work.clone();
                    let report = work.clone();
                    let destination = state.path.clone();
                    std::thread::spawn(move || {
                        let callback: Arc<dyn Fn(u32, &str) + Send + Sync> =
                            Arc::new(move |percent, message| {
                                let mut work = report.lock().unwrap();
                                work.percent = percent;
                                work.message = message.into();
                            });
                        let result = super::install(
                            Options {
                                install_dir: Some(destination),
                                startup: Some(startup),
                                launch: Some(false),
                                quiet: true,
                            },
                            Some(callback),
                        );
                        work.lock().unwrap().result = Some(result.map_err(|e| format!("{e:#}")));
                    });
                    0
                }
                CLOSE if !state.busy => {
                    DestroyWindow(window);
                    0
                }
                _ => 0,
            },
            WM_TIMER => {
                if state.busy {
                    let (percent, status, result) = {
                        let mut work = state.work.lock().unwrap();
                        (work.percent, work.message.clone(), work.result.take())
                    };
                    text(window, STATUS, &format!("{percent}% — {status}"));
                    if let Some(result) = result {
                        state.busy = false;
                        EnableWindow(GetDlgItem(window, CLOSE), 1);
                        EnableWindow(GetDlgItem(window, INSTALL), 1);
                        match result {
                            Ok(()) => {
                                state.finished = true;
                                text(
                                    window,
                                    STATUS,
                                    "You’re ready. Open the app to see connection status, choose permissions, and add models. Find it later in Start → EEF.",
                                );
                                text(window, INSTALL, "Open app");
                                text(window, CLOSE, "Finish");
                                if state.launch {
                                    if let Err(error) = super::launch_installed(
                                        &state.path.join(state.product.executable),
                                        &state.path.join(state.product.config),
                                    ) {
                                        text(
                                            window,
                                            STATUS,
                                            &format!(
                                                "Installed, but the app could not open: {error}"
                                            ),
                                        );
                                    }
                                }
                            }
                            Err(error) => {
                                text(
                                    window,
                                    STATUS,
                                    &format!("Installation needs attention: {error}"),
                                );
                                text(window, INSTALL, "Try again");
                                for id in [BROWSE, LOCATION, STARTUP, LAUNCH] {
                                    EnableWindow(GetDlgItem(window, id), 1);
                                }
                            }
                        }
                    }
                }
                0
            }
            WM_CLOSE => {
                if !state.busy {
                    DestroyWindow(window);
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}
