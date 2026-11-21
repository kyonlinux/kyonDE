use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use kyon_ipc::{Request, Response};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: kyonctl <state|layout <mode|cycle>|workspace <n>|window <close|maximize|fullscreen|toggle-floating|move-output-next|move-output-prev>|focus <target>|spawn <cmd>|apply <json>|reload|quit>");
        std::process::exit(2);
    }
    let (cmd, value) = match args[0].as_str() {
        "state" => ("state", serde_json::Value::Null),
        "reload" => ("reload", serde_json::Value::Null),
        "quit" => ("quit", serde_json::Value::Null),
        "layout" => {
            let arg = args.get(1).map(String::as_str).unwrap_or("cycle");
            if arg == "cycle" {
                ("layout.cycle", serde_json::Value::Null)
            } else {
                ("layout", serde_json::json!(arg))
            }
        }
        "workspace" => {
            let index = args
                .get(1)
                .and_then(|s| s.parse::<usize>().ok())
                .map(|n| n.saturating_sub(1))
                .unwrap_or(0);
            ("workspace", serde_json::json!(index))
        }
        "window" => {
            let target = args.get(1).map(String::as_str).unwrap_or("next");
            ("window", serde_json::json!(target))
        }
        "focus" => {
            let target = args.get(1).map(String::as_str).unwrap_or("next");
            ("focus", serde_json::json!(target))
        }
        "spawn" => {
            let command = args[1..].join(" ");
            ("spawn", serde_json::json!(command))
        }
        "apply" => {
            let raw = args[1..].join(" ");
            match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(value) => ("apply", value),
                Err(err) => {
                    eprintln!("invalid json: {err}");
                    std::process::exit(2);
                }
            }
        }
        other => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    };

    let request = Request {
        cmd: cmd.into(),
        args: value,
    };
    let mut stream = match UnixStream::connect(kyon_ipc::socket_path()) {
        Ok(stream) => stream,
        Err(err) => {
            eprintln!("failed to connect to kyonde: {err}");
            std::process::exit(1);
        }
    };
    let payload = serde_json::to_string(&request).unwrap();
    if stream.write_all(payload.as_bytes()).is_err() || stream.write_all(b"\n").is_err() {
        eprintln!("failed to send request");
        std::process::exit(1);
    }
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        eprintln!("no response");
        std::process::exit(1);
    }
    match serde_json::from_str::<Response>(line.trim()) {
        Ok(response) => {
            if !response.ok {
                if let Some(error) = response.error {
                    eprintln!("error: {error}");
                    std::process::exit(1);
                }
            }
            if let Some(state) = response.state {
                if cmd == "state" {
                    println!(
                        "layout: {}  workspace: {}/{}  windows: {}  output: {}x{}  gpu: {}",
                        state.layout_mode,
                        state.active_workspace + 1,
                        state.workspace_count,
                        state.windows,
                        state.output_width,
                        state.output_height,
                        state.gpu_backend
                    );
                    for display in &state.displays {
                        println!(
                            "display: {} {}x{} @ {} Hz at {},{} scale {} enabled={} primary={}",
                            display.name,
                            display.width,
                            display.height,
                            display.refresh,
                            display.x,
                            display.y,
                            display.scale,
                            display.enabled,
                            display.primary
                        );
                    }
                    if let Some(title) = state.focused_title {
                        println!("focused: {title}");
                    }
                } else {
                    println!("ok");
                }
            }
        }
        Err(err) => {
            eprintln!("invalid response: {err}");
            std::process::exit(1);
        }
    }
}
