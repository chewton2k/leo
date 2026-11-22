use leo_core::call_names::Readers;

pub fn readers() -> Option<Readers> {
    #[cfg(target_os = "macos")]
    {
        Some(std::sync::Arc::new(|| {
            Box::new(mac::Mac::new()) as Box<dyn leo_core::call_names::Reader>
        }))
    }
    #[cfg(windows)]
    {
        Some(std::sync::Arc::new(|| {
            Box::new(win::Win::new()) as Box<dyn leo_core::call_names::Reader>
        }))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    use leo_core::call_names::{
        self as names, Kind, Look, Node, Reader, Window, BROWSERS, MOST_WINDOWS, TEAMS_APPS,
        ZOOM_APPS,
    };
    use objc2_app_kit::{NSRunningApplication, NSWorkspace};
    use objc2_foundation::NSString;

    type Ref = *const c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Range {
        location: isize,
        length: isize,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> Ref;
        fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
        fn AXUIElementCopyMultipleAttributeValues(
            element: Ref,
            attributes: Ref,
            options: u32,
            values: *mut Ref,
        ) -> i32;
        fn AXUIElementSetAttributeValue(element: Ref, attribute: Ref, value: Ref) -> i32;
        fn AXUIElementSetMessagingTimeout(element: Ref, seconds: f32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: Ref;
        static kCFBooleanFalse: Ref;
        static kCFTypeArrayCallBacks: u8;
        fn CFRelease(value: Ref);
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFArrayGetTypeID() -> usize;
        fn CFArrayGetCount(array: Ref) -> isize;
        fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
        fn CFArrayCreate(
            allocator: Ref,
            values: *const Ref,
            count: isize,
            callbacks: *const u8,
        ) -> Ref;
        fn CFStringCreateWithBytes(
            allocator: Ref,
            bytes: *const u8,
            length: isize,
            encoding: u32,
            external: u8,
        ) -> Ref;
        fn CFStringGetLength(string: Ref) -> isize;
        fn CFStringGetBytes(
            string: Ref,
            range: Range,
            encoding: u32,
            loss: u8,
            external: u8,
            buffer: *mut u8,
            most: isize,
            used: *mut isize,
        ) -> isize;
    }

    const UTF8: u32 = 0x0800_0100;
    const KEYS: [&str; 8] = [
        "AXRole",
        "AXSubrole",
        "AXDescription",
        "AXTitle",
        "AXValue",
        "AXIdentifier",
        "AXDOMClassList",
        "AXChildren",
    ];
    const CHROMIUM: &[&str] = &[
        "com.google.Chrome",
        "com.microsoft.edgemac",
        "com.brave.Browser",
        "company.thebrowser.Browser",
        "com.microsoft.teams2",
        "com.microsoft.teams",
    ];
    const TYPED: &[&str] = &["AXTextField", "AXTextArea", "AXComboBox"];

    struct Owned(Ref);

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) }
            }
        }
    }

    fn cf_string(text: &str) -> Owned {
        Owned(unsafe {
            CFStringCreateWithBytes(null(), text.as_ptr(), text.len() as isize, UTF8, 0)
        })
    }

    fn text_of(value: Ref) -> Option<String> {
        if value.is_null() || unsafe { CFGetTypeID(value) != CFStringGetTypeID() } {
            return None;
        }
        let range = Range {
            location: 0,
            length: unsafe { CFStringGetLength(value) }.min(2 * names::MOST_TEXT as isize),
        };
        let mut used = 0;
        unsafe { CFStringGetBytes(value, range, UTF8, 0, 0, null_mut(), 0, &mut used) };
        let mut bytes = vec![0u8; used.max(0) as usize];
        unsafe {
            CFStringGetBytes(
                value,
                range,
                UTF8,
                0,
                0,
                bytes.as_mut_ptr(),
                bytes.len() as isize,
                &mut used,
            )
        };
        bytes.truncate(used.max(0) as usize);
        names::clip(&String::from_utf8_lossy(&bytes))
    }

    fn items(array: Ref) -> Vec<Ref> {
        if array.is_null() || unsafe { CFGetTypeID(array) != CFArrayGetTypeID() } {
            return Vec::new();
        }
        (0..unsafe { CFArrayGetCount(array) })
            .map(|i| unsafe { CFArrayGetValueAtIndex(array, i) })
            .collect()
    }

    fn attribute(element: Ref, name: &str) -> Owned {
        let key = cf_string(name);
        let mut value = null();
        if unsafe { AXUIElementCopyAttributeValue(element, key.0, &mut value) } != 0 {
            return Owned(null());
        }
        Owned(value)
    }

    fn set(element: Ref, name: &str, on: bool) {
        let key = cf_string(name);
        unsafe {
            let value = if on { kCFBooleanTrue } else { kCFBooleanFalse };
            AXUIElementSetAttributeValue(element, key.0, value);
        }
    }

    pub struct Mac {
        keys: Owned,
        asked: Vec<i32>,
        enhanced: Vec<i32>,
    }

    impl Mac {
        pub fn new() -> Self {
            let strings: Vec<Owned> = KEYS.iter().map(|k| cf_string(k)).collect();
            let refs: Vec<Ref> = strings.iter().map(|s| s.0).collect();
            let keys = Owned(unsafe {
                CFArrayCreate(
                    null(),
                    refs.as_ptr(),
                    refs.len() as isize,
                    &raw const kCFTypeArrayCallBacks,
                )
            });
            Self {
                keys,
                asked: Vec::new(),
                enhanced: Vec::new(),
            }
        }

        fn copy(
            &self,
            element: Ref,
            depth: usize,
            budget: &mut usize,
            most_depth: usize,
            zoom: bool,
        ) -> Node {
            *budget = budget.saturating_sub(1);
            let mut values = null();
            let copied = unsafe {
                AXUIElementCopyMultipleAttributeValues(element, self.keys.0, 0, &mut values)
            } == 0;
            let values = Owned(if copied { values } else { null() });
            let got = items(values.0);
            let at = |i: usize| got.get(i).copied().unwrap_or(null());
            let role = text_of(at(0)).unwrap_or_default();
            let mut node = Node {
                subrole: text_of(at(1)),
                description: text_of(at(2)),
                title: text_of(at(3)),
                value: if TYPED.contains(&role.as_str()) {
                    None
                } else {
                    text_of(at(4))
                },
                id: text_of(at(5)),
                classes: items(at(6))
                    .into_iter()
                    .filter_map(text_of)
                    .take(20)
                    .collect(),
                role,
                children: Vec::new(),
            };
            let tile = zoom
                && node.role == "AXTabGroup"
                && node.description.as_deref().is_some_and(|d| d.contains(','));
            if tile || depth >= most_depth {
                return node;
            }
            for child in items(at(7)) {
                if *budget == 0 {
                    break;
                }
                node.children
                    .push(self.copy(child, depth + 1, budget, most_depth, zoom));
            }
            node
        }

        fn ask(&mut self, app: Ref, pid: i32, enhance: bool) {
            if !self.asked.contains(&pid) {
                set(app, "AXManualAccessibility", true);
                self.asked.push(pid);
            }
            if enhance {
                set(app, "AXEnhancedUserInterface", true);
                if !self.enhanced.contains(&pid) {
                    self.enhanced.push(pid);
                }
            }
        }

        fn running() -> Vec<(i32, &'static str)> {
            let front = NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .map(|app| app.processIdentifier());
            let mut running: Vec<(i32, &'static str)> = ZOOM_APPS
                .iter()
                .chain(TEAMS_APPS)
                .chain(BROWSERS)
                .filter(|id| !id.ends_with(".exe"))
                .flat_map(|id| {
                    NSRunningApplication::runningApplicationsWithBundleIdentifier(
                        &NSString::from_str(id),
                    )
                    .iter()
                    .map(|app| (app.processIdentifier(), *id))
                    .collect::<Vec<_>>()
                })
                .collect();
            running.sort_by_key(|(pid, _)| Some(*pid) != front);
            running
        }
    }

    impl Reader for Mac {
        fn read(&mut self) -> Look {
            if !crate::permissions::accessibility() {
                return Look::Unreadable("no Accessibility permission".into());
            }
            let running = Self::running();
            if running.is_empty() {
                return Look::Unreadable("no call app is open".into());
            }
            for (pid, id) in running {
                let Some(kind) = names::app_kind(id) else {
                    continue;
                };
                let app = Owned(unsafe { AXUIElementCreateApplication(pid) });
                if app.0.is_null() {
                    continue;
                }
                unsafe { AXUIElementSetMessagingTimeout(app.0, 0.3) };
                let chromium = CHROMIUM.contains(&id);
                if chromium && kind == Kind::Teams {
                    self.ask(app.0, pid, false);
                }
                let list = attribute(app.0, "AXWindows");
                let found: Vec<(Ref, String, String)> = items(list.0)
                    .into_iter()
                    .map(|w| {
                        let title = text_of(attribute(w, "AXTitle").0).unwrap_or_default();
                        let ident = text_of(attribute(w, "AXIdentifier").0).unwrap_or_default();
                        (w, title, ident)
                    })
                    .filter(|(_, title, ident)| names::call_window(kind, title, ident))
                    .take(MOST_WINDOWS)
                    .collect();
                if found.is_empty() {
                    continue;
                }
                if chromium && kind != Kind::Teams {
                    self.ask(app.0, pid, true);
                    for (w, _, _) in &found {
                        set(*w, "AXManualAccessibility", true);
                    }
                }
                let zoom = kind == Kind::Zoom;
                let (most_depth, mut budget) = if zoom { (14, 1500) } else { (40, 4000) };
                let windows: Vec<Window> = found
                    .into_iter()
                    .map(|(w, title, id)| Window {
                        tree: self.copy(w, 0, &mut budget, most_depth, zoom),
                        title,
                        id,
                    })
                    .collect();
                if kind == Kind::Teams && !windows.iter().any(|w| names::is_call_tree(&w.tree)) {
                    continue;
                }
                return Look::Call {
                    app: id.to_owned(),
                    windows,
                };
            }
            Look::Unreadable("no call window is open".into())
        }
    }

    impl Drop for Mac {
        fn drop(&mut self) {
            for pid in &self.enhanced {
                let app = Owned(unsafe { AXUIElementCreateApplication(*pid) });
                if !app.0.is_null() {
                    set(app.0, "AXEnhancedUserInterface", false);
                }
            }
        }
    }
}

#[cfg(windows)]
mod win {
    use leo_core::call_names::{self as names, Kind, Look, Node, Reader, Window, MOST_WINDOWS};
    use windows::core::{Interface, BOOL};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Accessibility::{
        AutomationElementMode_None, CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2,
        IUIAutomationCacheRequest, IUIAutomationElement, TreeScope_Subtree,
        UIA_AutomationIdPropertyId, UIA_ClassNamePropertyId, UIA_ControlTypePropertyId,
        UIA_HelpTextPropertyId, UIA_NamePropertyId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    const MOST_DEPTH: usize = 40;
    const BUDGET: usize = 4000;
    const TIMEOUT_MS: u32 = 1500;

    struct Uia {
        uia: IUIAutomation,
        request: IUIAutomationCacheRequest,
    }

    pub struct Win {
        uia: Option<Uia>,
        com: bool,
    }

    fn start() -> windows::core::Result<Uia> {
        unsafe {
            let uia: IUIAutomation =
                match CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) {
                    Ok(uia) => uia,
                    Err(_) => CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?,
                };
            if let Ok(timed) = uia.cast::<IUIAutomation2>() {
                let _ = timed.SetConnectionTimeout(TIMEOUT_MS);
                let _ = timed.SetTransactionTimeout(TIMEOUT_MS);
            }
            let request = uia.CreateCacheRequest()?;
            for property in [
                UIA_ControlTypePropertyId,
                UIA_NamePropertyId,
                UIA_HelpTextPropertyId,
                UIA_AutomationIdPropertyId,
                UIA_ClassNamePropertyId,
            ] {
                request.AddProperty(property)?;
            }
            request.SetTreeScope(TreeScope_Subtree)?;
            request.SetTreeFilter(&uia.RawViewCondition()?)?;
            request.SetAutomationElementMode(AutomationElementMode_None)?;
            Ok(Uia { uia, request })
        }
    }

    impl Win {
        pub fn new() -> Self {
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            Self {
                uia: start().ok(),
                com,
            }
        }
    }

    impl Drop for Win {
        fn drop(&mut self) {
            self.uia = None;
            if self.com {
                unsafe { CoUninitialize() };
            }
        }
    }

    fn role(control: i32) -> &'static str {
        match control {
            50000 => "Button",
            50002 => "CheckBox",
            50003 => "ComboBox",
            50004 => "Edit",
            50005 => "Hyperlink",
            50006 => "Image",
            50007 => "ListItem",
            50008 => "List",
            50009 => "Menu",
            50011 => "MenuItem",
            50013 => "RadioButton",
            50017 => "StatusBar",
            50018 => "Tab",
            50019 => "TabItem",
            50020 => "Text",
            50021 => "ToolBar",
            50022 => "ToolTip",
            50023 => "Tree",
            50024 => "TreeItem",
            50025 => "Custom",
            50026 => "Group",
            50028 => "DataGrid",
            50029 => "DataItem",
            50030 => "Document",
            50031 => "SplitButton",
            50032 => "Window",
            50033 => "Pane",
            50036 => "Table",
            50037 => "TitleBar",
            _ => "Other",
        }
    }

    fn node(element: &IUIAutomationElement, depth: usize, budget: &mut usize) -> Node {
        *budget = budget.saturating_sub(1);
        unsafe {
            let title = element
                .CachedName()
                .ok()
                .and_then(|b| names::clip(&b.to_string()));
            let help = element
                .CachedHelpText()
                .ok()
                .and_then(|b| names::clip(&b.to_string()))
                .filter(|help| Some(help) != title.as_ref());
            let mut node = Node {
                role: element
                    .CachedControlType()
                    .map(|t| role(t.0))
                    .unwrap_or("Other")
                    .to_owned(),
                description: help,
                title,
                id: element
                    .CachedAutomationId()
                    .ok()
                    .and_then(|b| names::clip(&b.to_string())),
                classes: element
                    .CachedClassName()
                    .map(|b| {
                        b.to_string()
                            .split_whitespace()
                            .take(20)
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default(),
                ..Node::default()
            };
            if depth >= MOST_DEPTH || *budget == 0 {
                return node;
            }
            if let Ok(children) = element.GetCachedChildren() {
                for i in 0..children.Length().unwrap_or(0) {
                    if *budget == 0 {
                        break;
                    }
                    if let Ok(child) = children.GetElement(i) {
                        node.children.push(self::node(&child, depth + 1, budget));
                    }
                }
            }
            node
        }
    }

    unsafe extern "system" fn collect(hwnd: HWND, list: LPARAM) -> BOOL {
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                (*(list.0 as *mut Vec<HWND>)).push(hwnd);
            }
        }
        true.into()
    }

    fn exe(pid: u32) -> Option<String> {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut path = [0u16; 1024];
            let mut size = path.len() as u32;
            let found = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(path.as_mut_ptr()),
                &mut size,
            );
            let _ = CloseHandle(process);
            found.ok()?;
            let path = String::from_utf16_lossy(path.get(..size as usize)?);
            std::path::Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
        }
    }

    fn title(hwnd: HWND) -> String {
        let mut text = [0u16; 512];
        let length = unsafe { GetWindowTextW(hwnd, &mut text) }.max(0) as usize;
        String::from_utf16_lossy(&text[..length.min(text.len())])
    }

    impl Reader for Win {
        fn read(&mut self) -> Look {
            let Some(uia) = &self.uia else {
                return Look::Unreadable("UI Automation is not available".into());
            };
            let mut list: Vec<HWND> = Vec::new();
            if unsafe { EnumWindows(Some(collect), LPARAM(&mut list as *mut Vec<HWND> as isize)) }
                .is_err()
            {
                return Look::Unreadable("could not list windows".into());
            }
            let mut seen = false;
            let mut app: Option<String> = None;
            let mut windows = Vec::new();
            let mut budget = BUDGET;
            let mut names_of: Vec<(u32, Option<String>)> = Vec::new();
            for hwnd in list {
                let mut pid = 0;
                unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
                if pid == 0 || pid == std::process::id() {
                    continue;
                }
                let program = match names_of.iter().find(|(p, _)| *p == pid) {
                    Some((_, program)) => program.clone(),
                    None => {
                        let program = exe(pid);
                        names_of.push((pid, program.clone()));
                        program
                    }
                };
                let Some(program) = program else {
                    continue;
                };
                let Some(kind) = names::app_kind(&program) else {
                    continue;
                };
                seen = true;
                if windows.len() == MOST_WINDOWS || app.as_ref().is_some_and(|a| *a != program) {
                    continue;
                }
                let title = title(hwnd);
                if !names::call_window(kind, &title, "") {
                    continue;
                }
                let Ok(element) =
                    (unsafe { uia.uia.ElementFromHandleBuildCache(hwnd, &uia.request) })
                else {
                    continue;
                };
                let tree = node(&element, 0, &mut budget);
                if kind == Kind::Teams && !names::is_call_tree(&tree) {
                    continue;
                }
                app = Some(program);
                windows.push(Window {
                    title,
                    id: String::new(),
                    tree,
                });
            }
            match app {
                Some(app) => Look::Call { app, windows },
                None if seen => Look::Unreadable("no call window is open".into()),
                None => Look::Unreadable("no call app is open".into()),
            }
        }
    }
}
