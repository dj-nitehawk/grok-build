//! Privileged writer for user-initiated `$GROK_HOME/config.toml` persist.
//!
//! Sandbox profiles kernel-write-deny trust-boundary files (H1-3969489) so the agent cannot
//! change settings, hooks, or trust via tools or bash. Landlock/Seatbelt/bwrap apply to the
//! whole grok process, so the TUI persist path would fail too.
//!
//! This helper is re-exec'd *before* bwrap re-exec / Seatbelt apply (not `fork` from the
//! multi-thread runtime). It stays outside the sandbox and accepts only user-config writes
//! over an inherited socketpair. Agent path
//! opens still hit the kernel deny. CLOEXEC stops bash from inheriting the socket, but a
//! same-uid child can still dup it via `/proc/<pid>/fd`. Each request carries a 32-byte
//! token. Across the bwrap re-exec that token rides only in a pipe
//! (`GROK_USER_CONFIG_WRITE_TOKEN_FD`), never in the environment: `/proc/<pid>/environ`
//! keeps the initial block, so `remove_var` would not hide an env token from a later
//! child. Adopt reads the pipe and closes it before any tool runs. The helper also
//! refuses a `config.toml` symlink that resolves outside grok home.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
#[cfg(unix)]
use std::os::unix::net::UnixStream;

const ENV_FD: &str = "GROK_USER_CONFIG_WRITE_FD";
/// Set only on the re-exec'd helper process. Never on the sandboxed client.
const ENV_HELPER: &str = "GROK_USER_CONFIG_HELPER";
/// Read end of the one-shot token pipe. The value is an fd number, never the token.
const ENV_TOKEN_FD: &str = "GROK_USER_CONFIG_WRITE_TOKEN_FD";
/// Legacy name. Never set. Scrubbed on adopt so an older handoff cannot linger in the
/// live environment passed to tool children (`/proc` still shows the initial block).
const ENV_TOKEN: &str = "GROK_USER_CONFIG_WRITE_TOKEN";
const MAGIC: u32 = 0x3143_4B47; // "GKC1"
const TOKEN_LEN: usize = 32;
const MAX_DEST_BYTES: u32 = 4096;
const MAX_CONTENT_BYTES: u32 = 8 * 1024 * 1024;

static CLIENT: OnceLock<Mutex<UnixStream>> = OnceLock::new();
static TOKEN: OnceLock<[u8; TOKEN_LEN]> = OnceLock::new();

/// Start the helper (or adopt the fd after bwrap re-exec). Safe to call more than once.
pub fn install_user_config_writer() -> io::Result<()> {
    if CLIENT.get().is_some() {
        return Ok(());
    }
    if crate::is_inside_bwrap() || std::env::var_os(ENV_FD).is_some() {
        return adopt_from_env();
    }
    spawn_helper()
}

/// If this process was re-exec'd as the settings helper, detach and serve
/// until the client socket closes. Does not return in that case.
///
/// Every binary that calls [`install_user_config_writer`] must call this
/// before it starts threads or touches the inherited helper fds.
pub fn run_user_config_helper_if_requested() -> bool {
    if std::env::var_os(ENV_HELPER).is_none() {
        return false;
    }
    let code = match helper_process_main() {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("user-config helper: {err}");
            1
        }
    };
    std::process::exit(code);
}

fn helper_process_main() -> io::Result<()> {
    let socket_fd: RawFd = std::env::var(ENV_FD)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "helper socket fd missing"))?;
    let token_fd: RawFd = std::env::var(ENV_TOKEN_FD)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "helper token fd missing"))?;
    // Drop the names before detach so a later exec cannot see them.
    scrub_handoff_env();
    unsafe { std::env::remove_var(ENV_HELPER) };

    // Fail before detach so the spawner observes a non-zero status.
    if !fd_is_socket(socket_fd) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "helper socket fd is not a socket",
        ));
    }
    // The re-exec cleared CLOEXEC so the fd would survive. Put it back before
    // the grandchild serves, so a later child of the helper cannot inherit it.
    set_cloexec(socket_fd, true)?;
    let token = read_token_fd(token_fd)?;

    // New session, not the caller's process group, so terminal SIGINT/SIGHUP
    // is not delivered here. Safe: this process has not started other threads.
    if unsafe { libc::setsid() } < 0 {
        return Err(io::Error::last_os_error());
    }
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
    }
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::last_os_error());
    }
    if pid > 0 {
        // The spawner waits for this short-lived process. The grandchild serves.
        std::process::exit(0);
    }
    let stream = unsafe { UnixStream::from_raw_fd(socket_fd) };
    helper_loop_at(&xai_grok_config::grok_home(), stream, &token);
    Ok(())
}

fn wait_helper_detach(child: &mut std::process::Child) -> io::Result<std::process::ExitStatus> {
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if start.elapsed() > std::time::Duration::from_secs(2) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "user-config helper did not detach",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
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
/// On exec failure the handoff is rolled back so the token pipe is not left open.
#[cfg(unix)]
pub fn exec_command_preserving_user_config_writer(cmd: std::process::Command) -> io::Error {
    let handoff = match begin_writer_fd_handoff() {
        Ok(handoff) => handoff,
        Err(e) => return e,
    };
    let err = exec_unix_command(cmd);
    if let Some(handoff) = handoff {
        handoff.rollback();
    }
    err
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

/// The followed leaf may not exist yet. Its parent must already be inside `home`
/// after symlink resolution, so a slot symlink cannot retarget the privileged write.
fn config_dest_stays_in_home(home: &Path, resolved: &Path) -> bool {
    let Ok(home) = dunce::canonicalize(home) else {
        return false;
    };
    let Some(parent) = resolved.parent() else {
        return false;
    };
    let Ok(parent) = dunce::canonicalize(parent) else {
        return false;
    };
    parent.starts_with(&home)
}

fn random_token() -> io::Result<[u8; TOKEN_LEN]> {
    let mut buf = [0u8; TOKEN_LEN];
    let mut file = std::fs::File::open("/dev/urandom")?;
    file.read_exact(&mut buf)?;
    Ok(buf)
}

fn tokens_match(presented: &[u8], expected: &[u8]) -> bool {
    if presented.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in presented.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
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
    if !config_dest_stays_in_home(home, dest.as_path()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "privileged writer: config symlink escapes grok home",
        ));
    }
    if let Some(parent) = dest.as_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    xai_grok_config::fs_atomic::write_atomically_bound(&dest, content, None)
}

/// Start the helper by re-execing this binary, not `libc::fork`.
///
/// `install_user_config_writer` runs from `async_main` on a multi-thread
/// tokio runtime. `fork` there only duplicates the calling thread and then
/// runs Rust in the child, which can deadlock on locks held by vanished
/// threads. The re-exec'd process is single-threaded when it detaches.
/// It also leaves the caller's process group so terminal SIGINT does not
/// kill the helper while the TUI is still running.
#[allow(clippy::disallowed_methods)] // intermediate child is waited; grandchild is detached and exits when the client socket closes
fn spawn_helper() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let (server, client) = socketpair_cloexec()?;
    let token = random_token()?;
    let token_fd = token_pipe_read_fd(&token)?;
    let server_fd = server.as_raw_fd();

    let mut cmd = std::process::Command::new(&exe);
    cmd.env(ENV_HELPER, "1")
        .env(ENV_FD, server_fd.to_string())
        .env(ENV_TOKEN_FD, token_fd.to_string())
        .env_remove(ENV_TOKEN)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: `pre_exec` runs in the child after fork and before exec. It only
    // calls `fcntl` (async-signal-safe). Parent descriptors stay CLOEXEC, so a
    // concurrent spawn cannot inherit the socket or the token pipe.
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(move || {
            set_cloexec(server_fd, false)?;
            set_cloexec(token_fd, false)?;
            Ok(())
        });
    }

    let spawned = cmd.spawn();
    // The child has its own copies. Close ours even when spawn fails.
    drop(server);
    unsafe { libc::close(token_fd) };
    let mut child = spawned?;
    let status = wait_helper_detach(&mut child)?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "user-config helper failed to detach ({status})"
        )));
    }
    TOKEN
        .set(token)
        .map_err(|_| io::Error::other("user-config writer token already installed"))?;
    if CLIENT.set(Mutex::new(client)).is_err() {
        return Err(io::Error::other("user-config writer already installed"));
    }
    Ok(())
}

fn helper_loop_at(home: &Path, mut stream: UnixStream, token: &[u8; TOKEN_LEN]) {
    loop {
        match read_request(&mut stream, token) {
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
    let token = TOKEN
        .get()
        .ok_or_else(|| io::Error::other("user-config writer token missing"))?;
    write_request(&mut *guard, dest, content, token)?;
    read_response(&mut *guard)
}

fn write_request(
    w: &mut impl Write,
    dest: &Path,
    content: &str,
    token: &[u8; TOKEN_LEN],
) -> io::Result<()> {
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
    w.write_all(token)?;
    w.write_all(&(dest_bytes.len() as u32).to_le_bytes())?;
    w.write_all(dest_bytes)?;
    w.write_all(&(content_bytes.len() as u32).to_le_bytes())?;
    w.write_all(content_bytes)?;
    w.flush()
}

fn read_request(r: &mut impl Read, token: &[u8; TOKEN_LEN]) -> io::Result<(String, String)> {
    let magic = read_u32(r)?;
    if magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "privileged writer: bad magic",
        ));
    }
    let mut presented = [0u8; TOKEN_LEN];
    r.read_exact(&mut presented)?;
    if !tokens_match(&presented, token) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "privileged writer: bad token",
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
    let token_raw = std::env::var(ENV_TOKEN_FD).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "inside bwrap but GROK_USER_CONFIG_WRITE_TOKEN_FD is unset",
        )
    })?;
    let token_fd: RawFd = token_raw.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "GROK_USER_CONFIG_WRITE_TOKEN_FD is not an fd",
        )
    })?;
    // Close the pipe before touching the socket. Drop the names so later
    // `Command` children do not inherit them. `/proc/self/environ` still shows
    // the fd numbers from the initial block; the pipe itself is gone.
    let token = read_token_fd(token_fd);
    scrub_handoff_env();
    adopt_fd(fd, token?)
}

fn adopt_fd(fd: RawFd, token: [u8; TOKEN_LEN]) -> io::Result<()> {
    if !fd_is_socket(fd) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "GROK_USER_CONFIG_WRITE_FD is not a socket",
        ));
    }
    set_cloexec(fd, true)?;
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    scrub_handoff_env();
    TOKEN
        .set(token)
        .map_err(|_| io::Error::other("user-config writer token already installed"))?;
    CLIENT
        .set(Mutex::new(stream))
        .map_err(|_| io::Error::other("user-config writer already installed"))?;
    Ok(())
}

struct WriterFdHandoff {
    socket_fd: RawFd,
    token_fd: RawFd,
}

impl WriterFdHandoff {
    fn rollback(self) {
        let _ = set_cloexec(self.socket_fd, true);
        unsafe { libc::close(self.token_fd) };
        scrub_handoff_env();
    }
}

fn scrub_handoff_env() {
    unsafe {
        std::env::remove_var(ENV_FD);
        std::env::remove_var(ENV_TOKEN_FD);
        std::env::remove_var(ENV_TOKEN);
    }
}

/// Publish the socket and a one-shot token pipe for the bwrap re-exec.
/// Environment values are fd numbers only.
///
/// bubblewrap 0.12 has no `--forward-fd` and would reject it. Non-CLOEXEC
/// fds still reach the sandboxed exec: the supervisor closes only its own
/// dups (`close_extra_fds` in the monitor). Do not add `--forward-fd`.
fn begin_writer_fd_handoff() -> io::Result<Option<WriterFdHandoff>> {
    let Some(client) = CLIENT.get() else {
        return Ok(None);
    };
    let Some(token) = TOKEN.get() else {
        return Ok(None);
    };
    let socket_fd = {
        let guard = client
            .lock()
            .map_err(|_| io::Error::other("user-config writer lock poisoned"))?;
        guard.as_raw_fd()
    };
    let token_fd = token_pipe_read_fd(token)?;
    if let Err(e) = set_cloexec(socket_fd, false) {
        unsafe { libc::close(token_fd) };
        return Err(e);
    }
    if let Err(e) = set_cloexec(token_fd, false) {
        let _ = set_cloexec(socket_fd, true);
        unsafe { libc::close(token_fd) };
        return Err(e);
    }
    unsafe {
        std::env::set_var(ENV_FD, socket_fd.to_string());
        std::env::set_var(ENV_TOKEN_FD, token_fd.to_string());
        std::env::remove_var(ENV_TOKEN);
    }
    Ok(Some(WriterFdHandoff {
        socket_fd,
        token_fd,
    }))
}

/// Write `token` into a pipe and return the read end. The write end is closed
/// so the reader sees EOF after exactly [`TOKEN_LEN`] bytes.
fn token_pipe_read_fd(token: &[u8; TOKEN_LEN]) -> io::Result<RawFd> {
    let mut fds = [0 as RawFd; 2];
    #[cfg(target_os = "linux")]
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(not(target_os = "linux"))]
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    let [read_fd, write_fd] = fds;
    #[cfg(not(target_os = "linux"))]
    {
        if let Err(e) = set_cloexec(read_fd, true).and_then(|_| set_cloexec(write_fd, true)) {
            unsafe {
                libc::close(read_fd);
                libc::close(write_fd);
            }
            return Err(e);
        }
    }
    let write_result = {
        let mut file = unsafe { std::fs::File::from_raw_fd(write_fd) };
        file.write_all(token)
    };
    if let Err(e) = write_result {
        unsafe { libc::close(read_fd) };
        return Err(e);
    }
    Ok(read_fd)
}

/// Take ownership of `fd`, read the token, and close it (including on error).
fn read_token_fd(fd: RawFd) -> io::Result<[u8; TOKEN_LEN]> {
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut token = [0u8; TOKEN_LEN];
    file.read_exact(&mut token)?;
    Ok(token)
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
    use std::ffi::{CString, OsString};
    use std::os::unix::ffi::OsStrExt;

    // `Command::exec` passes envp to execve. `set_var` here would leave
    // `__GROK_INSIDE_BWRAP` (and other Command env) in this process if execvp fails.
    struct EnvRestore(Vec<(OsString, Option<OsString>)>);
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (key, old) in self.0.drain(..) {
                match old {
                    Some(v) => unsafe { std::env::set_var(key, v) },
                    None => unsafe { std::env::remove_var(key) },
                }
            }
        }
    }

    let mut saved = Vec::new();
    for (key, value) in cmd.get_envs() {
        saved.push((key.to_os_string(), std::env::var_os(key)));
        match value {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
    }
    let _restore = EnvRestore(saved);

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
    fn write_allowed_follows_symlink_inside_home() {
        let home = temp_home();
        let slot = home.path().join("config.toml");
        let real = home.path().join("real.toml");
        fs::write(&real, "old = true\n").unwrap();
        std::os::unix::fs::symlink(&real, &slot).unwrap();
        write_allowed_user_config(home.path(), &slot, "new = true\n").unwrap();
        assert_eq!("new = true\n", fs::read_to_string(&real).unwrap());
    }

    #[test]
    fn write_allowed_rejects_symlink_outside_home() {
        let home = temp_home();
        let outside = temp_home();
        let target = outside.path().join("authorized_keys");
        fs::write(&target, "ssh-ed25519 keep\n").unwrap();
        let slot = home.path().join("config.toml");
        std::os::unix::fs::symlink(&target, &slot).unwrap();
        let err = write_allowed_user_config(home.path(), &slot, "pwned = true\n").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(err.to_string().contains("escapes grok home"), "{err}");
        assert_eq!("ssh-ed25519 keep\n", fs::read_to_string(&target).unwrap());
    }

    #[test]
    fn helper_loop_roundtrip_writes_and_rejects() {
        let home = temp_home();
        let slot = home.path().join("config.toml");
        fs::write(&slot, "old = 1\n").unwrap();
        let (server, mut client) = socketpair_cloexec().unwrap();
        let home_path = home.path().to_path_buf();
        let token = [7u8; TOKEN_LEN];
        let thread = thread::spawn(move || helper_loop_at(&home_path, server, &token));

        write_request(&mut client, &slot, "ui.theme = \"dark\"\n", &token).unwrap();
        read_response(&mut client).unwrap();
        assert_eq!("ui.theme = \"dark\"\n", fs::read_to_string(&slot).unwrap());

        let other = home.path().join("sandbox.toml");
        fs::write(&other, "keep\n").unwrap();
        write_request(&mut client, &other, "pwned\n", &token).unwrap();
        let err = read_response(&mut client).unwrap_err();
        assert!(err.to_string().contains("not user config.toml"));
        assert_eq!("keep\n", fs::read_to_string(&other).unwrap());

        drop(client);
        thread.join().unwrap();
    }

    #[test]
    fn helper_loop_rejects_wrong_token_without_writing() {
        let home = temp_home();
        let slot = home.path().join("config.toml");
        fs::write(&slot, "old = 1\n").unwrap();
        let (server, mut client) = socketpair_cloexec().unwrap();
        let home_path = home.path().to_path_buf();
        let token = [9u8; TOKEN_LEN];
        let thread = thread::spawn(move || helper_loop_at(&home_path, server, &token));

        let wrong = [1u8; TOKEN_LEN];
        write_request(&mut client, &slot, "pwned = true\n", &wrong).unwrap();
        let err = read_response(&mut client).unwrap_err();
        assert_ne!(err.kind(), io::ErrorKind::PermissionDenied, "{err}");
        assert_eq!("old = 1\n", fs::read_to_string(&slot).unwrap());

        drop(client);
        thread.join().unwrap();
    }

    #[test]
    fn token_pipe_roundtrip_closes_read_end_and_is_not_an_env_value() {
        let token = [0xA5u8; TOKEN_LEN];
        let fd = token_pipe_read_fd(&token).unwrap();
        let fd_num = fd;
        // Handoff publishes this decimal fd number, never the token bytes.
        let published = fd_num.to_string();
        assert!(published.len() < 16, "{published}");
        assert!(!published.contains("a5a5"), "{published}");
        assert_eq!(read_token_fd(fd).unwrap(), token);
        let rc = unsafe { libc::fcntl(fd_num, libc::F_GETFD) };
        assert_eq!(rc, -1, "token pipe must be closed after it is read");
    }

    #[test]
    fn handoff_rollback_closes_token_pipe_and_scrubs_env() {
        let (socket, _peer) = socketpair_cloexec().unwrap();
        let socket_fd = socket.as_raw_fd();
        let token_fd = token_pipe_read_fd(&[3u8; TOKEN_LEN]).unwrap();
        set_cloexec(socket_fd, false).unwrap();
        set_cloexec(token_fd, false).unwrap();
        unsafe {
            std::env::set_var(ENV_FD, socket_fd.to_string());
            std::env::set_var(ENV_TOKEN_FD, token_fd.to_string());
            std::env::set_var(ENV_TOKEN, "not-the-channel");
        }
        WriterFdHandoff {
            socket_fd,
            token_fd,
        }
        .rollback();
        assert!(std::env::var_os(ENV_FD).is_none());
        assert!(std::env::var_os(ENV_TOKEN_FD).is_none());
        assert!(std::env::var_os(ENV_TOKEN).is_none());
        let flags = unsafe { libc::fcntl(socket_fd, libc::F_GETFD) };
        assert!(flags >= 0 && (flags & libc::FD_CLOEXEC) != 0);
        assert_eq!(unsafe { libc::fcntl(token_fd, libc::F_GETFD) }, -1);
    }

    #[test]
    fn failed_exec_does_not_leave_command_env_in_process() {
        let marker = "GROK_USER_CONFIG_WRITER_EXEC_TEST";
        unsafe { std::env::remove_var(marker) };
        let mut cmd = std::process::Command::new("/no/such/grok-user-config-bwrap");
        cmd.env(marker, "1");
        let err = exec_command_preserving_user_config_writer(cmd);
        assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
        assert!(
            std::env::var_os(marker).is_none(),
            "failed exec left Command env in the process"
        );
    }
}
