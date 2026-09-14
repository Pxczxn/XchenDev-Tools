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

fn take_session_child(runner: &CommandRunnerState, session_id: &str) -> Option<Child> {
    let mut guard = runner.inner.sessions.lock().unwrap();
    guard
        .get_mut(session_id)
        .and_then(|session| session.child.take())
}

fn complete_session_after_child_exit(
    runner: &CommandRunnerState,
    session_id: &str,
    profile_id: &str,
    mut child: Child,
) -> Option<i32> {
    let exit_code = match child.wait() {
        Ok(status) => {
            let code = status.code();
            let mut guard = runner.inner.sessions.lock().unwrap();
            if let Some(session) = guard.get_mut(session_id) {
                session.info.exit_code = code;
                session.info.state = LaunchSessionState::Stopped;
            }
            code
        }
        Err(_) => {
            let mut guard = runner.inner.sessions.lock().unwrap();
            if let Some(session) = guard.get_mut(session_id) {
                session.info.state = LaunchSessionState::Failed;
            }
            None
        }
    };

    runner
        .inner
        .profile_running
        .lock()
        .unwrap()
        .remove(profile_id);

    exit_code
}

#[cfg(test)]
fn await_child_exit_without_emit(
    runner: CommandRunnerState,
    session_id: String,
    profile_id: String,
) {
    let child = take_session_child(&runner, &session_id);
    if let Some(child) = child {
        complete_session_after_child_exit(&runner, &session_id, &profile_id, child);
    } else {
        runner
            .inner
            .profile_running
            .lock()
            .unwrap()
            .remove(&profile_id);
    }
}

// F002: wait_for_child takes the Child before waiting; stop() may set Stopping without
// killing the process if child is already taken. Process-tree teardown is F003.
fn wait_for_child(
    app: AppHandle,
    runner: CommandRunnerState,
    session_id: String,
    profile_id: String,
) {
    let child = take_session_child(&runner, &session_id);
    let exit_code = match child {
        Some(child) => {
            complete_session_after_child_exit(&runner, &session_id, &profile_id, child)
        }
        None => {
            runner
                .inner
                .profile_running
                .lock()
                .unwrap()
                .remove(&profile_id);
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
}

#[cfg(test)]
mod session_lock_tests {
    use super::*;
    use crate::domain::LaunchSessionState;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use uuid::Uuid;

    fn working_directory() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    fn spawn_cmd(command: &str) -> Child {
        Command::new("cmd")
            .args(["/C", command])
            .current_dir(working_directory())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn cmd")
    }

    fn insert_running_session(
        runner: &CommandRunnerState,
        profile_id: &str,
        child: Child,
    ) -> String {
        let session_id = Uuid::new_v4().to_string();
        let info = LaunchSessionInfo {
            launch_session_id: session_id.clone(),
            profile_id: profile_id.to_string(),
            pid: Some(child.id()),
            state: LaunchSessionState::Running,
            exit_code: None,
        };
        runner.inner.sessions.lock().unwrap().insert(
            session_id.clone(),
            LaunchSession {
                info,
                child: Some(child),
            },
        );
        runner
            .inner
            .profile_running
            .lock()
            .unwrap()
            .insert(profile_id.to_string(), session_id.clone());
        session_id
    }

    fn run_wait_thread(runner: CommandRunnerState, session_id: String, profile_id: String) {
        std::thread::spawn(move || {
            await_child_exit_without_emit(runner, session_id, profile_id);
        });
    }

    fn wait_until_running(runner: &CommandRunnerState, session_id: &str) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if runner
                .get(session_id)
                .is_some_and(|s| s.state == LaunchSessionState::Running)
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("session did not reach Running");
    }

    fn wait_until_stopped(runner: &CommandRunnerState, session_id: &str, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(info) = runner.get(session_id) {
                if matches!(
                    info.state,
                    LaunchSessionState::Stopped | LaunchSessionState::Failed
                ) {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("session did not stop within timeout");
    }

    fn start_long_running(runner: &CommandRunnerState, profile_id: &str) -> String {
        let child = spawn_cmd("ping -n 12 127.0.0.1");
        let session_id = insert_running_session(runner, profile_id, child);
        run_wait_thread(runner.clone(), session_id.clone(), profile_id.to_string());
        session_id
    }

    fn start_short(runner: &CommandRunnerState, profile_id: &str, command: &str) -> String {
        let child = spawn_cmd(command);
        let session_id = insert_running_session(runner, profile_id, child);
        run_wait_thread(runner.clone(), session_id.clone(), profile_id.to_string());
        session_id
    }

    #[test]
    fn list_sessions_remains_responsive_while_child_running() {
        let runner = CommandRunnerState::new();
        let session_id = start_long_running(&runner, "profile-list-responsive");
        wait_until_running(&runner, &session_id);

        let (tx, rx) = mpsc::channel();
        let runner_bg = runner.clone();
        let sid = session_id.clone();
        std::thread::spawn(move || {
            for _ in 0..20 {
                let sessions = runner_bg.list_sessions();
                if sessions.iter().any(|s| {
                    s.launch_session_id == sid && s.state == LaunchSessionState::Running
                }) {
                    let _ = tx.send(());
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });

        rx.recv_timeout(Duration::from_millis(500))
            .expect("list_sessions blocked or running session not visible");

        wait_until_stopped(&runner, &session_id, Duration::from_secs(20));
    }

    #[test]
    fn get_remains_responsive_while_child_running() {
        let runner = CommandRunnerState::new();
        let session_id = start_long_running(&runner, "profile-get-responsive");
        wait_until_running(&runner, &session_id);

        let (tx, rx) = mpsc::channel();
        let runner_bg = runner.clone();
        let sid = session_id.clone();
        std::thread::spawn(move || {
            for _ in 0..20 {
                if runner_bg
                    .get(&sid)
                    .is_some_and(|s| s.state == LaunchSessionState::Running)
                {
                    let _ = tx.send(());
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });

        rx.recv_timeout(Duration::from_millis(500))
            .expect("get blocked or running session not visible");

        wait_until_stopped(&runner, &session_id, Duration::from_secs(20));
    }

    #[test]
    fn session_reaches_stopped_after_natural_exit() {
        let runner = CommandRunnerState::new();
        let session_id = start_short(&runner, "profile-natural-exit", "exit 0");

        wait_until_stopped(&runner, &session_id, Duration::from_secs(5));

        let final_info = runner
            .get(&session_id)
            .expect("session should still be listed");
        assert_eq!(final_info.state, LaunchSessionState::Stopped);
        assert_eq!(final_info.exit_code, Some(0));
    }

    #[test]
    fn profile_can_start_again_after_natural_exit() {
        let runner = CommandRunnerState::new();
        let profile_id = "profile-restart-after-exit";

        let first_id = start_short(&runner, profile_id, "exit 0");
        wait_until_stopped(&runner, &first_id, Duration::from_secs(5));

        assert!(
            !runner
                .inner
                .profile_running
                .lock()
                .unwrap()
                .contains_key(profile_id)
        );

        let second_id = start_short(&runner, profile_id, "exit 0");
        assert_eq!(
            runner.get(&second_id).map(|s| s.state),
            Some(LaunchSessionState::Running)
        );

        wait_until_stopped(&runner, &second_id, Duration::from_secs(5));
    }
}
