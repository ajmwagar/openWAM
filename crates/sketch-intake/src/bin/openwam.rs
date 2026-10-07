use reversal_sketch_intake::openwam_api::{self as api, ApiError, ErrorCode, Response};
use std::{
    io::{Read, Write},
    process::ExitCode,
};

fn request(path: Option<&std::ffi::OsStr>) -> Response {
    let reader: Result<Box<dyn Read>, std::io::Error> = match path {
        None => Ok(Box::new(std::io::stdin())),
        Some(path) if path == "-" => Ok(Box::new(std::io::stdin())),
        Some(path) => std::fs::File::open(path).map(|file| Box::new(file) as Box<dyn Read>),
    };
    let mut bytes = Vec::new();
    let read = reader.and_then(|reader| {
        reader
            .take((api::MAX_REQUEST_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
    });
    match read {
        Ok(_) => api::dispatch(&bytes),
        Err(error) => Response::error(ApiError::new(ErrorCode::IoFailed, error.to_string())),
    }
}
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let (value, success) = match args.first().and_then(|v| v.to_str()) {
        Some("capabilities") if args.len() == 1 => (api::capabilities(), true),
        Some("request") if args.len() <= 2 => {
            let response = request(args.get(1).map(|p| p.as_os_str()));
            let success = response.is_ok();
            (serde_json::to_value(response).expect("JSON-compatible API response"), success)
        }
        _ => (serde_json::to_value(Response::error(ApiError::new(ErrorCode::Usage, "usage: openwam capabilities | openwam request [request.json|-]; omitted file reads stdin"))).unwrap(), false),
    };
    let bytes = serde_json::to_vec(&value).expect("JSON-compatible API response");
    let (bytes, success) = if bytes.len() > api::MAX_RESPONSE_BYTES {
        (
            serde_json::to_vec(&Response::error(ApiError::new(
                ErrorCode::OutputTooLarge,
                "response exceeds byte budget",
            )))
            .unwrap(),
            false,
        )
    } else {
        (bytes, success)
    };
    let mut stdout = std::io::stdout().lock();
    if let Err(error) = stdout
        .write_all(&bytes)
        .and_then(|_| stdout.write_all(b"\n"))
    {
        eprintln!("writing OpenWAM response: {error}");
        return ExitCode::from(3);
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}
