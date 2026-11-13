#[cfg(not(target_os = "android"))]
pub use system::{get, set};

#[cfg(not(target_os = "android"))]
mod system {
    use arboard::Clipboard;
    use std::sync::{Mutex, OnceLock};

    static CLIPBOARD: OnceLock<Mutex<Clipboard>> = OnceLock::new();

    /// Returns the clipboard, creating it on first success.
    /// If creation fails, the next call tries again.
    fn clipboard() -> Option<&'static Mutex<Clipboard>> {
        if let Some(c) = CLIPBOARD.get() {
            return Some(c);
        }

        let new = Clipboard::new().ok()?;
        Some(CLIPBOARD.get_or_init(|| Mutex::new(new)))
    }

    pub fn get() -> Option<String> {
        clipboard()?.lock().ok()?.get_text().ok()
    }

    pub fn set(text: &str) -> bool {
        clipboard()
            .and_then(|c| c.lock().ok())
            .map(|mut c| c.set_text(text).is_ok())
            .unwrap_or(false)
    }
}

// Android's clipboard is reached only through Java, which nothing here does yet.
#[cfg(target_os = "android")]
pub fn get() -> Option<String> {
    None
}

#[cfg(target_os = "android")]
pub fn set(_text: &str) -> bool {
    false
}
