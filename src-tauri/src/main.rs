// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use std::process::Stdio;

use anyhow::anyhow;
use interprocess::local_socket::{prelude::*, tokio::prelude::*, GenericNamespaced, ListenerOptions, Name, Stream};
use regex::Regex;
#[cfg(not(debug_assertions))]
use tauri::api::dialog::blocking::message;
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use codepage::to_encoding;
use encoding_rs::Encoding;
use windows::Win32::Globalization::GetOEMCP;

fn get_socket_name() -> Name<'static> {
  let id = "dev.yakex.q7z_ipc";
  let name = id.to_ns_name::<GenericNamespaced>().unwrap();
  name
}

fn matches_to_message(matches: tauri::api::cli::Matches) -> String {
  // TODO: argument validity check
  let input = &matches.args.get("input").unwrap().value;
  let output = &matches.args.get("output").unwrap().value;
  let filter = &matches.args.get("filter").unwrap().value;
  let message = format!("{}\0{}\0{}\n", input, output, filter);
  message
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
  .setup(|app| {
    let matches = app.get_cli_matches();
    let name = get_socket_name();
    match Stream::connect(name) {
      Ok(mut conn) => { // server exists
        match matches {
          Ok(matches) => {
            let ipc_message = matches_to_message(matches);
            conn.write_all(ipc_message.as_bytes()).unwrap();
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
          Err(_) => {
            Err(anyhow!("no argments specified but there is another process, nothing to do").into())
          }
        }
      }
      Err(_) => { // server does not exist, run
        app.get_window("main").unwrap().show().unwrap(); // show main window
        let app_handle = app.handle();
        tauri::async_runtime::spawn(async move {
          listen_for_ipc(app_handle).await;
        });
        match matches {
          Ok(matches) => {
            println!("{:?}", matches)
            // TODO: run 7za by specified arguments
          }
          Err(_) => {}
        }
        Ok(())
      }
    }  
  })
  .run(tauri::generate_context!())
  .expect("error while running tauri application");  
}

async fn listen_for_ipc(app_handle: tauri::AppHandle) {
  let name = get_socket_name();
  let opts = ListenerOptions::new().name(name);
  let listener = opts.create_tokio().unwrap();
  loop {
    let stream = listener.accept().await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut buffer = String::with_capacity(512);
    let _ = reader.read_line(&mut buffer).await.unwrap();
    let parts: Vec<&str> = buffer.strip_suffix('\n').unwrap().split('\0').collect();
    if parts.len() == 3 {
      let (input, output, filter) = (parts[0].to_string(), parts[1].to_string(), parts[2].to_string());
      println!("Recieved: input:{} output:{} filter:{}", input, output, filter);
      run_7z(&app_handle, input, output, filter).await;
    }
  }
}

async fn run_7z(app_handle: &tauri::AppHandle, input: String, output: String, filter: String) {
  let mut cmd = Command::new("7z.exe")
  .raw_arg("x")
  .raw_arg(&input)
  .raw_arg(format!("-o{}", output))
  .raw_arg("-aou") // auto rename extracting file
  .raw_arg("-bsp1") // set progress information to stdout
  .raw_arg(&filter)
  .stdout(Stdio::piped())
  .spawn()
  .expect("Failed to start 7z");

  if let Some(ref mut stdout) = cmd.stdout {
    let converter = get_encoding();
    let mut reader = tokio::io::BufReader::new(stdout);
    let re = Regex::new(r"^\s*(\d+)%").unwrap();
    let mut buf: Vec<u8> = vec![];
    while let Ok(num_bytes) = reader.read_until(b'\r', &mut buf).await {
      if num_bytes == 0 {
        break
      }
      let (raw_line, _, _) = converter.decode(&buf);
      let linefeed = raw_line.chars().nth(0) == Some('\n');
      let line = raw_line.trim_start_matches('\n').trim_end_matches('\r');
      if let Some(caps) = re.captures(line) {
        let percent = caps.get(1).unwrap().as_str();
        app_handle.emit_all("percent", percent).unwrap()
      }
      // TODO: "Everything is Ok" should lead 100%
      // TODO: Append log part
      // TODO: Show processing file
      println!("line:{} [{}] {}", line.len(), line, linefeed);
      buf = vec![];
    }
  }
}
