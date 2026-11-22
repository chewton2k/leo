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
        self as names, Ask, Kind, Look, Node, Reader, Seat, Seen, Window, BROWSERS, MOST_WINDOWS,
        TEAMS_APPS, ZOOM_APPS,
    };
    use objc2_app_kit::NSRunningApplication;
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
    const TIMEOUT: f32 = 0.3;
    const KEYS: [&str; 7] = [
        "AXRole",
        "AXSubrole",
        "AXDescription",
        "AXTitle",
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
        unsafe { AXUIElementSetMessagingTimeout(element, TIMEOUT) };
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
            AXUIElementSetMessagingTimeout(element, TIMEOUT);
            let value = if on { kCFBooleanTrue } else { kCFBooleanFalse };
            AXUIElementSetAttributeValue(element, key.0, value);
        }
    }

    pub struct Mac {
        keys: Owned,
        asked: Vec<i32>,
        enhanced: Vec<i32>,
    }

    struct Found {
        _app: Owned,
        _windows: Owned,
        pid: i32,
        id: &'static str,
        kind: Kind,
    }

    struct Candidate {
        found: usize,
        window: Ref,
        title: String,
        ident: String,
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
            ask: &Ask,
        ) -> Node {
            *budget = budget.saturating_sub(1);
            unsafe { AXUIElementSetMessagingTimeout(element, TIMEOUT) };
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
                value: if names::keeps_value(&role) {
                    text_of(attribute(element, "AXValue").0)
                } else {
                    None
                },
                id: text_of(at(4)),
                classes: items(at(5))
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
            for child in items(at(6)) {
                if *budget == 0 || ask.over() {
                    break;
                }
                node.children
                    .push(self.copy(child, depth + 1, budget, most_depth, zoom, ask));
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

        fn running(seat: Option<&Seat>) -> Vec<(i32, &'static str)> {
            ZOOM_APPS
                .iter()
                .chain(TEAMS_APPS)
                .chain(BROWSERS)
                .filter(|id| !id.ends_with(".exe"))
                .filter(|id| seat.is_none_or(|seat| seat.app == **id))
                .flat_map(|id| {
                    NSRunningApplication::runningApplicationsWithBundleIdentifier(
                        &NSString::from_str(id),
                    )
                    .iter()
                    .map(|app| (app.processIdentifier(), *id))
                    .collect::<Vec<_>>()
                })
                .filter(|(pid, _)| seat.is_none_or(|seat| seat.pid == *pid as u32))
                .collect()
        }

        fn tree(&self, window: Ref, kind: Kind, budget: &mut usize, ask: &Ask) -> Node {
            let zoom = kind == Kind::Zoom;
            let mut most = if zoom { (*budget).min(1500) } else { *budget };
            let before = most;
            let node = self.copy(window, 0, &mut most, if zoom { 14 } else { 40 }, zoom, ask);
            *budget -= before - most;
            node
        }
    }

    impl Reader for Mac {
        fn read(&mut self, ask: &Ask) -> Look {
            if !crate::permissions::accessibility() {
                return Look::Unreadable("no Accessibility permission".into());
            }
            let running = Self::running(ask.seat);
            if running.is_empty() {
                return Look::Unreadable(match ask.seat {
                    Some(_) => "the call window closed".into(),
                    None => "no call app is open".into(),
                });
            }
            let mut found: Vec<Found> = Vec::new();
            let mut candidates: Vec<Candidate> = Vec::new();
            for (pid, id) in running {
                if ask.over() {
                    return Look::Unreadable(names::SLOW.into());
                }
                let Some(kind) = names::app_kind(id) else {
                    continue;
                };
                let app = Owned(unsafe { AXUIElementCreateApplication(pid) });
                if app.0.is_null() {
                    continue;
                }
                let chromium = CHROMIUM.contains(&id);
                if chromium && kind == Kind::Teams {
                    self.ask(app.0, pid, false);
                }
                let windows = attribute(app.0, "AXWindows");
                let mine: Vec<Candidate> = items(windows.0)
                    .into_iter()
                    .map(|w| Candidate {
                        found: found.len(),
                        window: w,
                        title: text_of(attribute(w, "AXTitle").0).unwrap_or_default(),
                        ident: text_of(attribute(w, "AXIdentifier").0).unwrap_or_default(),
                    })
                    .filter(|c| names::call_window(kind, &c.title, &c.ident))
                    .take(MOST_WINDOWS)
                    .collect();
                if mine.is_empty() {
                    continue;
                }
                if chromium && kind != Kind::Teams {
                    self.ask(app.0, pid, true);
                    for c in &mine {
                        set(c.window, "AXManualAccessibility", true);
                    }
                }
                candidates.extend(mine);
                found.push(Found {
                    _app: app,
                    _windows: windows,
                    pid,
                    id,
                    kind,
                });
            }
            let mut budget = 4000;
            let window = |this: &Self, c: &Candidate, budget: &mut usize| Window {
                title: c.title.clone(),
                id: c.ident.clone(),
                tree: this.tree(c.window, found[c.found].kind, budget, ask),
            };
            let mut trees: Vec<Option<Window>> = Vec::new();
            if ask.seat.is_none() {
                for c in &candidates {
                    trees.push(Some(window(self, c, &mut budget)));
                }
            }
            let seen: Vec<Seen> = candidates
                .iter()
                .enumerate()
                .map(|(i, c)| Seen {
                    pid: found[c.found].pid as u32,
                    app: found[c.found].id.to_owned(),
                    title: c.title.clone(),
                    call: trees
                        .get(i)
                        .and_then(Option::as_ref)
                        .is_some_and(|w| names::shows_call(found[c.found].id, w)),
                })
                .collect();
            let (seat, at) = match names::pick(ask.seat, &seen) {
                Ok(picked) => picked,
                Err(why) => return Look::Unreadable(why.into()),
            };
            let mut windows = Vec::new();
            for i in at {
                windows.push(match trees.get_mut(i).and_then(Option::take) {
                    Some(tree) => tree,
                    None => window(self, &candidates[i], &mut budget),
                });
            }
            if ask.over() {
                return Look::Unreadable(names::SLOW.into());
            }
            Look::Call {
                app: seat.app.clone(),
                windows,
                seat,
            }
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
    use leo_core::call_names::{
        self as names, Ask, Look, Node, Reader, Seen, Window, MOST_WINDOWS,
    };
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
        CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationCacheRequest,
        IUIAutomationElement, IUIAutomationTreeWalker, TreeScope_Element,
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
        walker: IUIAutomationTreeWalker,
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
            request.SetTreeScope(TreeScope_Element)?;
            let walker = uia.RawViewWalker()?;
            Ok(Uia {
                uia,
                request,
                walker,
            })
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

    fn node(
        uia: &Uia,
        element: &IUIAutomationElement,
        depth: usize,
        budget: &mut usize,
        ask: &Ask,
    ) -> Node {
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
            let mut child = uia
                .walker
                .GetFirstChildElementBuildCache(element, &uia.request)
                .ok();
            while let Some(next) = child {
                if *budget == 0 || ask.over() {
                    break;
                }
                node.children
                    .push(self::node(uia, &next, depth + 1, budget, ask));
                child = uia
                    .walker
                    .GetNextSiblingElementBuildCache(&next, &uia.request)
                    .ok();
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
        fn read(&mut self, ask: &Ask) -> Look {
            let Some(uia) = &self.uia else {
                return Look::Unreadable("UI Automation is not available".into());
            };
            let mut list: Vec<HWND> = Vec::new();
            if unsafe { EnumWindows(Some(collect), LPARAM(&mut list as *mut Vec<HWND> as isize)) }
                .is_err()
            {
                return Look::Unreadable("could not list windows".into());
            }
            let mut seen_app = false;
            let mut seen: Vec<Seen> = Vec::new();
            let mut handles: Vec<HWND> = Vec::new();
            let mut names_of: Vec<(u32, Option<String>)> = Vec::new();
            for hwnd in list {
                let mut pid = 0;
                unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
                if pid == 0 || pid == std::process::id() {
                    continue;
                }
                if ask.seat.is_some_and(|seat| seat.pid != pid) {
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
                seen_app = true;
                let title = title(hwnd);
                let shown = seen.iter().filter(|s| s.pid == pid).count();
                if shown == MOST_WINDOWS || !names::call_window(kind, &title, "") {
                    continue;
                }
                seen.push(Seen {
                    pid,
                    app: program,
                    title,
                    call: false,
                });
                handles.push(hwnd);
            }
            let mut budget = BUDGET;
            let mut trees: Vec<Option<Window>> = Vec::new();
            if ask.seat.is_none() {
                for (i, hwnd) in handles.iter().enumerate() {
                    if ask.over() {
                        return Look::Unreadable(names::SLOW.into());
                    }
                    let tree = window_tree(uia, *hwnd, &mut budget, ask).map(|tree| Window {
                        title: seen[i].title.clone(),
                        id: String::new(),
                        tree,
                    });
                    let call = tree
                        .as_ref()
                        .is_some_and(|w| names::shows_call(&seen[i].app, w));
                    seen[i].call = call;
                    trees.push(tree);
                }
            }
            let (seat, at) = match names::pick(ask.seat, &seen) {
                Ok(picked) => picked,
                Err(_) if !seen_app && ask.seat.is_none() => {
                    return Look::Unreadable("no call app is open".into())
                }
                Err(why) => return Look::Unreadable(why.into()),
            };
            let mut windows = Vec::new();
            for i in at {
                let window = match trees.get_mut(i).and_then(Option::take) {
                    Some(window) => Some(window),
                    None => window_tree(uia, handles[i], &mut budget, ask).map(|tree| Window {
                        title: seen[i].title.clone(),
                        id: String::new(),
                        tree,
                    }),
                };
                windows.extend(window);
            }
            if ask.over() {
                return Look::Unreadable(names::SLOW.into());
            }
            Look::Call {
                app: seat.app.clone(),
                windows,
                seat,
            }
        }
    }

    fn window_tree(uia: &Uia, hwnd: HWND, budget: &mut usize, ask: &Ask) -> Option<Node> {
        let element = unsafe { uia.uia.ElementFromHandleBuildCache(hwnd, &uia.request) }.ok()?;
        Some(node(uia, &element, 0, budget, ask))
    }
}
