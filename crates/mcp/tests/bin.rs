//! The `wowdps-mcp` binary: usage, argument rejection, and a stdio session
//! that needs no daemon (`tools/list`), run as a subprocess.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::io::Write;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_wowdps-mcp");

#[test]
fn help_and_bad_arguments() {
    let out = Command::new(BIN).arg("--help").output().expect("run");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Usage:"), "{text}");
    assert!(text.contains("wowdps mcp"), "{text}");

    let out = Command::new(BIN).arg("--bogus").output().expect("run");
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown argument \"--bogus\""), "{err}");
    assert!(err.contains("Usage:"), "{err}");
    assert!(out.stdout.is_empty(), "stdout belongs to the protocol");
}

#[test]
fn a_stdio_session_answers_the_catalog_without_a_daemon() {
    let mut child = Command::new(BIN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n")
            .unwrap();
        // EOF ends the session cleanly.
    }
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "one reply line: {text}");
    assert!(lines[0].contains("\"list_fights\""), "{text}");
    assert!(lines[0].starts_with("{\"jsonrpc\":\"2.0\""), "{text}");
}

#[test]
fn a_broken_stdin_is_reported_on_stderr_with_status_1() {
    let mut child = Command::new(BIN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        // Not UTF-8: the line reader fails, and the server says so.
        stdin.write_all(&[0xff, 0xfe, b'\n']).unwrap();
    }
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("wowdps-mcp: "), "{err}");
    assert!(out.stdout.is_empty());
}
