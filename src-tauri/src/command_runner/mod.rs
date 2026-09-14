//! Launch command sessions. After F001, `wait_for_child` waits outside `sessions` lock.
//! Known limitation (F002): once `wait_for_child` has `take()`n the `Child`, `stop()` may set
//! `Stopping` without killing the process because `session.child` is already `None`.

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

fn reap_child(runner: &CommandRunnerState, session_id: &str) -> Option<i32> {
    let child = {
        let mut guard = runner.inner.sessions.lock().unwrap();
        guard
            .get_mut(session_id)
            .and_then(|session| session.child.take())
    };

    let (exit_code, final_state) = match child {
        Some(mut child) => match child.wait() {
            Ok(status) => (status.code(), LaunchSessionState::Stopped),
            Err(_) => (None, LaunchSessionState::Failed),
        },
        None => (None, LaunchSessionState::Stopped),
    };

    {
        let mut guard = runner.inner.sessions.lock().unwrap();
        if let Some(session) = guard.get_mut(session_id) {
            session.info.state = final_state;
            session.info.exit_code = exit_code;
        }
    }

    exit_code
}

fn wait_for_child(
    app: AppHandle,
    runner: CommandRunnerState,
    session_id: String,
    profile_id: String,
) {
    let exit_code = reap_child(&runner, &session_id);

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

#[cfg(test)]
impl CommandRunnerState {
    /// Spawns a session and reaper thread without stdout/stderr IPC (tests only).
    fn start_for_test(
        &self,
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
        let child = Command::new("cmd")
            .args(["/C", command])
            .current_dir(working_directory)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("LAUNCH_START_FAILED:{}", e))?;

        let pid = child.id();
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

        let runner = self.clone();
        let sid = session_id.clone();
        let profile_id_owned = profile_id.to_string();
        std::thread::spawn(move || wait_for_child_no_emit(runner, sid, profile_id_owned));

        Ok(info)
    }
}

#[cfg(test)]
fn wait_for_child_no_emit(
    runner: CommandRunnerState,
    session_id: String,
    profile_id: String,
) {
    reap_child(&runner, &session_id);
    runner
        .inner
        .profile_running
        .lock()
        .unwrap()
        .remove(&profile_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn wait_until_terminal(
        runner: &CommandRunnerState,
        session_id: &str,
        timeout: Duration,
    ) -> LaunchSessionInfo {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(info) = runner.get(session_id) {
                if matches!(
                    info.state,
                    LaunchSessionState::Stopped | LaunchSessionState::Failed
                ) {
                    return info;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("session {:?} did not reach terminal state within {:?}", session_id, timeout);
    }

    fn assert_query_responsive<F>(query: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            query();
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_millis(500))
            .expect("query should return within 500ms while child is running");
    }

    #[test]
    fn list_sessions_remains_responsive_while_child_running() {
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test("profile-list", ".", "ping -n 12 127.0.0.1")
            .expect("start");
        let session_id = info.launch_session_id.clone();

        std::thread::sleep(Duration::from_millis(200));

        let runner_for_query = runner.clone();
        let sid_for_assert = session_id.clone();
        assert_query_responsive(move || {
            let sessions = runner_for_query.list_sessions();
            assert!(
                sessions
                    .iter()
                    .any(|s| s.launch_session_id == sid_for_assert && s.state == LaunchSessionState::Running)
            );
        });

        wait_until_terminal(&runner, &session_id, Duration::from_secs(20));
    }

    #[test]
    fn get_remains_responsive_while_child_running() {
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test("profile-get", ".", "ping -n 12 127.0.0.1")
            .expect("start");
        let session_id = info.launch_session_id.clone();

        std::thread::sleep(Duration::from_millis(200));

        let runner_for_query = runner.clone();
        let sid = session_id.clone();
        assert_query_responsive(move || {
            let session = runner_for_query
                .get(&sid)
                .expect("session should exist");
            assert_eq!(session.state, LaunchSessionState::Running);
        });

        wait_until_terminal(&runner, &session_id, Duration::from_secs(20));
    }

    #[test]
    fn session_reaches_stopped_after_natural_exit() {
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test("profile-exit", ".", "exit 0")
            .expect("start");
        let session = wait_until_terminal(
            &runner,
            &info.launch_session_id,
            Duration::from_secs(5),
        );
        assert_eq!(session.state, LaunchSessionState::Stopped);
        assert_eq!(session.exit_code, Some(0));
    }

    #[test]
    fn profile_can_start_again_after_natural_exit() {
        let runner = CommandRunnerState::new();
        let profile_id = "profile-restart";
        let info = runner
            .start_for_test(profile_id, ".", "exit 0")
            .expect("start");
        wait_until_terminal(&runner, &info.launch_session_id, Duration::from_secs(5));

        let second = runner
            .start_for_test(profile_id, ".", "exit 0")
            .expect("second start should succeed after profile_running cleanup");
        wait_until_terminal(&runner, &second.launch_session_id, Duration::from_secs(5));
    }
}
