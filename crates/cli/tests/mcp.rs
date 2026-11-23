//! The MCP server stays up and reports a tool error without a live KiCad.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn the_server_lists_every_command_and_rejects_an_empty_test_directory() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pcbcraft"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pcbcraft mcp");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let _guard = KillOnDrop(&mut child);

    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
    );
    let hello = read_line(&mut stdout);
    assert!(hello.contains("tools"), "{hello}");

    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    );
    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    );
    let listed = read_line(&mut stdout);
    for name in [
        "board_setup",
        "export_bom",
        "export_pos",
        "export_cpl",
        "export_gerbers",
        "export_drawing",
        "export_2d",
        "tests_run",
        "tests_list",
        "tests_init_defaults",
        "tests_init_type",
        "tests_single_run",
        "parts_alternatives",
        "parts_search",
        "parts_download_model",
        "parts_download_datasheet",
        "components_sync",
        "components_show",
        "net_dump",
        "geometry_dump",
        "geometry_is_occupied",
        "geometry_find_symbol",
    ] {
        assert!(
            listed.contains(&format!("\"name\":\"{name}\"")),
            "{name} missing from {listed}"
        );
    }
    assert!(!listed.contains("\"name\":\"mcp\""), "{listed}");

    let dir = std::env::temp_dir().join(format!(
        "pcbcraft-mcp-live-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let cwd = dir.display().to_string().replace('\\', "\\\\");
    send(
        &mut stdin,
        &format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"tests_run","arguments":{{"cwd":"{cwd}","directory":"{cwd}"}}}}}}"#
        ),
    );
    let failed = read_line(&mut stdout);
    assert!(failed.contains("\"isError\":true"), "{failed}");
    assert!(failed.contains("no JSON"), "{failed}");

    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"tests_run","arguments":{"cwd":"relative"}}}"#,
    );
    let relative = read_line(&mut stdout);
    assert!(relative.contains("\"isError\":true"), "{relative}");
    assert!(relative.contains("absolute"), "{relative}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn send(stdin: &mut impl Write, line: &str) {
    writeln!(stdin, "{line}").expect("write");
    stdin.flush().expect("flush");
}

fn read_line(stdout: &mut impl BufRead) -> String {
    let mut line = String::new();
    stdout.read_line(&mut line).expect("read");
    assert!(!line.is_empty(), "server closed stdout");
    line
}

struct KillOnDrop<'a>(&'a mut std::process::Child);

impl Drop for KillOnDrop<'_> {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
