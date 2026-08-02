// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use std::process::Stdio;
use std::sync::Mutex;

use anyhow::anyhow;
use interprocess::local_socket::{
    prelude::*, tokio::prelude::*, GenericNamespaced, ListenerOptions, Name, Stream,
};
use regex::Regex;
#[cfg(not(debug_assertions))]
use tauri::api::dialog::blocking::message;
use tauri::{Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use codepage::to_encoding;
use encoding_rs::Encoding;
use windows::Win32::Globalization::GetOEMCP;

fn get_socket_name() -> Name<'static> {
    let id = "dev.yakex.q7z_ipc";
    let name = id.to_ns_name::<GenericNamespaced>().unwrap();
    name
}

fn matches_to_message(matches: tauri::api::cli::Matches) -> Option<String> {
    let input = matches.args.get("input")?;
    let output = matches.args.get("output")?;
    let filter_val = match matches.args.get("filter") {
        Some(f) if f.occurrences > 0 => match &f.value {
            serde_json::Value::String(s) => s.clone(),
            _ => return None,
        },
        _ => String::new(),
    };
    let input_val = match &input.value {
        serde_json::Value::String(s) => s.clone(),
        _ => return None,
    };
    let output_val = match &output.value {
        serde_json::Value::String(s) => s.clone(),
        _ => return None,
    };
    Some(format!("{}\0{}\0{}\n", input_val, output_val, filter_val))
}

#[derive(Default)]
struct AppState {
    current_job: Mutex<Option<(String, String)>>,
}

#[tauri::command]
fn current_job(state: State<AppState>) -> Option<(String, String)> {
    state.current_job.lock().unwrap().clone()
}

/// Parse a NUL-delimited `input\0output\0filter\n` message into a job tuple.
/// Returns None if the message is malformed (missing trailing newline or not
/// exactly three fields), so callers can reject it without panicking.
fn parse_message(message: &str) -> Option<(String, String, String)> {
    let payload = message.strip_suffix('\n')?;
    let parts: Vec<&str> = payload.split('\0').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].to_string(),
        parts[1].to_string(),
        parts[2].to_string(),
    ))
}

fn get_encoding() -> &'static Encoding {
    unsafe {
        let oem_codepage = GetOEMCP();
        to_encoding(oem_codepage.try_into().unwrap()).unwrap()
    }
}

// ref. https://qiita.com/takavfx/items/4743ceaf9fccc87eac52
// ref. https://www.reddit.com/r/rust/comments/16egg88/create_a_tauri_app_that_is_both_a_gui_and_a_cli/

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![current_job])
        .setup(|app| {
            let matches = app.get_cli_matches();
            let name = get_socket_name();
            match Stream::connect(name) {
                Ok(mut conn) => {
                    // server exists, forward this invocation's arguments to it
                    match matches {
                        Ok(matches) => {
                            let ipc_message = match matches_to_message(matches) {
                                Some(m) => m,
                                None => {
                                    return Err(anyhow!(
                                        "missing required arguments (input, output)"
                                    )
                                    .into());
                                }
                            };
                            if let Err(e) = conn.write_all(ipc_message.as_bytes()) {
                                eprintln!(
                                    "q7z: failed to forward arguments to existing process: {e}"
                                );
                            }
                            // TODO: if failed, restart server
                            let notice_message = "pass arguments to an existing process";
                            #[cfg(debug_assertions)]
                            println!("{}", notice_message);
                            let main_window = app.get_window("main");
                            tauri::async_runtime::spawn(async move {
                                #[cfg(not(debug_assertions))]
                                message(main_window.as_ref(), "q7z", notice_message);
                                main_window.unwrap().close().unwrap()
                            });
                            Ok(())
                        }
                        Err(_) => Err(anyhow!(
                            "no argments specified but there is another process, nothing to do"
                        )
                        .into()),
                    }
                }
                Err(_) => {
                    // server does not exist, become the persistent worker
                    app.get_window("main").unwrap().show().unwrap(); // show main window
                    let app_handle = app.handle();
                    let (tx, mut rx) = mpsc::channel::<(String, String, String)>(16);
                    let worker_handle = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        // Single serial worker: one job at a time, in arrival order.
                        while let Some((input, output, filter)) = rx.recv().await {
                            run_7z(&worker_handle, input, output, filter).await;
                        }
                    });
                    let listener_tx = tx.clone();
                    tauri::async_runtime::spawn(async move {
                        listen_for_ipc(listener_tx).await;
                    });
                    // The first invocation enqueues its own request through the same
                    // channel, so first and later invocations follow one path.
                    if let Ok(matches) = matches {
                        if let Some(ipc_message) = matches_to_message(matches) {
                            if let Some(job) = parse_message(&ipc_message) {
                                if let Err(e) = tx.try_send(job) {
                                    eprintln!("q7z: failed to enqueue first invocation: {e}");
                                }
                            }
                        }
                    }
                    Ok(())
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn listen_for_ipc(jobs_tx: mpsc::Sender<(String, String, String)>) {
    let name = get_socket_name();
    let opts = ListenerOptions::new().name(name);
    let listener = match opts.create_tokio() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("q7z: failed to bind IPC listener: {e}");
            return;
        }
    };
    loop {
        let stream = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("q7z: IPC accept failed: {e}");
                continue;
            }
        };
        let mut reader = BufReader::new(stream);
        let mut buffer = String::with_capacity(512);
        if let Err(e) = reader.read_line(&mut buffer).await {
            eprintln!("q7z: IPC read failed: {e}");
            continue;
        }
        let Some(job) = parse_message(&buffer) else {
            eprintln!("q7z: malformed IPC message ignored");
            continue;
        };
        println!(
            "Recieved: input:{} output:{} filter:{}",
            job.0, job.1, job.2
        );
        if let Err(e) = jobs_tx.send(job).await {
            eprintln!("q7z: failed to enqueue job (worker stopped?): {e}");
        }
    }
}

async fn run_7z(app_handle: &tauri::AppHandle, input: String, output: String, filter: String) {
    if let Some(state) = app_handle.try_state::<AppState>() {
        *state.current_job.lock().unwrap() = Some((input.clone(), output.clone()));
    }
    let _ = app_handle.emit_all("job", (&input, &output));

    let mut cmd = Command::new("7z.exe");
    cmd.raw_arg("x")
        .raw_arg(&input)
        .raw_arg(format!("-o{}", output))
        .raw_arg("-aou") // auto rename extracting file
        .raw_arg("-bsp1"); // set progress information to stdout
    if !filter.is_empty() {
        cmd.raw_arg(&filter);
    }
    let mut cmd = match cmd.stdout(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("q7z: failed to start 7z.exe (is it on PATH?): {e}");
            return;
        }
    };

    if let Some(ref mut stdout) = cmd.stdout {
        let converter = get_encoding();
        let mut reader = tokio::io::BufReader::new(stdout);
        let re = Regex::new(r"^\s*(\d+)%").unwrap();
        let mut buf: Vec<u8> = vec![];
        loop {
            match reader.read_until(b'\r', &mut buf).await {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    eprintln!("q7z: read error from 7z.exe stdout: {e}");
                    break;
                }
            }
            let (raw_line, _, _) = converter.decode(&buf);
            let linefeed = raw_line.chars().nth(0) == Some('\n');
            let line = raw_line.trim_start_matches('\n').trim_end_matches('\r');
            if let Some(caps) = re.captures(line) {
                let percent = caps.get(1).unwrap().as_str();
                if let Err(e) = app_handle.emit_all("percent", percent) {
                    eprintln!("q7z: failed to emit percent event: {e}");
                }
            }
            if line.contains("Everything is Ok") {
                if let Err(e) = app_handle.emit_all("percent", "100") {
                    eprintln!("q7z: failed to emit percent event: {e}");
                }
            }
            // TODO: Append log part
            // TODO: Show processing file
            println!("line:{} [{}] {}", line.len(), line, linefeed);
            buf = vec![];
        }
    }

    // Wait for process completion so completion is actually checked. Full
    // exit-status-based success/failure handling is T004.
    if let Err(e) = cmd.wait().await {
        eprintln!("q7z: failed to await 7z.exe exit: {e}");
    }
}
