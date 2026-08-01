// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// Keys typed while the connecting welcome is up must reach the composer.
///
/// The frame is not interactive. A reader thread holds stdin across prefetch
/// and ACP connect, drops mouse reports, and replays keys as startup type-ahead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn frozen_connecting_frame_replays_keys_into_composer() {
    const PROBE: &str = "zzfrozen9";
    const CONNECTING: &str = "Connecting…";

    let content = ContentController::start().await.expect("start content");
    let binary = pager_binary().expect("resolve pager binary");
    let mut harness =
        PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
            .expect("spawn pager");

    harness
        .wait_until(
            "connecting toast on the frozen welcome",
            WELCOME_TIMEOUT,
            |h| h.contains_text(CONNECTING),
        )
        .expect("connecting frame painted");
    assert!(
        !harness.contains_text(PROBE),
        "probe was on screen before it was typed\n{}",
        harness.screen_contents()
    );

    // Mouse while the frame is up must not become composer text. Keys must.
    let mouse = mouse_drag_line(2, 1, 6);
    harness
        .inject_keys(mouse.as_bytes())
        .expect("mouse during connecting frame");
    harness
        .inject_keys(PROBE.as_bytes())
        .expect("keys during connecting frame");

    harness
        .wait_until(
            "composer holds keys typed during the connecting frame",
            WELCOME_TIMEOUT,
            |h| composer_holds(h, PROBE),
        )
        .unwrap_or_else(|err| panic!("{err}"));

    let composer_line = harness
        .screen_contents()
        .lines()
        .find(|line| line.contains('│') && line.contains(PROBE))
        .expect("composer line")
        .to_owned();
    assert!(
        !composer_line.contains("32;"),
        "mouse report leaked into the composer: {composer_line}"
    );
    assert_eq!(
        block_lines_containing(&harness, PROBE),
        0,
        "probe was submitted instead of staying in the composer\n{}",
        harness.screen_contents()
    );

    harness.quit().expect("quit");
}
