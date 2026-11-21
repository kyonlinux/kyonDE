use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use kyon_ipc::{Request, Response, Snapshot};

pub fn send(cmd: &str, args: serde_json::Value) -> Option<Snapshot> {
    let stream = UnixStream::connect(kyon_ipc::socket_path()).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(400))).ok()?;
    stream.set_write_timeout(Some(Duration::from_millis(400))).ok()?;
    let mut writer = stream.try_clone().ok()?;
    let request = serde_json::to_string(&Request {
        cmd: cmd.to_string(),
        args,
    })
    .ok()?;
    writer.write_all(request.as_bytes()).ok()?;
    writer.write_all(b"\n").ok()?;
    writer.flush().ok()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let response: Response = serde_json::from_str(line.trim()).ok()?;
    response.state
}

pub fn state() -> Option<Snapshot> {
    send("state", serde_json::Value::Null)
}

pub fn apply(args: serde_json::Value) -> Option<Snapshot> {
    send("apply", args)
}

pub fn spawn_self(subcommand: &str) {
    let exe = std::env::current_exe().unwrap_or_else(|_| "kyonshell".into());
    let _ = std::process::Command::new(exe).arg(subcommand).spawn();
}

pub fn run_command(command: &str, args: &[&str]) {
    let _ = std::process::Command::new(command).args(args).spawn();
}
