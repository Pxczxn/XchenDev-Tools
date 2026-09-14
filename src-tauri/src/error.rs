use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum AppError {
    #[error("IPC not ready")]
    IpcNotReady,
    #[error("{0}")]
    Message(String),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            AppError::IpcNotReady => "IPC_NOT_READY",
            AppError::Message(msg) => error_code_from_message(msg),
        }
    }

    pub fn from_code(code: &str, message: impl Into<String>) -> Self {
        AppError::Message(format!("{}:{}", code, message.into()))
    }
}

fn error_code_from_message(msg: &str) -> &'static str {
    if let Some((code, _)) = msg.split_once(':') {
        return match code {
            "PATH_NOT_FOUND" => "PATH_NOT_FOUND",
            "EXECUTABLE_INVALID" => "EXECUTABLE_INVALID",
            "DETECT_PERMISSION_DENIED" => "DETECT_PERMISSION_DENIED",
            "DETECT_IO_ERROR" => "DETECT_IO_ERROR",
            "PORT_INVALID" => "PORT_INVALID",
            "PORT_QUERY_FAILED" => "PORT_QUERY_FAILED",
            "PROCESS_PROTECTED" => "PROCESS_PROTECTED",
            "PROCESS_NOT_FOUND" => "PROCESS_NOT_FOUND",
            "TERMINATE_DENIED" => "TERMINATE_DENIED",
            "TERMINATE_FAILED" => "TERMINATE_FAILED",
            "DIRECTORY_INVALID" => "DIRECTORY_INVALID",
            "PROCESS_QUERY_FAILED" => "PROCESS_QUERY_FAILED",
            "PROCESS_SNAPSHOT_MISMATCH" => "PROCESS_SNAPSHOT_MISMATCH",
            "PROJECT_PATH_INVALID" => "PROJECT_PATH_INVALID",
            "PROJECT_SCAN_DENIED" => "PROJECT_SCAN_DENIED",
            "PROJECT_SCAN_FAILED" => "PROJECT_SCAN_FAILED",
            "WORKDIR_INVALID" => "WORKDIR_INVALID",
            "COMMAND_POLICY_REJECTED" => "COMMAND_POLICY_REJECTED",
            "PROFILE_INVALID" => "PROFILE_INVALID",
            "PROFILE_NOT_FOUND" => "PROFILE_NOT_FOUND",
            "CONFIRMATION_ISSUE_FAILED" => "CONFIRMATION_ISSUE_FAILED",
            "LAUNCH_CONFIRMATION_REQUIRED" => "LAUNCH_CONFIRMATION_REQUIRED",
            "LAUNCH_START_FAILED" => "LAUNCH_START_FAILED",
            "LAUNCH_ALREADY_RUNNING" => "LAUNCH_ALREADY_RUNNING",
            "LAUNCH_SESSION_NOT_FOUND" => "LAUNCH_SESSION_NOT_FOUND",
            "LAUNCH_STOP_FAILED" => "LAUNCH_STOP_FAILED",
            _ => "UNKNOWN",
        };
    }
    "UNKNOWN"
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorDto {
    pub code: String,
    pub message: String,
}

impl From<AppError> for ErrorDto {
    fn from(value: AppError) -> Self {
        ErrorDto {
            code: value.code().to_string(),
            message: value.to_string(),
        }
    }
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ErrorDto::from(self.clone()).serialize(serializer)
    }
}
