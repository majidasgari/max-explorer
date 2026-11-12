//! The browser shell: one CEF Views window, our own tab strip and address bar,
//! and one `BrowserView` per tab. Everything here runs on the CEF UI thread,
//! which is also the only reason a plain `Mutex` is safe here.

use cef::*;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use max_explorer_core::normalize_url_input;

const TAB_WIDTH: i32 = 168;
const TAB_HEIGHT: i32 = 28;

static STATE: OnceLock<Arc<Mutex<BrowserState>>> = OnceLock::new();

pub fn install(state: Arc<Mutex<BrowserState>>) {
    let _ = STATE.set(state);
}

/// The browser state lives for the whole process, so callbacks can borrow it.
fn lock() -> Option<MutexGuard<'static, BrowserState>> {
    STATE.get()?.lock().ok()
}

fn cef_text(value: &CefStringUserfreeUtf16) -> String {
    CefString::from(value).to_string()
}

const START_HTML: &str = r#"<!doctype html><meta charset="utf-8"><title>New tab</title>
<style>
  :root { color-scheme: dark; }
  body { margin:0; height:100vh; display:flex; flex-direction:column;
         align-items:center; justify-content:center; gap:6px;
         background:#15181d; color:#e7eaf0; font:15px/1.6 system-ui, sans-serif; }
  h1 { font-size:20px; font-weight:600; letter-spacing:.3px; margin:0; }
  p  { color:#9aa4b2; margin:0 0 18px; }
  nav a { color:#8fb3ff; margin:0 10px; text-decoration:none; }
  nav a:hover { text-decoration:underline; }
</style>
<h1>max-explorer</h1>
<p>Type an address above, or press <b>+</b> for another tab.</p>
<nav><a href="https://kde.org">KDE</a><a href="https://github.com">GitHub</a><a href="https://wikipedia.org">Wikipedia</a></nav>"#;

/// Chromium refuses top-level `data:` URLs, so the start page is a real file.
fn start_page() -> String {
    let path = profile_dir().join("start.html");
    if let Err(err) = std::fs::write(&path, START_HTML) {
        eprintln!("cannot write start page: {err}");
    }
    format!("file://{}", path.display())
}

fn profile_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
        });
    let dir = base.join("max-explorer");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

struct Tab {
    id: usize,
    title: String,
    url: String,
    button: LabelButton,
    view: BrowserView,
    _client: Client,
}

pub struct BrowserState {
    tabs: Vec<Tab>,
    active: Option<usize>,
    next_id: usize,
    window: Option<Window>,
    root: Option<Panel>,
    tabbar: Option<Panel>,
    content: Option<Panel>,
    address: Option<Textfield>,
    closing: bool,
}

impl BrowserState {
    pub fn new() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            tabs: Vec::new(),
            active: None,
            next_id: 1,
            window: None,
            root: None,
            tabbar: None,
            content: None,
            address: None,
            closing: false,
        }))
    }

    fn is_closing(&self) -> bool {
        self.closing
    }

    /// Build the window content and open the first tab.
    fn boot(&mut self, window: Window) {
        let tabbar = panel_create(None).expect("tab bar");
        let tabbar_layout = tabbar
            .set_to_box_layout(Some(&BoxLayoutSettings {
                horizontal: 1,
                between_child_spacing: 4,
                inside_border_horizontal_spacing: 6,
                inside_border_vertical_spacing: 3,
                minimum_cross_axis_size: TAB_HEIGHT,
                cross_axis_alignment: AxisAlignment::STRETCH,
                ..Default::default()
            }))
            .expect("tab bar layout");

        let address_row = panel_create(None).expect("address row");
        let address_layout = address_row
            .set_to_box_layout(Some(&BoxLayoutSettings {
                horizontal: 1,
                between_child_spacing: 6,
                inside_border_horizontal_spacing: 6,
                inside_border_vertical_spacing: 3,
                minimum_cross_axis_size: 30,
                cross_axis_alignment: AxisAlignment::STRETCH,
                ..Default::default()
            }))
            .expect("address layout");

        let address = textfield_create(Some(&mut AddressDelegate::new())).expect("address field");
        let go = label_button_create(
            Some(&mut GoButtonDelegate::new()),
            Some(&CefString::from("Go")),
        )
        .expect("go button");
        address_row.add_child_view(Some(&mut View::from(&address)));
        address_row.add_child_view(Some(&mut View::from(&go)));
        address_layout.set_flex_for_view(Some(&mut View::from(&address)), 1);

        let content = panel_create(None).expect("content");
        content.set_to_fill_layout();

        let root = panel_create(None).expect("root");
        let root_layout = root
            .set_to_box_layout(Some(&BoxLayoutSettings {
                horizontal: 0,
                cross_axis_alignment: AxisAlignment::STRETCH,
                ..Default::default()
            }))
            .expect("root layout");
        root.add_child_view(Some(&mut View::from(&tabbar)));
        root.add_child_view(Some(&mut View::from(&address_row)));
        root.add_child_view(Some(&mut View::from(&content)));
        root_layout.set_flex_for_view(Some(&mut View::from(&content)), 1);

        // CEF windows size the child view themselves only in some styles, so
        // the root view is resized explicitly (see on_window_bounds_changed).
        window.add_child_view(Some(&mut View::from(&root)));
        window.set_title(Some(&CefString::from("max-explorer")));
        window.show();

        let bounds = window.bounds();
        root.set_bounds(Some(&Rect {
            x: 0,
            y: 0,
            width: bounds.width.max(800),
            height: bounds.height.max(600),
        }));
        self.window = Some(window);
        self.root = Some(root);
        self.tabbar = Some(tabbar);
        self.content = Some(content);
        self.address = Some(address);

        self.new_tab(&start_page());

        // tab controls sit after the tabs: [tab…] [+] spacer [×]
        if let Some(tabbar) = self.tabbar.clone() {
            let new_tab = label_button_create(
                Some(&mut NewTabDelegate::new()),
                Some(&CefString::from("+")),
            );
            let spacer = panel_create(None);
            let close_tab = label_button_create(
                Some(&mut CloseTabDelegate::new()),
                Some(&CefString::from("×")),
            );
            if let Some(button) = new_tab.as_ref() {
                tabbar.add_child_view(Some(&mut View::from(button)));
            }
            if let Some(spacer) = spacer.as_ref() {
                tabbar.add_child_view(Some(&mut View::from(spacer)));
                tabbar_layout.set_flex_for_view(Some(&mut View::from(spacer)), 1);
            }
            if let Some(button) = close_tab.as_ref() {
                tabbar.add_child_view(Some(&mut View::from(button)));
            }
        }

        if let Ok(count) = std::env::var("MAX_EXPLORER_SMOKE") {
            self.smoke(count.parse().unwrap_or(3));
        }
    }

    /// Dev hook: open `count` colour-coded tabs and cycle through them, so a
    /// screenshot series proves that tabs render and switch.
    fn smoke(&mut self, count: usize) {
        let colours = ["#b3224a", "#1f6f4a", "#2450a0", "#8a6d1f"];
        for i in 1..count.max(1) {
            let colour = colours[i % colours.len()];
            let path = profile_dir().join(format!("smoke-{i}.html"));
            let html = format!(
                "<!doctype html><meta charset=utf-8><title>Tab {i}</title>\
                 <body style=\"margin:0;height:100vh;display:grid;place-items:center;\
                 background:{colour};color:#fff;font:700 42px system-ui,sans-serif\">TAB {i}</body>"
            );
            if std::fs::write(&path, html).is_ok() {
                self.new_tab(&format!("file://{}", path.display()));
            }
        }
        self.select(self.tabs[0].id);
        let mut first = SmokeSwitch::new(0);
        post_delayed_task(ThreadId::UI, Some(&mut first), 2000);
    }

    fn select_nth(&mut self, index: usize) {
        let index = index % self.tabs.len().max(1);
        if let Some(tab) = self.tabs.get(index) {
            let id = tab.id;
            self.select(id);
        }
    }

    fn new_tab(&mut self, url: &str) {
        let (Some(content), Some(tabbar)) = (self.content.clone(), self.tabbar.clone()) else {
            return;
        };
        let id = self.next_id;
        self.next_id += 1;

        let client = TabClient::new(id);
        let mut client_handle = client.clone();
        let mut view_delegate = TabViewDelegate::new(RuntimeStyle::ALLOY);
        let settings = BrowserSettings::default();
        let url_string = CefString::from(url);
        let Some(view) = browser_view_create(
            Some(&mut client_handle),
            Some(&url_string),
            Some(&settings),
            None,
            None,
            Some(&mut view_delegate),
        ) else {
            return;
        };
        content.add_child_view(Some(&mut View::from(&view)));

        let mut button_delegate = TabButtonDelegate::new(id);
        let label = CefString::from("New tab");
        let Some(button) = label_button_create(Some(&mut button_delegate), Some(&label)) else {
            return;
        };
        button.set_id(id as i32);
        // tabs stay left of the "+" / "×" controls added in `boot`
        tabbar.add_child_view_at(Some(&mut View::from(&button)), self.tabs.len() as i32);

        self.tabs.push(Tab {
            id,
            title: String::from("New tab"),
            url: url.to_string(),
            button,
            view,
            _client: client,
        });
        self.select(id);
    }

    fn select(&mut self, id: usize) {
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        for (i, tab) in self.tabs.iter().enumerate() {
            let active = i == index;
            View::from(&tab.view).set_visible(i32::from(active));
            View::from(&tab.button).set_background_color(if active {
                0xffdbe6f8
            } else {
                0x00000000
            });
        }
        self.active = Some(index);
        self.sync_title();
        self.sync_address();
    }

    fn sync_title(&self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let title = self
            .active
            .and_then(|i| self.tabs.get(i))
            .map(|t| format!("{} — max-explorer", t.title))
            .unwrap_or_else(|| String::from("max-explorer"));
        window.set_title(Some(&CefString::from(title.as_str())));
    }

    fn sync_address(&mut self) {
        let Some(address) = self.address.clone() else {
            return;
        };
        let url = self
            .active
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.url.clone())
            .unwrap_or_default();
        address.set_text(Some(&CefString::from(url.as_str())));
    }

    fn load_active(&mut self, input: &str) {
        let target = normalize_url_input(input);
        if target.is_empty() {
            return;
        }
        let Some(index) = self.active else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let Some(browser) = tab.view.browser() else {
            return;
        };
        let Some(frame) = browser.main_frame() else {
            return;
        };
        tab.url = target.clone();
        frame.load_url(Some(&CefString::from(target.as_str())));
    }

    fn set_title(&mut self, id: usize, title: &str) {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) else {
            return;
        };
        tab.title = title.to_string();
        tab.button.set_text(Some(&CefString::from(title)));
    }

    fn set_url(&mut self, id: usize, url: &str) {
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        self.tabs[index].url = url.to_string();
        if self.active == Some(index) {
            self.sync_address();
        }
    }

    fn active_host(&self) -> Option<BrowserHost> {
        let index = self.active?;
        let tab = self.tabs.get(index)?;
        tab.view.browser().and_then(|b| b.host())
    }

    /// Remove a tab whose browser has finished closing. The returned tab is
    /// dropped by the caller, outside the state lock: dropping a BrowserView
    /// can call back into CEF.
    fn tab_closed(&mut self, id: usize) -> Option<Tab> {
        let index = self.tabs.iter().position(|t| t.id == id)?;
        let tab = self.tabs.remove(index);
        if let Some(content) = self.content.clone() {
            content.remove_child_view(Some(&mut View::from(&tab.view)));
        }
        if let Some(tabbar) = self.tabbar.clone() {
            tabbar.remove_child_view(Some(&mut View::from(&tab.button)));
        }

        if self.tabs.is_empty() {
            self.closing = true;
            quit_message_loop();
            return Some(tab);
        }
        if self.active == Some(index) {
            let next = index.min(self.tabs.len() - 1);
            let next_id = self.tabs[next].id;
            self.active = None;
            self.select(next_id);
        }
        Some(tab)
    }

    /// Mark the window as closing and hand back the browser hosts, so the
    /// caller can close them after the state lock is released.
    fn take_for_close(&mut self) -> Vec<BrowserHost> {
        self.closing = true;
        self.tabs
            .iter()
            .filter_map(|tab| tab.view.browser().and_then(|b| b.host()))
            .collect()
    }
}

// ---------------------------------------------------------------- delegates

wrap_window_delegate! {
    struct MainWindowDelegate { }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 1200, height: 800 }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let Some(window) = window.cloned() else { return };
            if let Some(mut state) = lock() {
                state.boot(window);
            }
        }

        fn can_close(&self, _window: Option<&mut Window>) -> i32 {
            let hosts = lock().map(|mut state| state.take_for_close()).unwrap_or_default();
            for host in hosts {
                host.close_browser(1);
            }
            0
        }

        fn initial_show_state(&self, _window: Option<&mut Window>) -> ShowState {
            ShowState::NORMAL
        }

        fn initial_bounds(&self, _window: Option<&mut Window>) -> Rect {
            Rect { x: 0, y: 0, width: 1200, height: 800 }
        }

        fn on_window_bounds_changed(
            &self,
            _window: Option<&mut Window>,
            new_bounds: Option<&Rect>,
        ) {
            let Some(bounds) = new_bounds else { return };
            if let Some(state) = lock()
                && let Some(root) = state.root.clone() {
                    root.set_bounds(Some(&Rect {
                        x: 0,
                        y: 0,
                        width: bounds.width,
                        height: bounds.height,
                    }));
                }
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::ALLOY
        }
    }
}

wrap_browser_view_delegate! {
    struct TabViewDelegate { runtime_style: RuntimeStyle }

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        fn browser_runtime_style(&self) -> RuntimeStyle {
            self.runtime_style
        }
    }
}

wrap_button_delegate! {
    struct TabButtonDelegate { id: usize }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: TAB_WIDTH, height: TAB_HEIGHT }
        }
    }

    impl ButtonDelegate {
        fn on_button_pressed(&self, _button: Option<&mut Button>) {
            if let Some(mut state) = lock() {
                state.select(self.id);
            }
        }
    }
}

wrap_button_delegate! {
    struct NewTabDelegate { }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 32, height: TAB_HEIGHT }
        }
    }

    impl ButtonDelegate {
        fn on_button_pressed(&self, _button: Option<&mut Button>) {
            let url = start_page();
            if let Some(mut state) = lock() {
                state.new_tab(&url);
            }
        }
    }
}

wrap_button_delegate! {
    struct CloseTabDelegate { }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 32, height: TAB_HEIGHT }
        }
    }

    impl ButtonDelegate {
        fn on_button_pressed(&self, _button: Option<&mut Button>) {
            let host = lock().and_then(|state| state.active_host());
            if let Some(host) = host {
                host.close_browser(0);
            }
        }
    }
}

wrap_button_delegate! {
    struct GoButtonDelegate { }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 48, height: 26 }
        }
    }

    impl ButtonDelegate {
        fn on_button_pressed(&self, _button: Option<&mut Button>) {
            let text = lock()
                .and_then(|state| state.address.clone())
                .map(|address| cef_text(&address.text()))
                .unwrap_or_default();
            if let Some(mut state) = lock() {
                state.load_active(&text);
            }
        }
    }
}

wrap_textfield_delegate! {
    struct AddressDelegate { }

    impl ViewDelegate {}

    impl TextfieldDelegate {
        fn on_key_event(
            &self,
            textfield: Option<&mut Textfield>,
            event: Option<&KeyEvent>,
        ) -> i32 {
            let Some(event) = event else { return 0 };
            if event.type_ != KeyEventType::RAWKEYDOWN || event.windows_key_code != 13 {
                return 0;
            }
            let text = textfield
                .map(|field| cef_text(&field.text()))
                .unwrap_or_default();
            if let Some(mut state) = lock() {
                state.load_active(&text);
            }
            1
        }
    }
}

wrap_client! {
    struct TabClient { id: usize }

    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(TabDisplayHandler::new(self.id))
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(TabLifeSpanHandler::new(self.id))
        }
    }
}

wrap_display_handler! {
    struct TabDisplayHandler { id: usize }

    impl DisplayHandler {
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            let title = title.map(CefString::to_string).unwrap_or_default();
            if let Some(mut state) = lock() {
                state.set_title(self.id, &title);
                state.sync_title();
            }
        }

        fn on_address_change(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            let url = url.map(CefString::to_string).unwrap_or_default();
            if let Some(mut state) = lock() {
                state.set_url(self.id, &url);
            }
        }
    }
}

wrap_life_span_handler! {
    struct TabLifeSpanHandler { id: usize }

    impl LifeSpanHandler {
        fn do_close(&self, _browser: Option<&mut Browser>) -> i32 {
            match lock() {
                Some(state) => i32::from(!state.is_closing()),
                None => 0,
            }
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            let mut task = ClosedTab::new(self.id);
            post_task(ThreadId::UI, Some(&mut task));
        }
    }
}

// Dev hook: advance to the next tab every two seconds (see `smoke`).
wrap_task! {
    struct SmokeSwitch { mark: usize }

    impl Task {
        fn execute(&self) {
            if let Some(mut state) = lock() {
                state.select_nth(self.mark);
            }
            let mut next = SmokeSwitch::new(self.mark + 1);
            post_delayed_task(ThreadId::UI, Some(&mut next), 2000);
        }
    }
}

wrap_task! {
    struct ClosedTab { id: usize }

    impl Task {
        fn execute(&self) {
            // the removed tab is dropped after the lock is released
            let removed = lock().and_then(|mut state| state.tab_closed(self.id));
            drop(removed);
        }
    }
}

// --------------------------------------------------------------------- app

// Alloy runtime style: CEF gives us the web contents only, so the tab strip
// and address bar are ours and the UI does not look like Chrome.
wrap_app! {
    pub struct BrowserApp { }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            if let Some(command_line) = command_line {
                command_line.append_switch(Some(&CefString::from("use-alloy-style")));
                // no GPU in the dev shell (and in most CI/Xvfb runs): software
                // compositing still paints pages, just slower
                if std::env::var("MAX_EXPLORER_NO_GPU").is_ok() {
                    command_line.append_switch(Some(&CefString::from("disable-gpu")));
                    command_line.append_switch(Some(&CefString::from("disable-gpu-compositing")));
                }
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(BrowserProcessHandlerImpl::new())
        }
    }
}

wrap_browser_process_handler! {
    struct BrowserProcessHandlerImpl { }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            let mut delegate = MainWindowDelegate::new();
            window_create_top_level(Some(&mut delegate));
        }
    }
}
