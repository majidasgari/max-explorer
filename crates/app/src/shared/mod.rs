//! CEF bootstrap: process dispatch, library loading, app start-up.

use cef::*;

pub mod browser;

#[cfg(target_os = "macos")]
pub type Library = library_loader::LibraryLoader;

#[cfg(not(target_os = "macos"))]
pub struct Library;

pub fn load_cef() -> Library {
    #[cfg(target_os = "macos")]
    let library = {
        let loader = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), false);
        assert!(loader.load());
        loader
    };

    #[cfg(not(target_os = "macos"))]
    let library = Library;

    // Initialize the CEF API version.
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    library
}

pub fn run_main(main_args: &MainArgs, cmd_line: &CommandLine, sandbox_info: *mut u8) {
    let switch = CefString::from("type");
    let is_browser_process = cmd_line.has_switch(Some(&switch)) != 1;

    let ret = execute_process(Some(main_args), None, sandbox_info);

    if is_browser_process {
        assert_eq!(ret, -1, "cannot execute browser process");
    } else {
        // render/GPU/utility processes do not initialize CEF themselves
        assert!(ret >= 0, "cannot execute non-browser process");
        return;
    }

    let state = browser::BrowserState::new();
    browser::install(state);

    let mut app = browser::BrowserApp::new();

    // shortcut: sandbox off for the dev shell, turn it on before shipping
    let (resources, locales) = match std::env::var("MAX_EXPLORER_CEF_DIR") {
        Ok(dir) => (
            CefString::from(format!("{dir}/").as_str()),
            CefString::from(format!("{dir}/locales").as_str()),
        ),
        Err(_) => (CefString::default(), CefString::default()),
    };
    let cache = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("max-explorer");
    let _ = std::fs::create_dir_all(&cache);

    let settings = Settings {
        no_sandbox: 1,
        resources_dir_path: resources,
        locales_dir_path: locales,
        root_cache_path: CefString::from(cache.to_string_lossy().as_ref()),
        ..Default::default()
    };
    assert_eq!(
        initialize(
            Some(main_args),
            Some(&settings),
            Some(&mut app),
            sandbox_info,
        ),
        1
    );

    run_message_loop();

    shutdown();
}
