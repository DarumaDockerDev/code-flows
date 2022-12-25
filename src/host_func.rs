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

pub fn get_event_query_length(
    l: i32,
) -> impl Fn(CallingFrame, Vec<WasmValue>) -> Result<Vec<WasmValue>, HostFuncError> + Send + Sync + 'static
{
    move |_frame: wasmedge_sdk::CallingFrame, _args: Vec<wasmedge_sdk::WasmValue>| {
        Ok(vec![WasmValue::from_i32(l)])
    }
}

pub fn get_event_query(
    event_query: String,
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

        _ = caller
            .memory(0)
            .unwrap()
            .write(event_query.clone(), ptr as u32);
        Ok(vec![WasmValue::from_i32(event_query.len() as i32)])
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
            std::ptr::write(flows_len_ptr as *mut usize, bytes.len());

            let v: Vec<u8> = Vec::with_capacity(bytes.len());
            let mut v = std::mem::ManuallyDrop::new(v);

            let p = v.as_mut_ptr() as usize;
            for n in 0..bytes.len() {
                std::ptr::write((p + n) as *mut u8, bytes[n]);
            }

            std::ptr::write(flows_ptr as *mut usize, p);
        }

        Ok(vec![])
    }
}

pub fn set_output(
    output_ptr: usize,
    output_len_ptr: usize,
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

        let output = caller
            .memory(0)
            .unwrap()
            .read_string(ptr as u32, len as u32)
            .unwrap();

        unsafe {
            let bytes = output.as_bytes();
            std::ptr::write(output_len_ptr as *mut usize, bytes.len());

            let v: Vec<u8> = Vec::with_capacity(bytes.len());
            let mut v = std::mem::ManuallyDrop::new(v);

            let p = v.as_mut_ptr() as usize;
            for n in 0..bytes.len() {
                std::ptr::write((p + n) as *mut u8, bytes[n]);
            }

            std::ptr::write(output_ptr as *mut usize, p);
        }

        Ok(vec![])
    }
}

pub fn set_error_log(
    error_log_ptr: usize,
    error_log_len_ptr: usize,
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

        let error_log = caller
            .memory(0)
            .unwrap()
            .read_string(ptr as u32, len as u32)
            .unwrap();

        unsafe {
            let bytes = error_log.as_bytes();
            std::ptr::write(error_log_len_ptr as *mut usize, bytes.len());

            let v: Vec<u8> = Vec::with_capacity(bytes.len());
            let mut v = std::mem::ManuallyDrop::new(v);

            let p = v.as_mut_ptr() as usize;
            for n in 0..bytes.len() {
                std::ptr::write((p + n) as *mut u8, bytes[n]);
            }

            std::ptr::write(error_log_ptr as *mut usize, p);
        }

        Ok(vec![])
    }
}

pub fn set_response(
    response_ptr: usize,
    response_len_ptr: usize,
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

        let response = caller
            .memory(0)
            .unwrap()
            .read_string(ptr as u32, len as u32)
            .unwrap();

        unsafe {
            let bytes = response.as_bytes();
            std::ptr::write(response_len_ptr as *mut usize, bytes.len());

            let v: Vec<u8> = Vec::with_capacity(bytes.len());
            let mut v = std::mem::ManuallyDrop::new(v);

            let p = v.as_mut_ptr() as usize;
            for n in 0..bytes.len() {
                std::ptr::write((p + n) as *mut u8, bytes[n]);
            }

            std::ptr::write(response_ptr as *mut usize, p);
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
