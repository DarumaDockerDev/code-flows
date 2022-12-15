use axum::{
    body::Bytes,
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Router,
};
use serde::Deserialize;
use serde_json::Value;
use std::fs::File;
use std::net::SocketAddr;
use std::path::{PathBuf, MAIN_SEPARATOR};
use std::process::{Command, ExitStatus};
use std::sync::Arc;
use wasmedge_sdk::{
    config::{CommonConfigOptions, ConfigBuilder, HostRegistrationConfigOptions},
    params, ImportObjectBuilder, Module, PluginManager, Vm,
};

mod host_func;

fn download_wasm_file(wasm_path: &str, local_wasm_path: &str) -> std::io::Result<ExitStatus> {
    let s3_bucket = std::env::var("AWS_S3_BUCKET").unwrap();
    let src = format!("s3://{}/{}", s3_bucket, wasm_path);
    Command::new("aws")
        .args(["--quiet", "s3", "cp", &src, local_wasm_path])
        .status()
}

fn get_wasm_file(flow_id: &str, newly_built: bool) -> Result<String, std::io::Error> {
    let wasm_dir = std::env::var("WASM_DIR").unwrap();
    let wasm_path = flow_id
        .get(0..3)
        .unwrap()
        .chars()
        .fold(String::new(), |accum, s| {
            format!("{}{}{}", accum, &s, MAIN_SEPARATOR)
        });
    let wasm_path = format!("{}{}.wasm", wasm_path, flow_id);
    let local_wasm_path = format!("{}{}{}", wasm_dir, MAIN_SEPARATOR, wasm_path);

    match newly_built {
        true => {
            download_wasm_file(&wasm_path, &local_wasm_path)?;
        }
        false => {
            if let Err(_) = File::open(&local_wasm_path) {
                download_wasm_file(&wasm_path, &local_wasm_path)?;
            }
        }
    }

    Ok(local_wasm_path)
}

async fn run_wasm(wp: WasmParams) -> Result<(), Box<dyn std::error::Error>> {
    PluginManager::load_from_default_paths();
    let module = Module::from_file(None, &wp.wasm_file)?;

    // create an import module
    let import = ImportObjectBuilder::new()
        .with_func::<i32, i32>("get_flows_user", host_func::get_flows_user(wp.flows_user))?
        .with_func::<i32, i32>("get_flow_id", host_func::get_flow_id(wp.flow_id))?
        .with_func::<(), i32>(
            "get_event_body_length",
            host_func::get_event_body_length(wp.event_body.len() as i32),
        )?
        .with_func::<i32, i32>("get_event_body", host_func::get_event_body(wp.event_body))?
        .with_func::<(i32, i32), ()>(
            "set_flows",
            host_func::set_flows(wp.flows_ptr, wp.flows_len_ptr),
        )?
        .build("env")?;

    let config = ConfigBuilder::new(CommonConfigOptions::default())
        .with_host_registration_config(HostRegistrationConfigOptions::default().wasi(true))
        .build()?;
    let mut vm = Vm::new(Some(config))?
        .register_import_module(import)?
        .register_module(None, module)?;
    let mut wasi_module = vm.wasi_module()?;
    wasi_module.initialize(None, None, None);

    vm.run_func_async(None::<&str>, wp.wasm_func, params!())
        .await?;

    Ok(())
}

struct WasmParams {
    flows_user: String,
    flow_id: String,
    event_body: Arc<Bytes>,
    flows_ptr: usize,
    flows_len_ptr: usize,
    wasm_file: String,
    wasm_func: String,
}

#[derive(Deserialize)]
struct Flow {
    flow_id: String,
    flows_user: String,
}

async fn prepare(Query(v): Query<Value>) -> impl IntoResponse {
    let wasm_file = match get_wasm_file(v["flow_id"].as_str().unwrap(), true) {
        Ok(f) => f,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let wp = WasmParams {
        flows_user: v["flows_user"].as_str().unwrap().to_string(),
        flow_id: v["flow_id"].as_str().unwrap().to_string(),
        event_body: Arc::new(Bytes::new()),
        flows_ptr: 0,
        flows_len_ptr: 0,
        wasm_file,
        wasm_func: String::from("prepare"),
    };
    match run_wasm(wp).await {
        Ok(_) => (StatusCode::OK, String::new()),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn _challenge(bytes: Bytes) -> impl IntoResponse {
    let v: Value = serde_json::from_slice(bytes.as_ref()).unwrap();
    (StatusCode::OK, v["challenge"].as_str().unwrap().to_string())
}

async fn hook(Path((app, handler)): Path<(String, String)>, bytes: Bytes) -> impl IntoResponse {
    tokio::spawn(async {
        let bytes = Arc::new(bytes);
        let wasm_dir = std::env::var("WASM_DIR").unwrap();
        let mut flows = Vec::<u8>::with_capacity(1000);
        let mut flows_len = vec![0 as u8];
        let wp = WasmParams {
            flows_user: String::new(),
            flow_id: String::new(),
            event_body: bytes.clone(),
            flows_ptr: flows.as_mut_ptr() as usize,
            flows_len_ptr: flows_len.as_mut_ptr() as usize,
            wasm_file: [
                wasm_dir,
                String::from("apps"),
                app,
                String::from("latest.wasm"),
            ]
            .iter()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned(),
            wasm_func: String::from(handler),
        };
        if let Err(_) = run_wasm(wp).await {
            return;
        }

        if flows_len[0] > 0 {
            unsafe {
                flows.set_len(flows_len[0] as usize);
            }

            if let Ok(flows) = serde_json::from_slice::<Vec<Flow>>(&flows) {
                for flow in flows.into_iter() {
                    let wasm_file = match get_wasm_file(&flow.flow_id, false) {
                        Ok(f) => f,
                        Err(_) => continue,
                    };
                    let wp = WasmParams {
                        flows_user: flow.flows_user,
                        wasm_file,
                        flow_id: flow.flow_id,
                        event_body: bytes.clone(),
                        flows_ptr: 0,
                        flows_len_ptr: 0,
                        wasm_func: String::from("work"),
                    };
                    _ = run_wasm(wp).await;
                }
            }
        }
    });
    (StatusCode::OK, String::new())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/prepare", post(prepare))
        .route("/hook/:app/:handler", post(hook));

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8094".to_string())
        .parse::<u16>()?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
