//! Privileged writer for user-initiated `$GROK_HOME/config.toml` persist.
//!
//! Sandbox profiles kernel-write-deny trust-boundary files (H1-3969489) so the agent cannot
//! change settings, hooks, or trust via tools or bash. Landlock/Seatbelt/bwrap apply to the
//! whole grok process, so the TUI persist path would fail too.
//!
//! This helper is forked *before* bwrap re-exec / Seatbelt apply. It stays outside the
//! sandbox and accepts only user-config writes over an inherited socketpair. Agent path
//! opens still hit the kernel deny. The socket fd is CLOEXEC in the inner process so bash
//! children do not inherit it.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
#[cfg(unix)]
use std::os::unix::net::UnixStream;

const ENV_FD: &str = "GROK_USER_CONFIG_WRITE_FD";
const MAGIC: u32 = 0x3143_4B47; // "GKC1"
const MAX_DEST_BYTES: u32 = 4096;
const MAX_CONTENT_BYTES: u32 = 8 * 1024 * 1024;

static CLIENT: OnceLock<Mutex<UnixStream>> = OnceLock::new();

/// Fork the helper (or adopt the fd after bwrap re-exec). Safe to call more than once.
pub fn install_user_config_writer() -> io::Result<()> {
    if CLIENT.get().is_some() {
        return Ok(());
    }
    if crate::is_inside_bwrap() || std::env::var_os(ENV_FD).is_some() {
        return adopt_from_env();
    }
    spawn_helper()
}

/// Write `content` to `dest` via the helper when it is live and `dest` is user `config.toml`.
/// Returns `None` when the caller should write directly (no helper, or not user config).
pub fn write_user_config_if_live(dest: &Path, content: &str) -> Option<io::Result<()>> {
    CLIENT.get()?;
    if !dest_is_user_config_at(&xai_grok_config::grok_home(), dest) {
        return None;
    }
    Some(client_write(dest, content))
}

/// `execvp` a `Command` without closing the writer socket (Rust's `Command::exec` would).
#[cfg(unix)]
pub fn exec_command_preserving_user_config_writer(cmd: std::process::Command) -> io::Error {
    inherit_writer_fd_across_exec();
    exec_unix_command(cmd)
}

fn dest_is_user_config_at(home: &Path, path: &Path) -> bool {
    let slot = user_config_slot(home);
    if path == slot {
        return true;
    }
    match xai_grok_config::fs_atomic::bind_follow_destination(&slot) {
        Ok(bound) => bound.as_path() == path,
        Err(_) => false,
    }
}

fn user_config_slot(home: &Path) -> PathBuf {
    home.join(xai_grok_config::USER_CONFIG_FILENAME)
}

fn write_allowed_user_config(home: &Path, requested: &Path, content: &str) -> io::Result<()> {
    if !dest_is_user_config_at(home, requested) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "privileged writer: dest is not user config.toml",
        ));
    }
    if content.len() > MAX_CONTENT_BYTES as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "privileged writer: content too large",
        ));
    }
    let slot = user_config_slot(home);
    let dest = xai_grok_config::fs_atomic::bind_follow_destination(&slot)?;
    if let Some(parent) = dest.as_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    xai_grok_config::fs_atomic::write_atomically_bound(&dest, content, None)
}

fn spawn_helper() -> io::Result<()> {
    let (server, client) = socketpair_cloexec()?;
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::last_os_error());
    }
    if pid == 0 {
        drop(client);
        let pid2 = unsafe { libc::fork() };
        if pid2 < 0 {
            unsafe { libc::_exit(1) };
        }
        if pid2 > 0 {
            unsafe { libc::_exit(0) };
        }
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            helper_loop_at(&xai_grok_config::grok_home(), server);
        }));
        unsafe { libc::_exit(0) };
    }
    drop(server);
    let mut status = 0;
    let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
    if CLIENT.set(Mutex::new(client)).is_err() {
        return Err(io::Error::other("user-config writer already installed"));
    }
    Ok(())
}

fn helper_loop_at(home: &Path, mut stream: UnixStream) {
    loop {
        match read_request(&mut stream) {
            Ok((dest, content)) => {
                let result = write_allowed_user_config(home, Path::new(&dest), &content);
                if write_response(&mut stream, result).is_err() {
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(_) => break,
        }
    }
}

fn client_write(dest: &Path, content: &str) -> io::Result<()> {
    let Some(client) = CLIENT.get() else {
        return Err(io::Error::other("user-config writer is not installed"));
    };
    let mut guard = client
        .lock()
        .map_err(|_| io::Error::other("user-config writer lock poisoned"))?;
    write_request(&mut *guard, dest, content)?;
    read_response(&mut *guard)
}

fn write_request(w: &mut impl Write, dest: &Path, content: &str) -> io::Result<()> {
    let dest = dest.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "privileged writer: dest is not UTF-8",
        )
    })?;
    let dest_bytes = dest.as_bytes();
    let content_bytes = content.as_bytes();
    if dest_bytes.len() > MAX_DEST_BYTES as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "privileged writer: dest too long",
        ));
    }
    if content_bytes.len() > MAX_CONTENT_BYTES as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "privileged writer: content too large",
        ));
    }
    w.write_all(&MAGIC.to_le_bytes())?;
    w.write_all(&(dest_bytes.len() as u32).to_le_bytes())?;
    w.write_all(dest_bytes)?;
    w.write_all(&(content_bytes.len() as u32).to_le_bytes())?;
    w.write_all(content_bytes)?;
    w.flush()
}

fn read_request(r: &mut impl Read) -> io::Result<(String, String)> {
    let magic = read_u32(r)?;
    if magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "privileged writer: bad magic",
        ));
    }
    let dest_len = read_u32(r)?;
    if dest_len == 0 || dest_len > MAX_DEST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "privileged writer: dest length",
        ));
    }
    let dest = read_exact_string(r, dest_len)?;
    let content_len = read_u32(r)?;
    if content_len > MAX_CONTENT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "privileged writer: content length",
        ));
    }
    let content = read_exact_string(r, content_len)?;
    Ok((dest, content))
}

fn write_response(w: &mut impl Write, result: io::Result<()>) -> io::Result<()> {
    match result {
        Ok(()) => {
            w.write_all(&[0u8])?;
            w.write_all(&0u32.to_le_bytes())?;
        }
        Err(e) => {
            let msg = e.to_string();
            let bytes = msg.as_bytes();
            let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
            w.write_all(&[1u8])?;
            w.write_all(&len.to_le_bytes())?;
            w.write_all(bytes)?;
        }
    }
    w.flush()
}

fn read_response(r: &mut impl Read) -> io::Result<()> {
    let mut status = [0u8; 1];
    r.read_exact(&mut status)?;
    let msg_len = read_u32(r)?;
    let msg = if msg_len == 0 {
        String::new()
    } else {
        read_exact_string(r, msg_len)?
    };
    match status.first().copied() {
        Some(0) => Ok(()),
        Some(_) => Err(io::Error::other(if msg.is_empty() {
            "privileged writer failed".to_string()
        } else {
            msg
        })),
        None => Err(io::Error::other("privileged writer: empty status")),
    }
}

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_exact_string(r: &mut impl Read, len: u32) -> io::Result<String> {
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "privileged writer: not UTF-8"))
}

fn adopt_from_env() -> io::Result<()> {
    let raw = std::env::var(ENV_FD).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "inside bwrap but GROK_USER_CONFIG_WRITE_FD is unset",
        )
    })?;
    let fd: RawFd = raw.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "GROK_USER_CONFIG_WRITE_FD is not an fd",
        )
    })?;
    adopt_fd(fd)
}

fn adopt_fd(fd: RawFd) -> io::Result<()> {
    if !fd_is_socket(fd) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "GROK_USER_CONFIG_WRITE_FD is not a socket",
        ));
    }
    set_cloexec(fd, true)?;
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    unsafe { std::env::remove_var(ENV_FD) };
    CLIENT
        .set(Mutex::new(stream))
        .map_err(|_| io::Error::other("user-config writer already installed"))?;
    Ok(())
}

fn inherit_writer_fd_across_exec() {
    let Some(client) = CLIENT.get() else {
        return;
    };
    let Ok(guard) = client.lock() else {
        return;
    };
    let fd = guard.as_raw_fd();
    let _ = set_cloexec(fd, false);
    unsafe { std::env::set_var(ENV_FD, fd.to_string()) };
}

fn fd_is_socket(fd: RawFd) -> bool {
    let mut st = unsafe { std::mem::zeroed::<libc::stat>() };
    let rc = unsafe { libc::fstat(fd, &mut st) };
    rc == 0 && (st.st_mode & libc::S_IFMT) == libc::S_IFSOCK
}

fn set_cloexec(fd: RawFd, on: bool) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    let new_flags = if on {
        flags | libc::FD_CLOEXEC
    } else {
        flags & !libc::FD_CLOEXEC
    };
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFD, new_flags) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn socketpair_cloexec() -> io::Result<(UnixStream, UnixStream)> {
    let mut fds = [0 as RawFd; 2];
    #[cfg(target_os = "linux")]
    let sock_type = libc::SOCK_STREAM | libc::SOCK_CLOEXEC;
    #[cfg(not(target_os = "linux"))]
    let sock_type = libc::SOCK_STREAM;
    let rc = unsafe { libc::socketpair(libc::AF_UNIX, sock_type, 0, fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    let [a, b] = fds;
    #[cfg(not(target_os = "linux"))]
    {
        set_cloexec(a, true)?;
        set_cloexec(b, true)?;
    }
    unsafe { Ok((UnixStream::from_raw_fd(a), UnixStream::from_raw_fd(b))) }
}

#[cfg(unix)]
fn exec_unix_command(cmd: std::process::Command) -> io::Error {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    for (key, value) in cmd.get_envs() {
        match value {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
    }
    let program = cmd.get_program();
    let Ok(c_program) = CString::new(program.as_bytes()) else {
        return io::Error::new(io::ErrorKind::InvalidInput, "bwrap program contains NUL");
    };
    let mut c_args = Vec::new();
    c_args.push(c_program.clone());
    for arg in cmd.get_args() {
        match CString::new(arg.as_bytes()) {
            Ok(c) => c_args.push(c),
            Err(_) => {
                return io::Error::new(io::ErrorKind::InvalidInput, "bwrap arg contains NUL");
            }
        }
    }
    let mut ptrs: Vec<*const libc::c_char> = c_args.iter().map(|s| s.as_ptr()).collect();
    ptrs.push(std::ptr::null());
    unsafe { libc::execvp(c_program.as_ptr(), ptrs.as_ptr()) };
    io::Error::last_os_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;

    fn temp_home() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn dest_accepts_slot_and_rejects_sibling() {
        let home = temp_home();
        let slot = home.path().join("config.toml");
        fs::write(&slot, "old = true\n").unwrap();
        assert!(dest_is_user_config_at(home.path(), &slot));
        assert!(!dest_is_user_config_at(
            home.path(),
            &home.path().join("sandbox.toml")
        ));
        assert!(!dest_is_user_config_at(
            home.path(),
            &home.path().join("trusted_folders.toml")
        ));
    }

    #[test]
    fn dest_accepts_followed_leaf_symlink() {
        let home = temp_home();
        let slot = home.path().join("config.toml");
        let real = home.path().join("real.toml");
        fs::write(&real, "old = true\n").unwrap();
        std::os::unix::fs::symlink(&real, &slot).unwrap();
        assert!(dest_is_user_config_at(home.path(), &real));
        assert!(dest_is_user_config_at(home.path(), &slot));
    }

    #[test]
    fn write_allowed_replaces_user_config() {
        let home = temp_home();
        let slot = home.path().join("config.toml");
        fs::write(&slot, "old = true\n").unwrap();
        write_allowed_user_config(home.path(), &slot, "new = true\n").unwrap();
        assert_eq!("new = true\n", fs::read_to_string(&slot).unwrap());
    }

    #[test]
    fn write_allowed_rejects_other_files() {
        let home = temp_home();
        let other = home.path().join("sandbox.toml");
        fs::write(&other, "keep\n").unwrap();
        let err = write_allowed_user_config(home.path(), &other, "pwned\n").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!("keep\n", fs::read_to_string(&other).unwrap());
    }

    #[test]
    fn helper_loop_roundtrip_writes_and_rejects() {
        let home = temp_home();
        let slot = home.path().join("config.toml");
        fs::write(&slot, "old = 1\n").unwrap();
        let (server, mut client) = socketpair_cloexec().unwrap();
        let home_path = home.path().to_path_buf();
        let thread = thread::spawn(move || helper_loop_at(&home_path, server));

        write_request(&mut client, &slot, "ui.theme = \"dark\"\n").unwrap();
        read_response(&mut client).unwrap();
        assert_eq!("ui.theme = \"dark\"\n", fs::read_to_string(&slot).unwrap());

        let other = home.path().join("sandbox.toml");
        fs::write(&other, "keep\n").unwrap();
        write_request(&mut client, &other, "pwned\n").unwrap();
        let err = read_response(&mut client).unwrap_err();
        assert!(err.to_string().contains("not user config.toml"));
        assert_eq!("keep\n", fs::read_to_string(&other).unwrap());

        drop(client);
        thread.join().unwrap();
    }
}
