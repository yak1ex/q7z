#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, Write};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use anyhow::anyhow;
use interprocess::local_socket::tokio::{Listener, Stream as TokioStream};
use interprocess::local_socket::{
    prelude::*, tokio::prelude::*, GenericNamespaced, ListenerOptions, Name, Stream,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
#[cfg(not(debug_assertions))]
use tauri::api::dialog::blocking::message;
use tauri::{Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use codepage::to_encoding;
use encoding_rs::Encoding;
use windows::Win32::Globalization::GetOEMCP;

#[derive(Serialize, Deserialize)]
struct JobRequest {
    input: String,
    output: String,
    #[serde(default)]
    filter: String,
}

#[derive(Serialize, Deserialize)]
struct JobResponse {
    accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

struct Job {
    #[allow(dead_code)]
    id: u64,
    input: String,
    output: String,
    filter: String,
}

#[derive(Default)]
struct AppState {
    current_job: Mutex<Option<(String, String)>>,
    next_job_id: AtomicU64,
}

#[tauri::command]
fn current_job(state: State<AppState>) -> Option<(String, String)> {
    state.current_job.lock().unwrap().clone()
}

fn get_socket_name() -> Name<'static> {
    let id = "dev.yakex.q7z_ipc";
    id.to_ns_name::<GenericNamespaced>().unwrap()
}

fn create_listener() -> Result<Listener, std::io::Error> {
    tauri::async_runtime::block_on(async {
        ListenerOptions::new()
            .name(get_socket_name())
            .create_tokio()
    })
}

fn matches_to_request(matches: &tauri::api::cli::Matches) -> Option<JobRequest> {
    let input = matches.args.get("input")?;
    let output = matches.args.get("output")?;
    let input_val = match &input.value {
        serde_json::Value::String(s) => s.clone(),
        _ => return None,
    };
    let output_val = match &output.value {
        serde_json::Value::String(s) => s.clone(),
        _ => return None,
    };
    let filter_val = match matches.args.get("filter") {
        Some(f) if f.occurrences > 0 => match &f.value {
            serde_json::Value::String(s) => s.clone(),
            _ => return None,
        },
        _ => String::new(),
    };
    Some(JobRequest {
        input: input_val,
        output: output_val,
        filter: filter_val,
    })
}

fn parse_request(line: &str) -> Option<JobRequest> {
    serde_json::from_str(line).ok()
}

fn parse_response(line: &str) -> Option<JobResponse> {
    serde_json::from_str(line).ok()
}

fn validate_request(req: &JobRequest) -> Result<(), String> {
    if req.input.trim().is_empty() {
        return Err("input archive path is required and must not be empty".into());
    }
    if req.output.trim().is_empty() {
        return Err("output directory is required and must not be empty".into());
    }
    Ok(())
}

fn get_encoding() -> &'static Encoding {
    unsafe {
        let oem_codepage = GetOEMCP();
        to_encoding(oem_codepage.try_into().unwrap()).unwrap()
    }
}

fn forward_and_report(
    mut conn: Stream,
    matches: &tauri::api::cli::Matches,
    main_window: Option<tauri::Window>,
) -> Result<(), Box<dyn std::error::Error>> {
    let req = matches_to_request(matches)
        .ok_or_else(|| anyhow!("missing required arguments (input, output)"))?;
    let json = serde_json::to_string(&req).unwrap();
    conn.write_all(format!("{}\n", json).as_bytes())?;
    let mut buf = String::new();
    std::io::BufReader::new(&mut conn).read_line(&mut buf)?;
    let (result, message) = match parse_response(&buf) {
        Some(resp) if resp.accepted => (
            Ok(()),
            format!("Extraction queued as job #{}", resp.id.unwrap_or(0)),
        ),
        Some(resp) => {
            let msg = resp.error.unwrap_or_else(|| "request rejected".into());
            (Err(anyhow!("{}", msg).into()), msg)
        }
        None => (
            Err(anyhow!("malformed response from server").into()),
            "malformed response from server".to_string(),
        ),
    };

    let mw = main_window;
    #[cfg(debug_assertions)]
    {
        println!("{}", message);
        if let Some(w) = mw {
            let _ = w.close();
        }
    }
    #[cfg(not(debug_assertions))]
    {
        let msg = message;
        tauri::async_runtime::spawn(async move {
            message(mw.as_ref(), "q7z", &msg);
            if let Some(w) = mw {
                let _ = w.close();
            }
        });
    }

    result
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![current_job])
        .setup(|app| {
            let matches = app.get_cli_matches();
            let main_window = app.get_window("main");

            match Stream::connect(get_socket_name()) {
                Ok(conn) => match matches {
                    Ok(ref m) => {
                        forward_and_report(conn, m, main_window)?;
                        Ok(())
                    }
                    Err(_) => Err(anyhow!(
                        "no arguments specified but there is another process, nothing to do"
                    )
                    .into()),
                },
                Err(_) => match create_listener() {
                    Ok(listener) => {
                        app.get_window("main").unwrap().show().unwrap();
                        let app_handle = app.handle();
                        let (tx, mut rx) = mpsc::channel::<Job>(16);
                        let worker_handle = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            while let Some(job) = rx.recv().await {
                                run_7z(&worker_handle, job).await;
                            }
                        });
                        let listener_tx = tx.clone();
                        let listener_handle = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            listen_for_ipc(listener, listener_tx, listener_handle).await;
                        });
                        if let Ok(ref m) = matches {
                            if let Some(req) = matches_to_request(m) {
                                match validate_request(&req) {
                                    Ok(()) => {
                                        let id = app_handle
                                            .state::<AppState>()
                                            .next_job_id
                                            .fetch_add(1, Ordering::Relaxed);
                                        let job = Job {
                                            id,
                                            input: req.input,
                                            output: req.output,
                                            filter: req.filter,
                                        };
                                        if let Err(e) = tx.try_send(job) {
                                            eprintln!(
                                                "q7z: failed to enqueue first invocation: {e}"
                                            );
                                        }
                                    }
                                    Err(e) => eprintln!("q7z: {e}"),
                                }
                            }
                        }
                        Ok(())
                    }
                    Err(bind_err) => match Stream::connect(get_socket_name()) {
                        Ok(conn) => match matches {
                            Ok(ref m) => {
                                forward_and_report(conn, m, main_window)?;
                                Ok(())
                            }
                            Err(_) => Err(anyhow!(
                                "no arguments specified but there is another process, nothing to do"
                            )
                            .into()),
                        },
                        Err(_) => Err(anyhow!(
                            "failed to bind listener and no existing process: {bind_err}"
                        )
                        .into()),
                    },
                },
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn listen_for_ipc(
    listener: Listener,
    jobs_tx: mpsc::Sender<Job>,
    app_handle: tauri::AppHandle,
) {
    loop {
        let stream = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("q7z: IPC accept failed: {e}");
                continue;
            }
        };
        let tx = jobs_tx.clone();
        let handle = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            handle_connection(stream, tx, handle).await;
        });
    }
}

async fn handle_connection(
    mut stream: TokioStream,
    jobs_tx: mpsc::Sender<Job>,
    app_handle: tauri::AppHandle,
) {
    let mut buf = String::new();
    {
        let mut reader = BufReader::new(&mut stream);
        match reader.read_line(&mut buf).await {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) => {
                eprintln!("q7z: IPC read failed: {e}");
                return;
            }
        }
    }
    let resp = match parse_request(&buf) {
        None => JobResponse {
            accepted: false,
            id: None,
            error: Some("malformed request".into()),
        },
        Some(req) => match validate_request(&req) {
            Err(e) => JobResponse {
                accepted: false,
                id: None,
                error: Some(e),
            },
            Ok(()) => {
                let id = app_handle
                    .state::<AppState>()
                    .next_job_id
                    .fetch_add(1, Ordering::Relaxed);
                let job = Job {
                    id,
                    input: req.input,
                    output: req.output,
                    filter: req.filter,
                };
                match jobs_tx.send(job).await {
                    Err(e) => JobResponse {
                        accepted: false,
                        id: None,
                        error: Some(format!("queue closed: {e}")),
                    },
                    Ok(()) => JobResponse {
                        accepted: true,
                        id: Some(id),
                        error: None,
                    },
                }
            }
        },
    };
    let json = serde_json::to_string(&resp).unwrap();
    if let Err(e) = stream.write_all(format!("{}\n", json).as_bytes()).await {
        eprintln!("q7z: failed to send ack: {e}");
    }
}

async fn run_7z(app_handle: &tauri::AppHandle, job: Job) {
    let input = job.input;
    let output = job.output;
    let filter = job.filter;
    if let Some(state) = app_handle.try_state::<AppState>() {
        *state.current_job.lock().unwrap() = Some((input.clone(), output.clone()));
    }
    let _ = app_handle.emit_all("job", (&input, &output));

    let mut cmd = Command::new("7z.exe");
    cmd.raw_arg("x")
        .raw_arg(&input)
        .raw_arg(format!("-o{}", output))
        .raw_arg("-aou")
        .raw_arg("-bsp1");
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
            println!("line:{} [{}] {}", line.len(), line, linefeed);
            buf = vec![];
        }
    }

    if let Err(e) = cmd.wait().await {
        eprintln!("q7z: failed to await 7z.exe exit: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request_valid() {
        let line = r#"{"input":"a.7z","output":"out","filter":""}"#;
        let req = parse_request(line).unwrap();
        assert_eq!(req.input, "a.7z");
        assert_eq!(req.output, "out");
        assert_eq!(req.filter, "");
    }

    #[test]
    fn parse_request_default_filter() {
        let line = r#"{"input":"a.7z","output":"out"}"#;
        let req = parse_request(line).unwrap();
        assert_eq!(req.filter, "");
    }

    #[test]
    fn parse_request_with_filter() {
        let line = r#"{"input":"a.7z","output":"out","filter":"*.txt"}"#;
        let req = parse_request(line).unwrap();
        assert_eq!(req.filter, "*.txt");
    }

    #[test]
    fn parse_request_malformed_json() {
        assert!(parse_request("not json").is_none());
        assert!(parse_request("").is_none());
    }

    #[test]
    fn parse_response_accept() {
        let line = r#"{"accepted":true,"id":5}"#;
        let resp = parse_response(line).unwrap();
        assert!(resp.accepted);
        assert_eq!(resp.id, Some(5));
        assert!(resp.error.is_none());
    }

    #[test]
    fn parse_response_reject() {
        let line = r#"{"accepted":false,"error":"bad input"}"#;
        let resp = parse_response(line).unwrap();
        assert!(!resp.accepted);
        assert_eq!(resp.error, Some("bad input".into()));
    }

    #[test]
    fn validate_accepts_nonempty() {
        let req = JobRequest {
            input: "a.7z".into(),
            output: "out".into(),
            filter: "".into(),
        };
        assert!(validate_request(&req).is_ok());
    }

    #[test]
    fn validate_rejects_empty_input() {
        let req = JobRequest {
            input: "   ".into(),
            output: "out".into(),
            filter: "".into(),
        };
        assert!(validate_request(&req).is_err());
    }

    #[test]
    fn validate_rejects_empty_output() {
        let req = JobRequest {
            input: "a.7z".into(),
            output: "".into(),
            filter: "".into(),
        };
        assert!(validate_request(&req).is_err());
    }

    #[test]
    fn validate_accepts_with_filter() {
        let req = JobRequest {
            input: "a.7z".into(),
            output: "out".into(),
            filter: "*.txt".into(),
        };
        assert!(validate_request(&req).is_ok());
    }

    #[test]
    fn response_serialize_roundtrip() {
        let resp = JobResponse {
            accepted: true,
            id: Some(42),
            error: None,
        };
        let s = serde_json::to_string(&resp).unwrap();
        let parsed = parse_response(&s).unwrap();
        assert!(parsed.accepted);
        assert_eq!(parsed.id, Some(42));
    }

    #[test]
    fn response_reject_roundtrip() {
        let resp = JobResponse {
            accepted: false,
            id: None,
            error: Some("malformed".into()),
        };
        let s = serde_json::to_string(&resp).unwrap();
        let parsed = parse_response(&s).unwrap();
        assert!(!parsed.accepted);
        assert_eq!(parsed.error, Some("malformed".into()));
    }

    #[test]
    fn queue_preserves_arrival_order() {
        let (tx, mut rx) = mpsc::channel::<Job>(16);
        for i in 0..5 {
            let job = Job {
                id: i,
                input: format!("a{i}.7z"),
                output: "out".into(),
                filter: "".into(),
            };
            tx.try_send(job).unwrap();
        }
        for expected in 0..5 {
            let job = rx.try_recv().unwrap();
            assert_eq!(job.id, expected);
        }
    }
}
