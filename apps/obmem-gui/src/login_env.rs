//! An app opened from Finder starts with launchd's short PATH (`/usr/bin:/bin:/usr/sbin:/sbin`),
//! which misses where `obmem` and the Obsidian CLI live (`~/.cargo/bin`, `/usr/local/bin`).
//! Agents run both from the user's shell, so the app takes its PATH from a login shell, as VS
//! Code does. Windows apps already get the user's PATH.

#[cfg(target_os = "macos")]
pub(crate) fn adopt_login_path() {
    if let Some(path) = login_shell_path() {
        // SAFETY: `prepare_process` runs before the engine or any other thread starts (the
        // runners call `native` first, `main` calls it first), so nothing reads the environment
        // at the same time.
        unsafe { std::env::set_var("PATH", path) };
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn adopt_login_path() {}

#[cfg(target_os = "macos")]
fn login_shell_path() -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    // Markers around the value: an interactive shell's profile may print its own output.
    const MARK: &str = "__OBMEM_PATH__";
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut child = Command::new(shell)
        .args(["-ilc", &format!("printf '{MARK}%s{MARK}' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // A profile that waits for something must not keep the app from starting.
    let start = Instant::now();
    while child.try_wait().ok()?.is_none() {
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    let from = out.find(MARK)? + MARK.len();
    let to = from + out[from..].find(MARK)?;
    Some(out[from..to].to_string()).filter(|p| !p.is_empty())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn a_login_shell_reports_its_path() {
        let path = super::login_shell_path().expect("the login shell answered");
        assert!(path.split(':').any(|p| p == "/usr/bin"), "{path}");
    }
}
