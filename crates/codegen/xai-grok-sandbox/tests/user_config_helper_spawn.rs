//! The settings helper must outlive SIGINT delivered to the caller's process group.
//!
//! `install_user_config_writer` runs on the multi-thread tokio runtime. A raw
//! `fork` there is not async-signal-safe, and the grandchild stays in the
//! terminal process group, so the default SIGINT disposition kills it while
//! the TUI keeps running. This binary is also the helper entry (re-exec).

fn main() {
    #[cfg(target_os = "linux")]
    {
        if xai_grok_sandbox::run_user_config_helper_if_requested() {
            return;
        }
        if let Err(err) = spawned_helper_survives_parent_group_sigint() {
            eprintln!("user_config_helper_spawn: {err}");
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "linux")]
fn spawned_helper_survives_parent_group_sigint() -> std::io::Result<()> {
    let home = tempfile::tempdir().expect("tempdir");
    // This test is the outer process, even when the runner itself is sandboxed.
    unsafe {
        std::env::remove_var("__GROK_INSIDE_BWRAP");
        std::env::remove_var("GROK_USER_CONFIG_WRITE_FD");
        std::env::remove_var("GROK_USER_CONFIG_WRITE_TOKEN_FD");
        std::env::set_var("GROK_HOME", home.path());
    }

    let my_pid = unsafe { libc::getpid() };
    if unsafe { libc::setpgid(0, 0) } != 0 {
        return Err(std::io::Error::last_os_error());
    }

    xai_grok_sandbox::install_user_config_writer()?;

    let slot = home.path().join(xai_grok_config::USER_CONFIG_FILENAME);
    write_ok(&slot, "before = 1\n")?;

    // Only this process group. Do not signal pid 0 (that would hit cargo).
    unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
    if unsafe { libc::kill(-my_pid, libc::SIGINT) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    wait_until_alone_in_group(my_pid)?;

    write_ok(&slot, "after = 1\n")?;
    let body = std::fs::read_to_string(&slot)?;
    if body != "after = 1\n" {
        return Err(std::io::Error::other(format!(
            "helper write lost after SIGINT: {body:?}"
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn write_ok(slot: &std::path::Path, content: &str) -> std::io::Result<()> {
    xai_grok_sandbox::write_user_config_if_live(slot, content)
        .ok_or_else(|| std::io::Error::other("user-config helper is not live"))?
}

#[cfg(target_os = "linux")]
fn wait_until_alone_in_group(pgid: libc::pid_t) -> std::io::Result<()> {
    let start = std::time::Instant::now();
    loop {
        let others = pgroup_members(pgid)?
            .into_iter()
            .filter(|pid| *pid != pgid)
            .count();
        if others == 0 {
            return Ok(());
        }
        if start.elapsed() > std::time::Duration::from_secs(2) {
            return Err(std::io::Error::other(format!(
                "process group {pgid} still has {others} other member(s) after SIGINT"
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(target_os = "linux")]
fn pgroup_members(pgid: libc::pid_t) -> std::io::Result<Vec<libc::pid_t>> {
    let mut members = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<libc::pid_t>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        if stat_pgrp(&stat) == Some(pgid) {
            members.push(pid);
        }
    }
    Ok(members)
}

/// `stat` is `pid (comm) state ppid pgrp ...`. `comm` may contain spaces.
#[cfg(target_os = "linux")]
fn stat_pgrp(stat: &str) -> Option<libc::pid_t> {
    let rest = stat.rsplit_once(')')?.1;
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    let _ppid = fields.next()?;
    fields.next()?.parse().ok()
}
