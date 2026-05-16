#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    apply_linux_workarounds();
    vpkmerge_gui_lib::run();
}

#[cfg(target_os = "linux")]
fn apply_linux_workarounds() {
    // WebKitGTK's DMABUF renderer triggers Wayland protocol errors on several
    // compositors and on Nvidia's proprietary driver. Disabling it picks a
    // fallback path that works reliably across distros at negligible cost
    // for a tool like this.
    set_env_default("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
}

#[cfg(not(target_os = "linux"))]
fn apply_linux_workarounds() {}

#[cfg(target_os = "linux")]
fn set_env_default(key: &str, value: &str) {
    if std::env::var_os(key).is_none() {
        // SAFETY: called during single-threaded program startup before any
        // threads are spawned, so the documented set_var data race cannot occur.
        unsafe { std::env::set_var(key, value) };
    }
}
