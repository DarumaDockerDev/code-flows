use axum::{
    body::Bytes,
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Router,
};
use log::info;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{PathBuf, MAIN_SEPARATOR};
use std::process::{Command, ExitStatus};
use std::sync::Arc;
use std::time::SystemTime;
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
        .with_func::<(), i32>(
            "get_event_query_length",
            host_func::get_event_query_length(wp.event_query.len() as i32),
        )?
        .with_func::<i32, i32>(
            "get_event_query",
            host_func::get_event_query(wp.event_query),
        )?
        .with_func::<(i32, i32), ()>(
            "set_flows",
            host_func::set_flows(wp.flows_ptr, wp.flows_len_ptr),
        )?
        .with_func::<(i32, i32), ()>(
            "set_error_log",
            host_func::set_error_log(wp.error_log_ptr, wp.error_log_len_ptr),
        )?
        .with_func::<(i32, i32), ()>(
            "set_output",
            host_func::set_output(wp.output_ptr, wp.output_len_ptr),
        )?
        .with_func::<(i32, i32), ()>(
            "set_response",
            host_func::set_response(wp.response_ptr, wp.response_len_ptr),
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
    event_query: String,
    event_body: Arc<Bytes>,
    flows_ptr: usize,
    flows_len_ptr: usize,
    error_log_ptr: usize,
    error_log_len_ptr: usize,
    output_ptr: usize,
    output_len_ptr: usize,
    response_ptr: usize,
    response_len_ptr: usize,
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
    let mut error_log = Vec::<u8>::with_capacity(1000);
    let mut error_log_len = vec![0 as u8];
    let mut output = Vec::<u8>::with_capacity(1000);
    let mut output_len = vec![0 as u8];
    let wp = WasmParams {
        flows_user: v["flows_user"].as_str().unwrap().to_string(),
        flow_id: v["flow_id"].as_str().unwrap().to_string(),
        event_query: String::new(),
        event_body: Arc::new(Bytes::new()),
        flows_ptr: 0,
        flows_len_ptr: 0,
        error_log_ptr: error_log.as_mut_ptr() as usize,
        error_log_len_ptr: error_log_len.as_mut_ptr() as usize,
        output_ptr: output.as_mut_ptr() as usize,
        output_len_ptr: output_len.as_mut_ptr() as usize,
        response_ptr: 0,
        response_len_ptr: 0,
        wasm_file,
        wasm_func: String::from("prepare"),
    };
    match run_wasm(wp).await {
        Ok(_) => match error_log_len[0] > 0 {
            true => unsafe {
                error_log.set_len(error_log_len[0] as usize);
                (
                    StatusCode::BAD_REQUEST,
                    String::from_utf8_lossy(&error_log).into_owned(),
                )
            },
            false => {
                let out = match output_len[0] > 0 {
                    true => unsafe {
                        output.set_len(output_len[0] as usize);
                        String::from_utf8_lossy(&output).into_owned()
                    },
                    false => String::new(),
                };
                (StatusCode::OK, out)
            }
        },
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
        let mut error_log = Vec::<u8>::with_capacity(1000);
        let mut error_log_len = vec![0 as u8];
        let wp = WasmParams {
            flows_user: String::new(),
            flow_id: String::new(),
            event_query: String::new(),
            event_body: bytes.clone(),
            flows_ptr: flows.as_mut_ptr() as usize,
            flows_len_ptr: flows_len.as_mut_ptr() as usize,
            error_log_ptr: error_log.as_mut_ptr() as usize,
            error_log_len_ptr: error_log_len.as_mut_ptr() as usize,
            output_ptr: 0,
            output_len_ptr: 0,
            response_ptr: 0,
            response_len_ptr: 0,
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
                    let flow_id = flow.flow_id.clone();
                    let mut error_log = Vec::<u8>::with_capacity(1000);
                    let mut error_log_len = vec![0 as u8];
                    let wp = WasmParams {
                        flows_user: flow.flows_user,
                        wasm_file,
                        flow_id,
                        event_query: String::new(),
                        event_body: bytes.clone(),
                        flows_ptr: 0,
                        flows_len_ptr: 0,
                        error_log_ptr: error_log.as_mut_ptr() as usize,
                        error_log_len_ptr: error_log_len.as_mut_ptr() as usize,
                        output_ptr: 0,
                        output_len_ptr: 0,
                        response_ptr: 0,
                        response_len_ptr: 0,
                        wasm_func: String::from("work"),
                    };
                    info!(
                        r#""msg": {:?}, "flow": {:?}, "function": {:?}"#,
                        "Running function", flow.flow_id, "work"
                    );
                    _ = run_wasm(wp).await;
                    if error_log_len[0] > 0 {
                        unsafe {
                            error_log.set_len(error_log_len[0] as usize);
                        }
                        info!(
                            r#""msg": {:?}, "flow": {:?}, "function": {:?}, "error": {:?}"#,
                            "function returned with error",
                            flow.flow_id,
                            "work",
                            String::from_utf8_lossy(&error_log).into_owned()
                        );
                    }
                }
            }
        }
    });
    (StatusCode::OK, String::new())
}

async fn lambda(
    Path(l_key): Path<String>,
    Query(mut qry): Query<HashMap<String, Value>>,
    bytes: Bytes,
) -> impl IntoResponse {
    let bytes = Arc::new(bytes);
    let wasm_dir = std::env::var("WASM_DIR").unwrap();
    let mut flows = Vec::<u8>::with_capacity(1000);
    let mut flows_len = vec![0 as u8];
    let mut error_log = Vec::<u8>::with_capacity(1000);
    let mut error_log_len = vec![0 as u8];
    qry.insert(String::from("l_key"), Value::String(l_key));
    let wp = WasmParams {
        flows_user: String::new(),
        flow_id: String::new(),
        event_query: serde_json::to_string(&qry).unwrap(),
        event_body: bytes.clone(),
        flows_ptr: flows.as_mut_ptr() as usize,
        flows_len_ptr: flows_len.as_mut_ptr() as usize,
        error_log_ptr: error_log.as_mut_ptr() as usize,
        error_log_len_ptr: error_log_len.as_mut_ptr() as usize,
        output_ptr: 0,
        output_len_ptr: 0,
        response_ptr: 0,
        response_len_ptr: 0,
        wasm_file: [
            wasm_dir,
            String::from("apps"),
            String::from("lambda"),
            String::from("latest.wasm"),
        ]
        .iter()
        .collect::<PathBuf>()
        .to_string_lossy()
        .into_owned(),
        wasm_func: String::from("request"),
    };
    if let Err(e) = run_wasm(wp).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    if flows_len[0] > 0 {
        unsafe {
            flows.set_len(flows_len[0] as usize);
        }

        if let Ok(flow) = serde_json::from_slice::<Flow>(&flows) {
            let wasm_file = match get_wasm_file(&flow.flow_id, false) {
                Ok(f) => f,
                Err(e) => {
                    return Err((StatusCode::NOT_FOUND, e.to_string()));
                }
            };
            let flow_id = flow.flow_id.clone();
            let mut error_log = Vec::<u8>::with_capacity(1000);
            let mut error_log_len = vec![0 as u8];
            let mut response = Vec::<u8>::with_capacity(1000);
            let mut response_len = vec![0 as u8];
            let wp = WasmParams {
                flows_user: flow.flows_user,
                wasm_file,
                flow_id,
                event_query: serde_json::to_string(&qry).unwrap(),
                event_body: bytes.clone(),
                flows_ptr: 0,
                flows_len_ptr: 0,
                error_log_ptr: error_log.as_mut_ptr() as usize,
                error_log_len_ptr: error_log_len.as_mut_ptr() as usize,
                output_ptr: 0,
                output_len_ptr: 0,
                response_ptr: response.as_mut_ptr() as usize,
                response_len_ptr: response_len.as_mut_ptr() as usize,
                wasm_func: String::from("work"),
            };
            info!(
                r#""msg": {:?}, "flow": {:?}, "function": {:?}"#,
                "Running function", flow.flow_id, "work"
            );
            _ = run_wasm(wp).await;
            if error_log_len[0] > 0 {
                unsafe {
                    error_log.set_len(error_log_len[0] as usize);
                }
                info!(
                    r#""msg": {:?}, "flow": {:?}, "function": {:?}, "error": {:?}"#,
                    "function returned with error",
                    flow.flow_id,
                    "work",
                    String::from_utf8_lossy(&error_log).into_owned()
                );
            }

            if response_len[0] > 0 {
                unsafe {
                    response.set_len(response_len[0] as usize);
                }
                return Ok((
                    StatusCode::OK,
                    String::from_utf8_lossy(&response).into_owned(),
                ));
            }
        }
    }
    Err((StatusCode::NOT_FOUND, String::new()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::builder()
        .format(|buf, record| {
            writeln!(
                buf,
                r#"{{"level": {:?}, "ts": {:?}, {}}}"#,
                record.level().as_str(),
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs_f64(),
                record.args(),
            )
        })
        .target(env_logger::Target::Pipe(Box::new(
            File::options()
                .create(true)
                .append(true)
                .open(std::env::var("LOG_FILE").unwrap())
                .unwrap(),
        )))
        .init();
    let app = Router::new()
        .route("/prepare", post(prepare))
        .route("/hook/:app/:handler", post(hook))
        .route("/lambda/:l_key", post(lambda).get(lambda));

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8094".to_string())
        .parse::<u16>()?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
