//! Native, accessible Windows setup. No web runtime, elevation, or installer scripts.
use super::{Options, Result, wide};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    BST_CHECKED, ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, InitCommonControlsEx, PBM_SETPOS,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const LOCATION: i32 = 101;
const BROWSE: i32 = 102;
const STARTUP: i32 = 103;
const LAUNCH: i32 = 104;
const NEXT: i32 = 105;
const CLOSE: i32 = 106;
const STATUS: i32 = 107;
const BACK: i32 = 108;
const REVIEW: i32 = 109;
const PROGRESS: i32 = 110;
const HEADING: i32 = 201;
const INTRO: i32 = 202;
const LOCATION_LABEL: i32 = 203;
const OPTIONS_HINT: i32 = 204;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Welcome,
    Options,
    Review,
    Installing,
    Finished,
    LaunchFailed,
}

#[derive(Default)]
struct Work {
    percent: u32,
    message: String,
    result: Option<std::result::Result<(), String>>,
}
struct State {
    work: Arc<Mutex<Work>>,
    page: Page,
    launch: bool,
    path: PathBuf,
    installer: PathBuf,
    product_name: String,
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

unsafe fn show_page(window: HWND, state: &mut State, page: Page) {
    unsafe {
        state.page = page;
        let options = page == Page::Options;
        for id in [
            LOCATION_LABEL,
            LOCATION,
            BROWSE,
            OPTIONS_HINT,
            STARTUP,
            LAUNCH,
        ] {
            ShowWindow(
                GetDlgItem(window, id),
                if options { SW_SHOW } else { SW_HIDE },
            );
        }
        for (id, visible) in [
            (REVIEW, page == Page::Review),
            (PROGRESS, page == Page::Installing),
            (BACK, matches!(page, Page::Options | Page::Review)),
            (NEXT, page != Page::Installing),
            (CLOSE, page != Page::Installing && page != Page::Finished),
        ] {
            ShowWindow(
                GetDlgItem(window, id),
                if visible { SW_SHOW } else { SW_HIDE },
            );
        }
        let (heading, next, description) = match page {
            Page::Welcome => (
                format!("Welcome to {}", state.product.display),
                "Next",
                "Choose where to install and whether the app should start automatically.",
            ),
            Page::Options => (
                "Installation options".into(),
                "Next",
                "Choose a dedicated folder and how the app should start.",
            ),
            Page::Review => (
                "Review setup".into(),
                "Install",
                "Check these settings before installing.",
            ),
            Page::Installing => (
                "Installing".into(),
                "",
                "Please wait while setup copies the files and creates the Start menu entry.",
            ),
            Page::Finished => (
                "Installation complete".into(),
                "Finish",
                "You can find the app later in the eef folder in the Start menu.",
            ),
            Page::LaunchFailed => (
                "App could not open".into(),
                "Retry opening",
                "Installation is complete. Retry opening the app, or close setup and open it later from the Start menu.",
            ),
        };
        text(window, HEADING, &heading);
        text(window, INTRO, description);
        text(window, NEXT, next);
        text(
            window,
            CLOSE,
            if page == Page::LaunchFailed {
                "Close"
            } else {
                "Cancel"
            },
        );
        text(window, STATUS, "");
        if page == Page::Review {
            let startup = if checked(window, STARTUP) {
                "On"
            } else {
                "Off"
            };
            let launch = if state.launch { "Yes" } else { "No" };
            text(
                window,
                REVIEW,
                &format!(
                    "{}\r\n\r\nFolder\r\n{}\r\n\r\nStart at sign-in  {startup}\r\nOpen after setup  {launch}",
                    state.product.display,
                    state.path.display()
                ),
            );
        }
        if page == Page::Installing {
            SendMessageW(GetDlgItem(window, PROGRESS), PBM_SETPOS, 0, 0);
            text(window, STATUS, "Preparing to install…");
            SetFocus(window);
        } else {
            SetFocus(GetDlgItem(window, if options { LOCATION } else { NEXT }));
        }
    }
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
        page: Page::Welcome,
        launch: true,
        path: path.clone(),
        installer: executable,
        product_name: manifest.name,
        product,
    });
    unsafe {
        let _ = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_PROGRESS_CLASS,
        });
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
            HEADING,
            28,
            25,
            610,
            32,
        );
        control(window, "STATIC", "", 0, INTRO, 28, 65, 610, 54);
        control(
            window,
            "STATIC",
            "Install location",
            0,
            LOCATION_LABEL,
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
            OPTIONS_HINT,
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
        control(window, "STATIC", "", 0, STATUS, 28, 354, 605, 52);
        control(window, "STATIC", "", 0, REVIEW, 28, 135, 595, 205);
        control(
            window,
            "msctls_progress32",
            "",
            0,
            PROGRESS,
            28,
            142,
            595,
            18,
        );
        control(
            window, "BUTTON", "Back", WS_TABSTOP, BACK, 263, 423, 110, 36,
        );
        control(
            window,
            "BUTTON",
            "Next",
            WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            NEXT,
            386,
            423,
            110,
            36,
        );
        control(
            window, "BUTTON", "Cancel", WS_TABSTOP, CLOSE, 510, 423, 110, 36,
        );
        show_page(window, &mut state, Page::Welcome);
        SetTimer(window, 1, 150, None);
        ShowWindow(window, SW_SHOW);
        SetFocus(GetDlgItem(window, NEXT));
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
                BROWSE if state.page == Page::Options => {
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
                BACK => {
                    match state.page {
                        Page::Options => show_page(window, state, Page::Welcome),
                        Page::Review => show_page(window, state, Page::Options),
                        _ => (),
                    }
                    0
                }
                NEXT | IDOK => {
                    match state.page {
                        Page::Welcome => {
                            show_page(window, state, Page::Options);
                            return 0;
                        }
                        Page::Options => {
                            let edit = GetDlgItem(window, LOCATION);
                            let mut path = vec![0u16; GetWindowTextLengthW(edit) as usize + 1];
                            let length = GetWindowTextW(edit, path.as_mut_ptr(), path.len() as i32);
                            let path = String::from_utf16_lossy(&path[..length as usize]);
                            if path.trim().is_empty() {
                                text(window, STATUS, "Choose an installation folder first.");
                                SetFocus(edit);
                                return 0;
                            }
                            let validated = super::absolute(PathBuf::from(path)).and_then(|path| {
                                super::validate_install_destination(
                                    &state.installer,
                                    &path,
                                    &state.product_name,
                                )?;
                                Ok(path)
                            });
                            match validated {
                                Ok(path) => state.path = path,
                                Err(error) => {
                                    text(
                                        window,
                                        STATUS,
                                        &format!("Could not use this folder: {error:#}"),
                                    );
                                    SetFocus(edit);
                                    return 0;
                                }
                            }
                            state.launch = checked(window, LAUNCH);
                            show_page(window, state, Page::Review);
                            return 0;
                        }
                        Page::Installing => return 0,
                        Page::Finished | Page::LaunchFailed => {
                            if state.launch {
                                if let Err(error) = super::launch_installed(
                                    &state.path.join(state.product.executable),
                                    &state.path.join(state.product.config),
                                ) {
                                    show_page(window, state, Page::LaunchFailed);
                                    text(
                                        window,
                                        STATUS,
                                        &format!("Installed, but the app could not open: {error}"),
                                    );
                                    return 0;
                                }
                                state.launch = false;
                            }
                            DestroyWindow(window);
                            return 0;
                        }
                        Page::Review => (),
                    }
                    let startup = checked(window, STARTUP);
                    show_page(window, state, Page::Installing);
                    *state.work.lock().unwrap() = Work::default();
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
                CLOSE | IDCANCEL if state.page != Page::Installing => {
                    DestroyWindow(window);
                    0
                }
                _ => 0,
            },
            WM_TIMER => {
                if state.page == Page::Installing {
                    let (percent, status, result) = {
                        let mut work = state.work.lock().unwrap();
                        (work.percent, work.message.clone(), work.result.take())
                    };
                    if !status.is_empty() {
                        SendMessageW(
                            GetDlgItem(window, PROGRESS),
                            PBM_SETPOS,
                            percent as usize,
                            0,
                        );
                        text(window, STATUS, &format!("{percent}% — {status}"));
                    }
                    if let Some(result) = result {
                        match result {
                            Ok(()) => show_page(window, state, Page::Finished),
                            Err(error) => {
                                show_page(window, state, Page::Review);
                                text(
                                    window,
                                    STATUS,
                                    &format!("Installation needs attention: {error}"),
                                );
                            }
                        }
                    }
                }
                0
            }
            WM_CLOSE => {
                if state.page != Page::Installing {
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
