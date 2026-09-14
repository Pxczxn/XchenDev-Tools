use crate::domain::{LaunchSessionInfo, LaunchSessionState};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

struct LaunchSession {
    info: LaunchSessionInfo,
    child: Option<Child>,
}

#[derive(Clone)]
pub struct CommandRunnerState {
    inner: Arc<Inner>,
}

struct Inner {
    sessions: Mutex<HashMap<String, LaunchSession>>,
    profile_running: Mutex<HashMap<String, String>>,
}

impl CommandRunnerState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                sessions: Mutex::new(HashMap::new()),
                profile_running: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn start(
        &self,
        app: &AppHandle,
        profile_id: &str,
        working_directory: &str,
        command: &str,
    ) -> Result<LaunchSessionInfo, String> {
        if self
            .inner
            .profile_running
            .lock()
            .map_err(|_| "lock")?
            .contains_key(profile_id)
        {
            return Err("LAUNCH_ALREADY_RUNNING:该配置已有运行会话".to_string());
        }

        let session_id = Uuid::new_v4().to_string();
        let mut child = Command::new("cmd")
            .args(["/C", command])
            .current_dir(working_directory)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("LAUNCH_START_FAILED:{}", e))?;

        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let info = LaunchSessionInfo {
            launch_session_id: session_id.clone(),
            profile_id: profile_id.to_string(),
            pid: Some(pid),
            state: LaunchSessionState::Running,
            exit_code: None,
        };

        self.inner.sessions.lock().map_err(|_| "lock")?.insert(
            session_id.clone(),
            LaunchSession {
                info: info.clone(),
                child: Some(child),
            },
        );
        self.inner
            .profile_running
            .lock()
            .map_err(|_| "lock")?
            .insert(profile_id.to_string(), session_id.clone());

        if let Some(out) = stdout {
            let app = app.clone();
            let sid = session_id.clone();
            std::thread::spawn(move || stream_lines(&app, &sid, "stdout", out));
        }
        if let Some(err) = stderr {
            let app = app.clone();
            let sid = session_id.clone();
            std::thread::spawn(move || stream_lines(&app, &sid, "stderr", err));
        }

        let runner = self.clone();
        let app = app.clone();
        let sid = session_id.clone();
        let profile_id_owned = profile_id.to_string();
        std::thread::spawn(move || wait_for_child(app, runner, sid, profile_id_owned));

        Ok(info)
    }

    pub fn stop(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.inner.sessions.lock().map_err(|_| "lock")?;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| "LAUNCH_SESSION_NOT_FOUND:会话不存在".to_string())?;
        session.info.state = LaunchSessionState::Stopping;
        if let Some(child) = &mut session.child {
            let _ = child.kill();
        }
        Ok(())
    }

    pub fn get(&self, session_id: &str) -> Option<LaunchSessionInfo> {
        self.inner
            .sessions
            .lock()
            .ok()
            .and_then(|m| m.get(session_id).map(|s| s.info.clone()))
    }

    pub fn list_sessions(&self) -> Vec<LaunchSessionInfo> {
        self.inner
            .sessions
            .lock()
            .map(|m| m.values().map(|s| s.info.clone()).collect())
            .unwrap_or_default()
    }
}

fn stream_lines<R: Read>(app: &AppHandle, session_id: &str, stream: &str, pipe: R) {
    let reader = BufReader::new(pipe);
    for line in reader.lines() {
        match line {
            Ok(chunk) => {
                let _ = app.emit(
                    "launch_output",
                    serde_json::json!({
                        "launchSessionId": session_id,
                        "stream": stream,
                        "chunk": chunk,
                        "final": false
                    }),
                );
            }
            Err(_) => break,
        }
    }
}

fn wait_for_child(
    app: AppHandle,
    runner: CommandRunnerState,
    session_id: String,
    profile_id: String,
) {
    let exit_code = {
        let mut guard = runner.inner.sessions.lock().unwrap();
        let session = guard.get_mut(&session_id);
        if let Some(session) = session {
            if let Some(mut child) = session.child.take() {
                let status = child.wait();
                session.info.state = LaunchSessionState::Stopped;
                match status {
                    Ok(s) => {
                        let code = s.code();
                        session.info.exit_code = code;
                        code
                    }
                    Err(_) => {
                        session.info.state = LaunchSessionState::Failed;
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    };

    let _ = app.emit(
        "launch_output",
        serde_json::json!({
            "launchSessionId": session_id,
            "stream": "exit",
            "chunk": "",
            "final": true,
            "exitCode": exit_code,
            "finalState": "STOPPED"
        }),
    );

    runner
        .inner
        .profile_running
        .lock()
        .unwrap()
        .remove(&profile_id);
}
