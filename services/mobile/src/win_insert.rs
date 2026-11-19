//! Types into a Windows app without keys or focus: the text goes into the
//! app's edit control as one `EM_REPLACESEL` message, at its caret, so what
//! is already there stays. Only classic edit controls take it (Edit and the
//! RichEdit family, which covers Notepad and WordPad); the field it goes into
//! is the one focused in that app's window, or its only visible edit control.
//! Password and read-only fields are refused.

/// Window classes that take `EM_REPLACESEL`.
const EDIT_CLASSES: [&str; 5] = [
    "edit",
    "richedit20w",
    "richedit50w",
    "richeditd2dpt",
    "richedit60w",
];

/// Whether a window of this class is an edit control FRIDAY can insert into.
pub fn is_edit_class(class: &str) -> bool {
    EDIT_CLASSES.contains(&class.to_ascii_lowercase().as_str())
}

/// Whether a window belongs to the app named `app`: its program's file name
/// (with or without `.exe`) or its title, ignoring case.
pub fn names_app(app: &str, exe_path: &str, title: &str) -> bool {
    let app = app.trim().to_lowercase();
    if app.is_empty() {
        return false;
    }
    let file = exe_path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_lowercase();
    let stem = file.strip_suffix(".exe").unwrap_or(&file);
    stem == app.strip_suffix(".exe").unwrap_or(&app) || title.to_lowercase().contains(&app)
}

/// Inserts `text` at the caret of `app`'s edit control.
#[cfg(windows)]
pub fn insert_text(app: &str, text: &str) -> Result<(), String> {
    let windows = sys::top_windows(app);
    if windows.is_empty() {
        return Err(format!("no open window of {app}"));
    }
    let target = windows
        .iter()
        .find_map(|&w| sys::focused_edit(w))
        .or_else(|| {
            let mut edits: Vec<_> = windows
                .iter()
                .flat_map(|&w| sys::visible_edits(w))
                .collect();
            (edits.len() == 1).then(|| edits.remove(0))
        })
        .ok_or_else(|| format!("{app} has no single text field FRIDAY can type into"))?;
    sys::insert(target, text)
}

#[cfg(windows)]
mod sys {
    use super::{is_edit_class, names_app};
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::Controls::EM_REPLACESEL;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, EnumWindows, GetClassNameW, GetGUIThreadInfo, GetWindowLongW,
        GetWindowTextW, GetWindowThreadProcessId, IsChild, IsWindowVisible, SendMessageW,
        ES_PASSWORD, ES_READONLY, GUITHREADINFO, GWL_STYLE,
    };

    struct Search<'a> {
        app: &'a str,
        found: Vec<HWND>,
    }

    /// The app's visible top-level windows.
    pub fn top_windows(app: &str) -> Vec<HWND> {
        let mut search = Search {
            app,
            found: Vec::new(),
        };
        unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> i32 {
            let search = &mut *(lp as *mut Search);
            if IsWindowVisible(hwnd) != 0 && names_app(search.app, &exe_of(hwnd), &title_of(hwnd)) {
                search.found.push(hwnd);
            }
            1
        }
        unsafe { EnumWindows(Some(each), &mut search as *mut Search as LPARAM) };
        search.found
    }

    /// The edit control focused in `top`'s thread, when it is inside `top`.
    pub fn focused_edit(top: HWND) -> Option<HWND> {
        let thread = unsafe { GetWindowThreadProcessId(top, std::ptr::null_mut()) };
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetGUIThreadInfo(thread, &mut info) } == 0 {
            return None;
        }
        let focus = info.hwndFocus;
        let inside = focus == top || unsafe { IsChild(top, focus) } != 0;
        (!focus.is_null() && inside && is_edit_class(&class_of(focus))).then_some(focus)
    }

    /// Every visible edit control in `top`, `top` itself included.
    pub fn visible_edits(top: HWND) -> Vec<HWND> {
        let mut found = Vec::new();
        if is_edit_class(&class_of(top)) {
            found.push(top);
        }
        unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> i32 {
            let found = &mut *(lp as *mut Vec<HWND>);
            if IsWindowVisible(hwnd) != 0 && is_edit_class(&class_of(hwnd)) {
                found.push(hwnd);
            }
            1
        }
        unsafe { EnumChildWindows(top, Some(each), &mut found as *mut Vec<HWND> as LPARAM) };
        found
    }

    /// Replaces the (usually empty) selection with `text`, as one undoable
    /// edit. The system copies the string across to the app's process.
    pub fn insert(edit: HWND, text: &str) -> Result<(), String> {
        let style = unsafe { GetWindowLongW(edit, GWL_STYLE) };
        if style & ES_PASSWORD != 0 {
            return Err("refusing to type into password or secure field".to_string());
        }
        if style & ES_READONLY != 0 {
            return Err("that field is read-only".to_string());
        }
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        unsafe { SendMessageW(edit, EM_REPLACESEL, 1, wide.as_ptr() as LPARAM) };
        Ok(())
    }

    pub fn class_of(hwnd: HWND) -> String {
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    fn title_of(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    fn exe_of(hwnd: HWND) -> String {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = unsafe {
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
        };
        unsafe { CloseHandle(process) };
        if ok == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WM_GETTEXT, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        };

        fn wide(s: &str) -> Vec<u16> {
            s.encode_utf16().chain(Some(0)).collect()
        }

        fn edit(text: &str, style: u32) -> HWND {
            let (class, text) = (wide("EDIT"), wide(text));
            unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    text.as_ptr(),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE | style,
                    0,
                    0,
                    200,
                    100,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            }
        }

        fn text_of(hwnd: HWND) -> String {
            let mut buf = [0u16; 256];
            let n =
                unsafe { SendMessageW(hwnd, WM_GETTEXT, buf.len(), buf.as_mut_ptr() as LPARAM) };
            String::from_utf16_lossy(&buf[..n as usize])
        }

        #[test]
        fn inserts_without_replacing_and_refuses_secret_fields() {
            let field = edit("kept", 0);
            assert!(!field.is_null());
            assert_eq!(visible_edits(field), vec![field]);
            insert(field, " and added").unwrap();
            let after = text_of(field);
            assert!(
                after.contains("kept") && after.contains(" and added"),
                "{after}"
            );

            let secret = edit("", ES_PASSWORD as u32);
            assert!(insert(secret, "x").is_err());
            unsafe {
                DestroyWindow(field);
                DestroyWindow(secret);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_edit_classes() {
        assert!(is_edit_class("Edit"));
        assert!(is_edit_class("RichEditD2DPT"));
        assert!(!is_edit_class("Chrome_RenderWidgetHostHWND"));
    }

    #[test]
    fn names_an_app_by_program_or_title() {
        let exe = r"C:\Windows\System32\notepad.exe";
        assert!(names_app("notepad", exe, "Untitled - Notepad"));
        assert!(names_app("Notepad.exe", exe, ""));
        assert!(names_app(
            "wordpad",
            r"C:\x\other.exe",
            "Document - WordPad"
        ));
        assert!(!names_app("note", r"C:\x\other.exe", "Calculator"));
        assert!(!names_app(" ", exe, "Notepad"));
    }
}
