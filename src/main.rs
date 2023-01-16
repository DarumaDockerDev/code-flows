use axum::{
    body::Bytes,
    extract::{Path, Query},
    http::{
        header::{HeaderMap, HeaderName, HeaderValue},
        StatusCode,
    },
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use log::info;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{PathBuf, MAIN_SEPARATOR};
use std::process::{Command, ExitStatus};
use std::sync::Arc;
use std::time::SystemTime;
use std::{collections::HashMap, fs};
use wasmedge_sdk::{
    config::{CommonConfigOptions, ConfigBuilder, HostRegistrationConfigOptions},
    params, ImportObjectBuilder, Module, PluginManager, Vm, HOST_FUNCS,
};

mod host_func;

fn download_wasm_file(wasm_path: &str, local_wasm_path: &str) -> std::io::Result<ExitStatus> {
    let s3_bucket = std::env::var("AWS_S3_BUCKET").unwrap();
    let src = format!("s3://{}/{}", s3_bucket, wasm_path);
    Command::new("aws")
        .args(["--quiet", "s3", "cp", &src, local_wasm_path])
        .status()
}

fn get_wasm_path(flow_id: &str) -> String {
    flow_id
        .get(0..3)
        .unwrap()
        .chars()
        .fold(String::new(), |accum, s| {
            format!("{}{}{}", accum, &s, MAIN_SEPARATOR)
        })
}

fn get_wasm_file(flow_id: &str, newly_built: bool) -> Result<String, std::io::Error> {
    let wasm_dir = std::env::var("WASM_DIR").unwrap();
    let wasm_path = format!("{}{}.wasm", get_wasm_path(flow_id), flow_id);
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

fn get_env_file(flow_id: &str) -> String {
    let wasm_dir = std::env::var("WASM_DIR").unwrap();
    let wasm_path = format!("{}{}.env.json", get_wasm_path(flow_id), flow_id);
    let local_env_path = format!("{}{}{}", wasm_dir, MAIN_SEPARATOR, wasm_path);

    local_env_path
}

async fn run_wasm(wp: WasmParams) -> Result<(), Box<dyn std::error::Error>> {
    PluginManager::load_from_default_paths();
    let module = Module::from_file(None, &wp.wasm_file)?;

    let mut func_keys = vec![];

    // create an import module
    let (builder, func_key) = ImportObjectBuilder::new()
        .with_func::<i32, i32>("get_flows_user", host_func::get_flows_user(wp.flows_user))?;
    func_keys.push(func_key);

    let (builder, func_key) =
        builder.with_func::<i32, i32>("get_flow_id", host_func::get_flow_id(wp.flow_id))?;
    func_keys.push(func_key);

    let (builder, func_key) = builder.with_func::<(), i32>(
        "get_event_body_length",
        host_func::get_event_body_length(wp.event_body.len() as i32),
    )?;
    func_keys.push(func_key);

    let (builder, func_key) = builder
        .with_func::<i32, i32>("get_event_body", host_func::get_event_body(wp.event_body))?;
    func_keys.push(func_key);

    let (builder, func_key) = builder.with_func::<(), i32>(
        "get_event_query_length",
        host_func::get_event_query_length(wp.event_query.len() as i32),
    )?;
    func_keys.push(func_key);

    let (builder, func_key) = builder.with_func::<i32, i32>(
        "get_event_query",
        host_func::get_event_query(wp.event_query),
    )?;
    func_keys.push(func_key);

    let (builder, func_key) = builder.with_func::<(i32, i32), ()>(
        "set_flows",
        host_func::set_flows(wp.flows_ptr, wp.flows_len_ptr),
    )?;
    func_keys.push(func_key);

    let (builder, func_key) = builder.with_func::<(i32, i32), ()>(
        "set_error_log",
        host_func::set_error_log(wp.error_log_ptr, wp.error_log_len_ptr),
    )?;
    func_keys.push(func_key);

    let (builder, func_key) = builder.with_func::<(i32, i32), ()>(
        "set_output",
        host_func::set_output(wp.output_ptr, wp.output_len_ptr),
    )?;
    func_keys.push(func_key);

    let (builder, func_key) = builder.with_func::<(i32, i32), ()>(
        "set_response",
        host_func::set_response(wp.response_ptr, wp.response_len_ptr),
    )?;
    func_keys.push(func_key);

    let (builder, func_key) = builder.with_func::<(i32, i32), ()>(
        "set_response_headers",
        host_func::set_response_headers(wp.response_headers_ptr, wp.response_headers_len_ptr),
    )?;
    func_keys.push(func_key);

    let (builder, func_key) = builder.with_func::<i32, ()>(
        "set_response_status",
        host_func::set_response_status(wp.response_status_ptr),
    )?;
    func_keys.push(func_key);

    let (builder, func_key) =
        builder.with_func::<(), i32>("is_listening", host_func::is_listening(wp.listening))?;
    func_keys.push(func_key);

    let import = builder.build("env")?;

    let config = ConfigBuilder::new(CommonConfigOptions::default())
        .with_host_registration_config(HostRegistrationConfigOptions::default().wasi(true))
        .build()?;
    let mut vm = Vm::new(Some(config))?
        .register_import_module(import)?
        .register_module(None, module)?;
    let mut wasi_module = vm.wasi_module()?;
    match fs::read(wp.wasm_env) {
        Ok(env) => {
            let env = Some(serde_json::from_slice(&env).unwrap());
            wasi_module.initialize(None, env, None);
        }
        Err(_) => {
            wasi_module.initialize(None, None, None);
        }
    };

    vm.run_func_async(None::<&str>, wp.wasm_func, params!())
        .await?;

    drop(vm);

    let mut hf = HOST_FUNCS.write();
    for fk in func_keys.iter() {
        hf.remove(fk);
    }

    Ok(())
}

struct PtrParams {
    flows_ptr: usize,
    flows_len_ptr: usize,
    error_log_ptr: usize,
    error_log_len_ptr: usize,
    output_ptr: usize,
    output_len_ptr: usize,
    response_ptr: usize,
    response_len_ptr: usize,
    response_headers_ptr: usize,
    response_headers_len_ptr: usize,
    response_status_ptr: usize,
}

impl Drop for PtrParams {
    fn drop(&mut self) {
        unsafe {
            if self.flows_len_ptr > 0 && *(self.flows_len_ptr as *mut usize) > 0 {
                if self.flows_ptr > 0 && *(self.flows_ptr as *mut usize) > 0 {
                    let len = *(self.flows_len_ptr as *mut usize);
                    Vec::from_raw_parts(*(self.flows_ptr as *mut usize) as *mut u8, len, len);
                }
            }

            if self.error_log_len_ptr > 0 && *(self.error_log_len_ptr as *mut usize) > 0 {
                if self.error_log_ptr > 0 && *(self.error_log_ptr as *mut usize) > 0 {
                    let len = *(self.error_log_len_ptr as *mut usize);
                    Vec::from_raw_parts(*(self.error_log_ptr as *mut usize) as *mut u8, len, len);
                }
            }

            if self.output_len_ptr > 0 && *(self.output_len_ptr as *mut usize) > 0 {
                if self.output_ptr > 0 && *(self.output_ptr as *mut usize) > 0 {
                    let len = *(self.output_len_ptr as *mut usize);
                    Vec::from_raw_parts(*(self.output_ptr as *mut usize) as *mut u8, len, len);
                }
            }

            if self.response_len_ptr > 0 && *(self.response_len_ptr as *mut usize) > 0 {
                if self.response_ptr > 0 && *(self.response_ptr as *mut usize) > 0 {
                    let len = *(self.response_len_ptr as *mut usize);
                    Vec::from_raw_parts(*(self.response_ptr as *mut usize) as *mut u8, len, len);
                }
            }

            if self.response_headers_len_ptr > 0
                && *(self.response_headers_len_ptr as *mut usize) > 0
            {
                if self.response_headers_ptr > 0 && *(self.response_headers_ptr as *mut usize) > 0 {
                    let len = *(self.response_headers_len_ptr as *mut usize);
                    Vec::from_raw_parts(
                        *(self.response_headers_ptr as *mut usize) as *mut u8,
                        len,
                        len,
                    );
                }
            }
        }
    }
}

struct WasmParams {
    listening: i32,
    flows_user: String,
    flow_id: String,
    event_query: String,
    event_body: Arc<Bytes>,
    wasm_file: String,
    wasm_func: String,
    wasm_env: String,

    flows_ptr: usize,
    flows_len_ptr: usize,
    error_log_ptr: usize,
    error_log_len_ptr: usize,
    output_ptr: usize,
    output_len_ptr: usize,
    response_ptr: usize,
    response_len_ptr: usize,
    response_headers_ptr: usize,
    response_headers_len_ptr: usize,
    response_status_ptr: usize,
}

#[derive(Deserialize)]
struct Flow {
    flow_id: String,
    flows_user: String,
}

#[derive(Serialize, Deserialize)]
struct Env {
    name: String,
    value: String,
}

async fn env(Query(v): Query<Value>, Json(env_json): Json<Vec<Env>>) -> impl IntoResponse {
    let flow_id = v["flow_id"].as_str().unwrap();
    let env_file = get_env_file(flow_id);
    let env_json: Vec<String> = env_json
        .iter()
        .map(|e| format!("{}={}", e.name, e.value))
        .collect();
    match fs::write(env_file, serde_json::to_string_pretty(&env_json).unwrap()) {
        Ok(_) => (StatusCode::OK, String::new()),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn register(Query(v): Query<Value>) -> impl IntoResponse {
    let wasm_file =
        match get_wasm_file(v["flow_id"].as_str().unwrap(), v["newly_built"].is_string()) {
            Ok(f) => f,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
    let mut error_log_ptr = vec![0 as usize];
    let mut error_log_len = vec![0 as usize];
    let mut output_ptr = vec![0 as usize];
    let mut output_len = vec![0 as usize];
    let pp = PtrParams {
        flows_ptr: 0,
        flows_len_ptr: 0,
        error_log_ptr: error_log_ptr.as_mut_ptr() as usize,
        error_log_len_ptr: error_log_len.as_mut_ptr() as usize,
        output_ptr: output_ptr.as_mut_ptr() as usize,
        output_len_ptr: output_len.as_mut_ptr() as usize,
        response_ptr: 0,
        response_len_ptr: 0,
        response_headers_ptr: 0,
        response_headers_len_ptr: 0,
        response_status_ptr: 0,
    };
    let wp = WasmParams {
        listening: 1,
        flows_user: v["flows_user"].as_str().unwrap().to_string(),
        flow_id: v["flow_id"].as_str().unwrap().to_string(),
        event_query: String::new(),
        event_body: Arc::new(Bytes::new()),
        wasm_file,
        wasm_func: String::from("run"),
        wasm_env: get_env_file(v["flow_id"].as_str().unwrap()),

        flows_ptr: pp.flows_ptr,
        flows_len_ptr: pp.flows_len_ptr,
        error_log_ptr: pp.error_log_ptr,
        error_log_len_ptr: pp.error_log_len_ptr,
        output_ptr: pp.output_ptr,
        output_len_ptr: pp.output_len_ptr,
        response_ptr: pp.response_ptr,
        response_len_ptr: pp.response_len_ptr,
        response_headers_ptr: pp.response_headers_ptr,
        response_headers_len_ptr: pp.response_headers_len_ptr,
        response_status_ptr: pp.response_status_ptr,
    };
    match run_wasm(wp).await {
        Ok(_) => match error_log_len[0] > 0 && error_log_ptr[0] > 0 {
            true => unsafe {
                let e_len = error_log_len[0] as usize;
                let error_log = Vec::from_raw_parts(error_log_ptr[0] as *mut u8, e_len, e_len);
                // Prevent reconstruct vec from ptr when wp is dropped
                error_log_ptr[0] = 0;
                error_log_len[0] = 0;
                (
                    StatusCode::BAD_REQUEST,
                    String::from_utf8_lossy(&error_log).into_owned(),
                )
            },
            false => {
                let out = match output_len[0] > 0 && output_ptr[0] > 0 {
                    true => unsafe {
                        let o_len = output_len[0] as usize;
                        let output = Vec::from_raw_parts(output_ptr[0] as *mut u8, o_len, o_len);
                        // Prevent reconstruct vec from ptr when wp is droped
                        output_ptr[0] = 0;
                        output_len[0] = 0;
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

async fn hook(
    Path((app, handler)): Path<(String, String)>,
    Query(qry): Query<HashMap<String, Value>>,
    bytes: Bytes,
) -> impl IntoResponse {
    tokio::spawn(async move {
        let bytes = Arc::new(bytes);
        let wasm_dir = std::env::var("WASM_DIR").unwrap();
        let mut flows_ptr = vec![0 as usize];
        let mut flows_len = vec![0 as usize];
        let pp = PtrParams {
            flows_ptr: flows_ptr.as_mut_ptr() as usize,
            flows_len_ptr: flows_len.as_mut_ptr() as usize,
            error_log_ptr: 0,
            error_log_len_ptr: 0,
            output_ptr: 0,
            output_len_ptr: 0,
            response_ptr: 0,
            response_len_ptr: 0,
            response_headers_ptr: 0,
            response_headers_len_ptr: 0,
            response_status_ptr: 0,
        };
        let wp = WasmParams {
            listening: 0,
            flows_user: String::new(),
            flow_id: String::new(),
            event_query: serde_json::to_string(&qry).unwrap(),
            event_body: bytes.clone(),
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
            wasm_env: String::from("[]"),
            flows_ptr: pp.flows_ptr,
            flows_len_ptr: pp.flows_len_ptr,
            error_log_ptr: pp.error_log_ptr,
            error_log_len_ptr: pp.error_log_len_ptr,
            output_ptr: pp.output_ptr,
            output_len_ptr: pp.output_len_ptr,
            response_ptr: pp.response_ptr,
            response_len_ptr: pp.response_len_ptr,
            response_headers_ptr: pp.response_headers_ptr,
            response_headers_len_ptr: pp.response_headers_len_ptr,
            response_status_ptr: pp.response_status_ptr,
        };
        if let Err(_) = run_wasm(wp).await {
            return;
        }

        if flows_len[0] > 0 && flows_ptr[0] > 0 {
            unsafe {
                let f_len = flows_len[0] as usize;
                let flows = Vec::from_raw_parts(flows_ptr[0] as *mut u8, f_len, f_len);
                // Prevent reconstruct vec from ptr when wp is droped
                flows_ptr[0] = 0;
                flows_len[0] = 0;

                if let Ok(flows) = serde_json::from_slice::<Vec<Flow>>(&flows) {
                    for flow in flows.into_iter() {
                        let wasm_file = match get_wasm_file(&flow.flow_id, false) {
                            Ok(f) => f,
                            Err(_) => continue,
                        };
                        let flow_id = flow.flow_id.clone();
                        let mut error_log_ptr = vec![0 as usize];
                        let mut error_log_len = vec![0 as usize];
                        let pp = PtrParams {
                            flows_ptr: 0,
                            flows_len_ptr: 0,
                            error_log_ptr: error_log_ptr.as_mut_ptr() as usize,
                            error_log_len_ptr: error_log_len.as_mut_ptr() as usize,
                            output_ptr: 0,
                            output_len_ptr: 0,
                            response_ptr: 0,
                            response_len_ptr: 0,
                            response_headers_ptr: 0,
                            response_headers_len_ptr: 0,
                            response_status_ptr: 0,
                        };
                        let wp = WasmParams {
                            listening: 0,
                            flows_user: flow.flows_user,
                            wasm_file,
                            wasm_env: get_env_file(&flow_id),
                            flow_id,
                            event_query: serde_json::to_string(&qry).unwrap(),
                            event_body: bytes.clone(),
                            wasm_func: String::from("run"),
                            flows_ptr: pp.flows_ptr,
                            flows_len_ptr: pp.flows_len_ptr,
                            error_log_ptr: pp.error_log_ptr,
                            error_log_len_ptr: pp.error_log_len_ptr,
                            output_ptr: pp.output_ptr,
                            output_len_ptr: pp.output_len_ptr,
                            response_ptr: pp.response_ptr,
                            response_len_ptr: pp.response_len_ptr,
                            response_headers_ptr: pp.response_headers_ptr,
                            response_headers_len_ptr: pp.response_headers_len_ptr,
                            response_status_ptr: pp.response_status_ptr,
                        };
                        info!(
                            r#""msg": {:?}, "flow": {:?}, "function": {:?}"#,
                            "Running function", flow.flow_id, "run"
                        );
                        _ = run_wasm(wp).await;
                        if error_log_len[0] > 0 && error_log_ptr[0] > 0 {
                            let e_len = error_log_len[0] as usize;
                            let error_log =
                                Vec::from_raw_parts(error_log_ptr[0] as *mut u8, e_len, e_len);
                            // Prevent reconstruct vec from ptr when wp is droped
                            error_log_ptr[0] = 0;
                            error_log_len[0] = 0;
                            info!(
                                r#""msg": {:?}, "flow": {:?}, "function": {:?}, "error": {:?}"#,
                                "function returned with error",
                                flow.flow_id,
                                "run",
                                String::from_utf8_lossy(&error_log).into_owned()
                            );
                        }
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
    let mut flows_ptr = vec![0 as usize];
    let mut flows_len = vec![0 as usize];
    qry.insert(String::from("l_key"), Value::String(l_key));
    let pp = PtrParams {
        flows_ptr: flows_ptr.as_mut_ptr() as usize,
        flows_len_ptr: flows_len.as_mut_ptr() as usize,
        error_log_ptr: 0,
        error_log_len_ptr: 0,
        output_ptr: 0,
        output_len_ptr: 0,
        response_ptr: 0,
        response_len_ptr: 0,
        response_headers_ptr: 0,
        response_headers_len_ptr: 0,
        response_status_ptr: 0,
    };
    let wp = WasmParams {
        listening: 0,
        flows_user: String::new(),
        flow_id: String::new(),
        event_query: serde_json::to_string(&qry).unwrap(),
        event_body: bytes.clone(),
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
        wasm_env: String::from("[]"),
        flows_ptr: pp.flows_ptr,
        flows_len_ptr: pp.flows_len_ptr,
        error_log_ptr: pp.error_log_ptr,
        error_log_len_ptr: pp.error_log_len_ptr,
        output_ptr: pp.output_ptr,
        output_len_ptr: pp.output_len_ptr,
        response_ptr: pp.response_ptr,
        response_len_ptr: pp.response_len_ptr,
        response_headers_ptr: pp.response_headers_ptr,
        response_headers_len_ptr: pp.response_headers_len_ptr,
        response_status_ptr: pp.response_status_ptr,
    };
    if let Err(e) = run_wasm(wp).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    if flows_len[0] > 0 && flows_ptr[0] > 0 {
        unsafe {
            let f_len = flows_len[0] as usize;
            let flows = Vec::from_raw_parts(flows_ptr[0] as *mut u8, f_len, f_len);
            // Prevent reconstruct vec from ptr when wp is droped
            flows_ptr[0] = 0;
            flows_len[0] = 0;

            if let Ok(flow) = serde_json::from_slice::<Flow>(&flows) {
                let wasm_file = match get_wasm_file(&flow.flow_id, false) {
                    Ok(f) => f,
                    Err(e) => {
                        return Err((StatusCode::NOT_FOUND, e.to_string()));
                    }
                };
                let flow_id = flow.flow_id.clone();
                let mut error_log_ptr = vec![0 as usize];
                let mut error_log_len = vec![0 as usize];
                let mut response_ptr = vec![0 as usize];
                let mut response_len = vec![0 as usize];
                let mut response_headers_ptr = vec![0 as usize];
                let mut response_headers_len = vec![0 as usize];
                let mut response_status = vec![0 as usize];
                let pp = PtrParams {
                    flows_ptr: 0,
                    flows_len_ptr: 0,
                    error_log_ptr: error_log_ptr.as_mut_ptr() as usize,
                    error_log_len_ptr: error_log_len.as_mut_ptr() as usize,
                    output_ptr: 0,
                    output_len_ptr: 0,
                    response_ptr: response_ptr.as_mut_ptr() as usize,
                    response_len_ptr: response_len.as_mut_ptr() as usize,
                    response_headers_ptr: response_headers_ptr.as_mut_ptr() as usize,
                    response_headers_len_ptr: response_headers_len.as_mut_ptr() as usize,
                    response_status_ptr: response_status.as_mut_ptr() as usize,
                };
                let wp = WasmParams {
                    listening: 0,
                    flows_user: flow.flows_user,
                    wasm_file,
                    wasm_env: get_env_file(&flow_id),
                    flow_id,
                    event_query: serde_json::to_string(&qry).unwrap(),
                    event_body: bytes.clone(),
                    wasm_func: String::from("run"),
                    flows_ptr: pp.flows_ptr,
                    flows_len_ptr: pp.flows_len_ptr,
                    error_log_ptr: pp.error_log_ptr,
                    error_log_len_ptr: pp.error_log_len_ptr,
                    output_ptr: pp.output_ptr,
                    output_len_ptr: pp.output_len_ptr,
                    response_ptr: pp.response_ptr,
                    response_len_ptr: pp.response_len_ptr,
                    response_headers_ptr: pp.response_headers_ptr,
                    response_headers_len_ptr: pp.response_headers_len_ptr,
                    response_status_ptr: pp.response_status_ptr,
                };
                info!(
                    r#""msg": {:?}, "flow": {:?}, "function": {:?}"#,
                    "Running function", flow.flow_id, "run"
                );
                _ = run_wasm(wp).await;
                if error_log_len[0] > 0 && error_log_ptr[0] > 0 {
                    let e_len = error_log_len[0] as usize;
                    let error_log = Vec::from_raw_parts(error_log_ptr[0] as *mut u8, e_len, e_len);
                    // Prevent reconstruct vec from ptr when wp is droped
                    error_log_ptr[0] = 0;
                    error_log_len[0] = 0;
                    info!(
                        r#""msg": {:?}, "flow": {:?}, "function": {:?}, "error": {:?}"#,
                        "function returned with error",
                        flow.flow_id,
                        "run",
                        String::from_utf8_lossy(&error_log).into_owned()
                    );
                }

                let mut res_status = 200 as u16;
                if response_status[0] > 0 {
                    res_status = response_status[0] as u16;
                }

                let mut response = vec![];
                if response_ptr[0] > 0 && response_len[0] > 0 {
                    let r_len = response_len[0] as usize;
                    response = Vec::from_raw_parts(response_ptr[0] as *mut u8, r_len, r_len);
                    // Prevent reconstruct vec from ptr when wp is droped
                    response_ptr[0] = 0;
                    response_len[0] = 0;
                }

                let mut res_headers = vec![];
                if response_headers_ptr[0] > 0 && response_headers_len[0] > 0 {
                    let r_len = response_headers_len[0] as usize;
                    let response_headers =
                        Vec::from_raw_parts(response_headers_ptr[0] as *mut u8, r_len, r_len);
                    // Prevent reconstruct vec from ptr when wp is droped
                    response_headers_ptr[0] = 0;
                    response_headers_len[0] = 0;

                    if let Ok(response_headers) =
                        serde_json::from_slice::<Vec<(String, String)>>(&response_headers)
                    {
                        res_headers = response_headers;
                    }
                }

                let mut h = HeaderMap::new();
                for rh in res_headers.into_iter() {
                    if let Ok(hn) = HeaderName::from_bytes(rh.0.as_bytes()) {
                        if let Ok(hv) = HeaderValue::from_str(&rh.1) {
                            h.insert(hn, hv);
                        }
                    }
                }

                return Ok((
                    StatusCode::from_u16(res_status).unwrap_or(StatusCode::OK),
                    h,
                    response,
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
        .route("/inner/register", post(register))
        .route("/inner/env", post(env))
        .route("/hook/:app/:handler", post(hook))
        .route("/lambda/:l_key", post(lambda).get(lambda));

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8094".to_string())
        .parse::<u16>()?;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
