//! Launch command sessions. After F001, `wait_for_child` waits outside `sessions` lock.
//! F002: `stop()` kills via `session.child` when present, otherwise `taskkill /PID` on the direct
//! child (no `/T` — process-tree teardown is F003).
//! Follow-up: `launch_output.finalState` is always `"STOPPED"` even when the session is `Failed`.

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
        let plan = {
            let mut sessions = self.inner.sessions.lock().map_err(|_| "lock")?;
            let session = sessions
                .get_mut(session_id)
                .ok_or_else(|| "LAUNCH_SESSION_NOT_FOUND:会话不存在".to_string())?;
            match session.info.state {
                LaunchSessionState::Stopped | LaunchSessionState::Failed => return Ok(()),
                LaunchSessionState::Stopping => StopPlan {
                    pid: session.info.pid,
                    must_attempt: false,
                },
                LaunchSessionState::Running | LaunchSessionState::Starting => {
                    session.info.state = LaunchSessionState::Stopping;
                    StopPlan {
                        pid: session.info.pid,
                        must_attempt: true,
                    }
                }
            }
        };

        let mut terminated = false;
        {
            let mut sessions = self.inner.sessions.lock().map_err(|_| "lock")?;
            if let Some(session) = sessions.get_mut(session_id) {
                if let Some(child) = session.child.as_mut() {
                    terminated = child.kill().is_ok();
                }
            }
        }
        if !terminated {
            terminated = attempt_terminate(plan.pid);
        }
        if plan.must_attempt && !terminated {
            return Err("LAUNCH_STOP_FAILED:无法向子进程发起终止".to_string());
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

struct StopPlan {
    pid: Option<u32>,
    must_attempt: bool,
}

fn attempt_terminate(pid: Option<u32>) -> bool {
    pid.is_some_and(terminate_direct_child)
}

#[cfg(windows)]
fn terminate_direct_child(pid: u32) -> bool {
    let pid_arg = pid.to_string();
    Command::new("taskkill")
        .args(["/PID", &pid_arg, "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn terminate_direct_child(_pid: u32) -> bool {
    false
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

fn wait_on_child(runner: &CommandRunnerState, session_id: &str) -> (Option<i32>, LaunchSessionState) {
    let child = {
        let mut guard = runner.inner.sessions.lock().unwrap();
        guard
            .get_mut(session_id)
            .and_then(|session| session.child.take())
    };

    match child {
        Some(mut child) => match child.wait() {
            Ok(status) => (status.code(), LaunchSessionState::Stopped),
            Err(_) => (None, LaunchSessionState::Failed),
        },
        None => (None, LaunchSessionState::Stopped),
    }
}

/// Clears `profile_running` before publishing `Stopped` / `Failed` so a terminal `get()` never
/// races with `LAUNCH_ALREADY_RUNNING` on immediate restart.
fn publish_terminal_session(
    runner: &CommandRunnerState,
    session_id: &str,
    profile_id: &str,
    exit_code: Option<i32>,
    final_state: LaunchSessionState,
) -> Option<i32> {
    runner
        .inner
        .profile_running
        .lock()
        .unwrap()
        .remove(profile_id);

    {
        let mut guard = runner.inner.sessions.lock().unwrap();
        if let Some(session) = guard.get_mut(session_id) {
            session.info.state = final_state;
            session.info.exit_code = exit_code;
        }
    }

    exit_code
}

fn finalize_child_exit(
    runner: &CommandRunnerState,
    session_id: &str,
    profile_id: &str,
) -> Option<i32> {
    let (exit_code, final_state) = wait_on_child(runner, session_id);
    publish_terminal_session(runner, session_id, profile_id, exit_code, final_state)
}

fn wait_for_child(
    app: AppHandle,
    runner: CommandRunnerState,
    session_id: String,
    profile_id: String,
) {
    let exit_code = finalize_child_exit(&runner, &session_id, &profile_id);

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
    finalize_child_exit(&runner, &session_id, &profile_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as OsCommand;
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

    fn is_pid_alive(pid: u32) -> bool {
        let filter = format!("PID eq {}", pid);
        let output = OsCommand::new("tasklist")
            .args(["/FI", &filter, "/NH"])
            .output()
            .expect("tasklist");
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains(&pid.to_string()) && !stdout.contains("INFO: No tasks")
    }

    fn assert_pid_exits(pid: u32, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !is_pid_alive(pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("pid {} still alive after {:?}", pid, timeout);
    }

    fn assert_never_running_again(
        runner: &CommandRunnerState,
        session_id: &str,
        timeout: Duration,
    ) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(info) = runner.get(session_id) {
                assert_ne!(info.state, LaunchSessionState::Running);
                if matches!(
                    info.state,
                    LaunchSessionState::Stopped | LaunchSessionState::Failed
                ) {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "session {:?} did not reach terminal state within {:?}",
            session_id, timeout
        );
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

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(session) = runner.get(&info.launch_session_id) {
                if matches!(
                    session.state,
                    LaunchSessionState::Stopped | LaunchSessionState::Failed
                ) {
                    let second = runner
                        .start_for_test(profile_id, ".", "exit 0")
                        .expect(
                            "second start must succeed immediately once terminal state is observable",
                        );
                    wait_until_terminal(&runner, &second.launch_session_id, Duration::from_secs(5));
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("session did not reach terminal state within timeout");
    }

    #[test]
    fn stop_terminates_direct_child() {
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test("profile-stop-kill", ".", "ping -n 30 127.0.0.1")
            .expect("start");
        let pid = info.pid.expect("pid");
        std::thread::sleep(Duration::from_millis(200));
        runner.stop(&info.launch_session_id).expect("stop should succeed");
        assert_pid_exits(pid, Duration::from_secs(3));
        let final_info =
            wait_until_terminal(&runner, &info.launch_session_id, Duration::from_secs(5));
        assert!(matches!(
            final_info.state,
            LaunchSessionState::Stopped | LaunchSessionState::Failed
        ));
    }

    #[test]
    fn stop_reaches_terminal_state_without_returning_to_running() {
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test("profile-stop-terminal", ".", "ping -n 30 127.0.0.1")
            .expect("start");
        std::thread::sleep(Duration::from_millis(200));
        runner.stop(&info.launch_session_id).expect("stop should succeed");
        if let Some(after_stop) = runner.get(&info.launch_session_id) {
            assert_ne!(after_stop.state, LaunchSessionState::Running);
        }
        let final_info =
            wait_until_terminal(&runner, &info.launch_session_id, Duration::from_secs(5));
        assert!(matches!(
            final_info.state,
            LaunchSessionState::Stopped | LaunchSessionState::Failed
        ));
    }

    #[test]
    fn stop_is_idempotent_after_terminal() {
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test("profile-stop-idempotent", ".", "exit 0")
            .expect("start");
        wait_until_terminal(&runner, &info.launch_session_id, Duration::from_secs(5));
        runner
            .stop(&info.launch_session_id)
            .expect("stop on terminal session should be idempotent");
        runner
            .stop(&info.launch_session_id)
            .expect("duplicate stop should remain idempotent");
        let final_info = runner.get(&info.launch_session_id).expect("session");
        assert!(matches!(
            final_info.state,
            LaunchSessionState::Stopped | LaunchSessionState::Failed
        ));
    }

    #[test]
    fn wait_and_stop_race_still_terminates_direct_child() {
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test("profile-stop-race", ".", "ping -n 30 127.0.0.1")
            .expect("start");
        let session_id = info.launch_session_id.clone();
        let pid = info.pid.expect("pid");
        let runner_bg = runner.clone();
        let sid = session_id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            let _ = runner_bg.stop(&sid);
        });
        std::thread::sleep(Duration::from_millis(50));
        runner.stop(&session_id).expect("stop should succeed under race");
        assert_pid_exits(pid, Duration::from_secs(3));
        assert_never_running_again(&runner, &session_id, Duration::from_secs(5));
    }
}
