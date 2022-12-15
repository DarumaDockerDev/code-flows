use axum::body::Bytes;
use reqwest::ClientBuilder;
use std::sync::Arc;
use std::time::Duration;
use wasmedge_sdk::{
    async_host_function, error::HostFuncError, Caller, CallingFrame, ValType, WasmValue,
};

pub fn get_flows_user(
    flows_user: String,
) -> impl Fn(CallingFrame, Vec<WasmValue>) -> Result<Vec<WasmValue>, HostFuncError> + Send + Sync + 'static
{
    move |frame: CallingFrame, args: Vec<WasmValue>| {
        let caller = Caller::new(frame);
        if args.len() != 1 {
            return Err(HostFuncError::User(1));
        }

        let ptr = if args[0].ty() == ValType::I32 {
            args[0].to_i32()
        } else {
            return Err(HostFuncError::User(2));
        };

        _ = caller
            .memory(0)
            .unwrap()
            .write(flows_user.clone(), ptr as u32);
        Ok(vec![WasmValue::from_i32(flows_user.len() as i32)])
    }
}

pub fn get_flow_id(
    flow_id: String,
) -> impl Fn(CallingFrame, Vec<WasmValue>) -> Result<Vec<WasmValue>, HostFuncError> + Send + Sync + 'static
{
    move |frame: wasmedge_sdk::CallingFrame, args: Vec<wasmedge_sdk::WasmValue>| {
        let caller = Caller::new(frame);
        if args.len() != 1 {
            return Err(HostFuncError::User(1));
        }

        let ptr = if args[0].ty() == ValType::I32 {
            args[0].to_i32()
        } else {
            return Err(HostFuncError::User(2));
        };

        _ = caller.memory(0).unwrap().write(flow_id.clone(), ptr as u32);
        Ok(vec![WasmValue::from_i32(flow_id.len() as i32)])
    }
}

pub fn get_event_body_length(
    l: i32,
) -> impl Fn(CallingFrame, Vec<WasmValue>) -> Result<Vec<WasmValue>, HostFuncError> + Send + Sync + 'static
{
    move |_frame: wasmedge_sdk::CallingFrame, _args: Vec<wasmedge_sdk::WasmValue>| {
        Ok(vec![WasmValue::from_i32(l)])
    }
}

pub fn get_event_body(
    bytes: Arc<Bytes>,
) -> impl Fn(CallingFrame, Vec<WasmValue>) -> Result<Vec<WasmValue>, HostFuncError> + Send + Sync + 'static
{
    move |frame: wasmedge_sdk::CallingFrame, args: Vec<wasmedge_sdk::WasmValue>| {
        let caller = Caller::new(frame);
        if args.len() != 1 {
            return Err(HostFuncError::User(1));
        }

        let ptr = if args[0].ty() == ValType::I32 {
            args[0].to_i32()
        } else {
            return Err(HostFuncError::User(2));
        };

        _ = caller.memory(0).unwrap().write(bytes.as_ref(), ptr as u32);
        Ok(vec![WasmValue::from_i32(bytes.len() as i32)])
    }
}

pub fn set_flows(
    flows_ptr: usize,
    flows_len_ptr: usize,
) -> impl Fn(CallingFrame, Vec<WasmValue>) -> Result<Vec<WasmValue>, HostFuncError> + Send + Sync + 'static
{
    move |frame: wasmedge_sdk::CallingFrame, args: Vec<wasmedge_sdk::WasmValue>| {
        let caller = Caller::new(frame);
        if args.len() != 2 {
            return Err(HostFuncError::User(1));
        }

        let ptr = if args[0].ty() == ValType::I32 {
            args[0].to_i32()
        } else {
            return Err(HostFuncError::User(2));
        };

        let len = if args[1].ty() == ValType::I32 {
            args[1].to_i32()
        } else {
            return Err(HostFuncError::User(2));
        };

        let flows = caller
            .memory(0)
            .unwrap()
            .read_string(ptr as u32, len as u32)
            .unwrap();

        unsafe {
            let bytes = flows.as_bytes();
            std::ptr::write(flows_len_ptr as *mut u8, bytes.len() as u8);
            for n in 0..bytes.len() {
                std::ptr::write((flows_ptr + n) as *mut u8, bytes[n]);
            }
        }

        Ok(vec![])
    }
}

#[async_host_function]
pub async fn redirect_to(
    caller: Caller,
    args: Vec<WasmValue>,
) -> Result<Vec<WasmValue>, HostFuncError> {
    if args.len() != 2 {
        return Err(HostFuncError::User(1));
    }

    let ptr = if args[0].ty() == ValType::I32 {
        args[0].to_i32()
    } else {
        return Err(HostFuncError::User(2));
    };

    let len = if args[1].ty() == ValType::I32 {
        args[1].to_i32()
    } else {
        return Err(HostFuncError::User(2));
    };

    let url = caller
        .memory(0)
        .unwrap()
        .read_string(ptr as u32, len as u32)
        .unwrap();
    println!("{}", url);

    let client = ClientBuilder::new()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Can't build the reqwest client");

    match client.get(url).send().await {
        Ok(r) => {
            let status = r.status();
            println!("{:?}", status);
        }
        Err(e) => {
            println!("{:?}", e);
        }
    }

    Ok(vec![])
}
