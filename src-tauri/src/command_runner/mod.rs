//! Launch command sessions. After F001, wait runs outside `sessions` lock.
//! F002: identity-verified `taskkill /PID` fallback when Job terminate is unavailable.
//! F003: Windows Job Object owns the session process tree; stop uses `TerminateJobObject` first.

use crate::domain::{LaunchSessionInfo, LaunchSessionState};
use crate::process_manager::process_start_time_secs;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

const MAX_TERMINAL_SESSIONS: usize = 64;

#[cfg(windows)]
mod windows_job;

#[cfg(not(windows))]
use std::process::{Child, Command, Stdio};

/// Direct child identity for PID fallback (not exposed on `LaunchSessionInfo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirectChildIdentity {
    pid: u32,
    start_time_secs: u64,
}

impl DirectChildIdentity {
    fn capture(pid: u32) -> Result<Self, String> {
        let start_time_secs = process_start_time_secs(pid).ok_or_else(|| {
            "LAUNCH_START_FAILED:无法读取子进程启动时间".to_string()
        })?;
        Ok(Self {
            pid,
            start_time_secs,
        })
    }
}

enum PidIdentityVerdict {
    Gone,
    Matches,
    Mismatch,
}

fn verify_pid_identity(expected: &DirectChildIdentity) -> PidIdentityVerdict {
    match process_start_time_secs(expected.pid) {
        None => PidIdentityVerdict::Gone,
        Some(start_time_secs) if start_time_secs == expected.start_time_secs => {
            PidIdentityVerdict::Matches
        }
        Some(_) => PidIdentityVerdict::Mismatch,
    }
}

#[cfg(windows)]
struct LaunchSession {
    info: LaunchSessionInfo,
    job: Option<windows_job::SessionJob>,
    process: Option<windows_job::OwnedProcess>,
    direct_child: DirectChildIdentity,
}

#[cfg(not(windows))]
struct LaunchSession {
    info: LaunchSessionInfo,
    child: Option<Child>,
    direct_child: DirectChildIdentity,
}

#[derive(Clone)]
pub struct CommandRunnerState {
    inner: Arc<Inner>,
}

struct Inner {
    sessions: Mutex<HashMap<String, LaunchSession>>,
    profile_running: Mutex<HashMap<String, String>>,
    terminal_sessions: Mutex<VecDeque<String>>,
}

impl CommandRunnerState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                sessions: Mutex::new(HashMap::new()),
                profile_running: Mutex::new(HashMap::new()),
                terminal_sessions: Mutex::new(VecDeque::new()),
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

        let (info, stdout, stderr) =
            register_session(self, profile_id, working_directory, command, true, true)?;

        spawn_io_and_wait(app, self, &info, stdout, stderr, profile_id);

        Ok(info)
    }

    pub fn stop(&self, session_id: &str) -> Result<(), String> {
        let direct_child = {
            let mut sessions = self.inner.sessions.lock().map_err(|_| "lock")?;
            let session = sessions
                .get_mut(session_id)
                .ok_or_else(|| "LAUNCH_SESSION_NOT_FOUND:会话不存在".to_string())?;
            match session.info.state {
                LaunchSessionState::Stopped | LaunchSessionState::Failed => return Ok(()),
                LaunchSessionState::Stopping => session.direct_child,
                LaunchSessionState::Running | LaunchSessionState::Starting => {
                    session.info.state = LaunchSessionState::Stopping;
                    session.direct_child
                }
            }
        };

        attempt_stop_terminate(self, session_id, direct_child)
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

fn register_session(
    runner: &CommandRunnerState,
    profile_id: &str,
    working_directory: &str,
    command: &str,
    pipe_stdout: bool,
    pipe_stderr: bool,
) -> Result<(LaunchSessionInfo, Option<std::fs::File>, Option<std::fs::File>), String> {
    let session_id = Uuid::new_v4().to_string();

    #[cfg(windows)]
    {
        let spawned = windows_job::spawn_cmd_session(
            working_directory,
            command,
            pipe_stdout,
            pipe_stderr,
        )?;
        let direct_child = DirectChildIdentity::capture(spawned.pid)?;
        let pid = spawned.pid;
        let stdout = spawned.stdout;
        let stderr = spawned.stderr;

        let info = LaunchSessionInfo {
            launch_session_id: session_id.clone(),
            profile_id: profile_id.to_string(),
            pid: Some(pid),
            state: LaunchSessionState::Running,
            exit_code: None,
        };

        runner.inner.sessions.lock().map_err(|_| "lock")?.insert(
            session_id.clone(),
            LaunchSession {
                info: info.clone(),
                job: Some(spawned.job),
                process: Some(spawned.process),
                direct_child,
            },
        );
        runner
            .inner
            .profile_running
            .lock()
            .map_err(|_| "lock")?
            .insert(profile_id.to_string(), session_id);

        return Ok((info, stdout, stderr));
    }

    #[cfg(not(windows))]
    {
        let mut child = Command::new("cmd")
            .args(["/C", command])
            .current_dir(working_directory)
            .stdout(if pipe_stdout {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(if pipe_stderr {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .spawn()
            .map_err(|e| format!("LAUNCH_START_FAILED:{}", e))?;

        let pid = child.id();
        let direct_child = DirectChildIdentity::capture(pid)?;
        let stdout = if pipe_stdout {
            child.stdout.take()
        } else {
            None
        };
        let stderr = if pipe_stderr {
            child.stderr.take()
        } else {
            None
        };

        let info = LaunchSessionInfo {
            launch_session_id: session_id.clone(),
            profile_id: profile_id.to_string(),
            pid: Some(pid),
            state: LaunchSessionState::Running,
            exit_code: None,
        };

        runner.inner.sessions.lock().map_err(|_| "lock")?.insert(
            session_id.clone(),
            LaunchSession {
                info: info.clone(),
                child: Some(child),
                direct_child,
            },
        );
        runner
            .inner
            .profile_running
            .lock()
            .map_err(|_| "lock")?
            .insert(profile_id.to_string(), session_id);

        Ok((info, stdout, stderr))
    }
}

fn spawn_io_and_wait(
    app: &AppHandle,
    runner: &CommandRunnerState,
    info: &LaunchSessionInfo,
    stdout: Option<impl Read + Send + 'static>,
    stderr: Option<impl Read + Send + 'static>,
    profile_id: &str,
) {
    let session_id = info.launch_session_id.clone();
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

    let runner = runner.clone();
    let app = app.clone();
    let sid = session_id.clone();
    let profile_id_owned = profile_id.to_string();
    std::thread::spawn(move || wait_for_child(app, runner, sid, profile_id_owned));
}

fn attempt_stop_terminate(
    runner: &CommandRunnerState,
    session_id: &str,
    direct_child: DirectChildIdentity,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        if !test_skip_job_terminate() {
            let job_result = {
                let mut sessions = runner.inner.sessions.lock().map_err(|_| "lock")?;
                sessions
                    .get_mut(session_id)
                    .and_then(|session| session.job.as_ref().map(|job| job.terminate()))
            };
            if job_result == Some(Ok(())) {
                return Ok(());
            }
        }
    }

    #[cfg(not(windows))]
    {
        let mut sessions = runner.inner.sessions.lock().map_err(|_| "lock")?;
        if let Some(session) = sessions.get_mut(session_id) {
            if let Some(child) = session.child.as_mut() {
                if child.kill().is_ok() {
                    return Ok(());
                }
            }
        }
    }

    #[cfg(windows)]
    {
        let terminate_via_handle = {
            let mut sessions = runner.inner.sessions.lock().map_err(|_| "lock")?;
            if let Some(session) = sessions.get_mut(session_id) {
                if let Some(process) = session.process.as_ref() {
                    unsafe {
                        use windows::Win32::System::Threading::TerminateProcess;
                        TerminateProcess(process.raw(), 1).is_ok()
                    }
                } else {
                    false
                }
            } else {
                false
            }
        };
        if terminate_via_handle {
            return Ok(());
        }
    }

    match verify_pid_identity(&direct_child) {
        PidIdentityVerdict::Gone => Ok(()),
        PidIdentityVerdict::Mismatch => {
            Err("LAUNCH_PROCESS_IDENTITY_MISMATCH:PID 已被其他进程复用".to_string())
        }
        PidIdentityVerdict::Matches => {
            if terminate_direct_child(direct_child.pid) {
                Ok(())
            } else {
                match verify_pid_identity(&direct_child) {
                    PidIdentityVerdict::Gone => Ok(()),
                    _ => Err("LAUNCH_STOP_FAILED:无法向子进程发起终止".to_string()),
                }
            }
        }
    }
}

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
static TEST_SKIP_JOB_TERMINATE: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
static TEST_FORCE_TASKKILL_FAIL: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
fn test_skip_job_terminate() -> bool {
    TEST_SKIP_JOB_TERMINATE.load(Ordering::SeqCst)
}

#[cfg(not(test))]
fn test_skip_job_terminate() -> bool {
    false
}

#[cfg(test)]
fn test_force_taskkill_fail() -> bool {
    TEST_FORCE_TASKKILL_FAIL.load(Ordering::SeqCst)
}

#[cfg(not(test))]
fn test_force_taskkill_fail() -> bool {
    false
}

#[cfg(windows)]
fn terminate_direct_child(pid: u32) -> bool {
    if test_force_taskkill_fail() {
        return false;
    }
    use std::process::{Command, Stdio};
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

fn terminal_state_after_wait(
    runner: &CommandRunnerState,
    session_id: &str,
    exit_code: Option<i32>,
    wait_failed: bool,
) -> LaunchSessionState {
    if wait_failed {
        return LaunchSessionState::Failed;
    }
    if runner
        .get(session_id)
        .is_some_and(|session| session.state == LaunchSessionState::Stopping)
    {
        return LaunchSessionState::Stopped;
    }
    if exit_code == Some(0) {
        LaunchSessionState::Stopped
    } else {
        LaunchSessionState::Failed
    }
}

fn wait_on_process(
    runner: &CommandRunnerState,
    session_id: &str,
) -> (Option<i32>, LaunchSessionState) {
    #[cfg(windows)]
    {
        let process = {
            let mut guard = runner.inner.sessions.lock().unwrap();
            guard
                .get_mut(session_id)
                .and_then(|session| session.process.take())
        };
        match process {
            Some(process) => {
                let (code, failed) = process.wait();
                let final_state = terminal_state_after_wait(runner, session_id, code, failed);
                (code, final_state)
            }
            None => (None, LaunchSessionState::Stopped),
        }
    }

    #[cfg(not(windows))]
    {
        let child = {
            let mut guard = runner.inner.sessions.lock().unwrap();
            guard
                .get_mut(session_id)
                .and_then(|session| session.child.take())
        };
        match child {
            Some(mut child) => match child.wait() {
                Ok(status) => {
                    let code = status.code();
                    let final_state = terminal_state_after_wait(runner, session_id, code, false);
                    (code, final_state)
                }
                Err(_) => (None, LaunchSessionState::Failed),
            },
            None => (None, LaunchSessionState::Stopped),
        }
    }
}

fn remember_terminal_session(runner: &CommandRunnerState, session_id: &str) {
    let evicted = {
        let mut terminal = runner.inner.terminal_sessions.lock().unwrap();
        if terminal.iter().any(|id| id == session_id) {
            return;
        }
        terminal.push_back(session_id.to_string());
        let mut evicted = Vec::new();
        while terminal.len() > MAX_TERMINAL_SESSIONS {
            if let Some(id) = terminal.pop_front() {
                evicted.push(id);
            }
        }
        evicted
    };

    if evicted.is_empty() {
        return;
    }

    let mut sessions = runner.inner.sessions.lock().unwrap();
    for id in evicted {
        let is_terminal = sessions.get(&id).is_some_and(|session| {
            matches!(
                session.info.state,
                LaunchSessionState::Stopped | LaunchSessionState::Failed
            )
        });
        if is_terminal {
            sessions.remove(&id);
        }
    }
}

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

    #[cfg(windows)]
    let (job_released, published) = {
        let mut guard = runner.inner.sessions.lock().unwrap();
        if let Some(session) = guard.get_mut(session_id) {
            session.info.state = final_state;
            session.info.exit_code = exit_code;
            (session.job.take(), true)
        } else {
            (None, false)
        }
    };

    #[cfg(windows)]
    drop(job_released);

    #[cfg(not(windows))]
    let published = {
        let mut guard = runner.inner.sessions.lock().unwrap();
        if let Some(session) = guard.get_mut(session_id) {
            session.info.state = final_state;
            session.info.exit_code = exit_code;
            true
        } else {
            false
        }
    };

    if published {
        remember_terminal_session(runner, session_id);
    }

    exit_code
}

fn finalize_process_exit(
    runner: &CommandRunnerState,
    session_id: &str,
    profile_id: &str,
) -> Option<i32> {
    let (exit_code, final_state) = wait_on_process(runner, session_id);
    publish_terminal_session(runner, session_id, profile_id, exit_code, final_state)
}

fn wait_for_child(
    app: AppHandle,
    runner: CommandRunnerState,
    session_id: String,
    profile_id: String,
) {
    let exit_code = finalize_process_exit(&runner, &session_id, &profile_id);
    let final_state = runner
        .get(&session_id)
        .map(|session| session.state)
        .unwrap_or(LaunchSessionState::Stopped);

    let _ = app.emit(
        "launch_output",
        serde_json::json!({
            "launchSessionId": session_id,
            "stream": "exit",
            "chunk": "",
            "final": true,
            "exitCode": exit_code,
            "finalState": final_state
        }),
    );
}

#[cfg(test)]
impl CommandRunnerState {
    fn test_take_process(&self, session_id: &str) {
        #[cfg(windows)]
        {
            self.inner
                .sessions
                .lock()
                .unwrap()
                .get_mut(session_id)
                .expect("session")
                .process
                .take();
        }
        #[cfg(not(windows))]
        {
            self.inner
                .sessions
                .lock()
                .unwrap()
                .get_mut(session_id)
                .expect("session")
                .child
                .take();
        }
    }

    fn test_set_direct_child_start_time(&self, session_id: &str, wrong_start_time_secs: u64) {
        self.inner
            .sessions
            .lock()
            .unwrap()
            .get_mut(session_id)
            .expect("session")
            .direct_child
            .start_time_secs = wrong_start_time_secs;
    }

    fn test_insert_session_with_state(&self, session_id: &str, state: LaunchSessionState) {
        let info = LaunchSessionInfo {
            launch_session_id: session_id.to_string(),
            profile_id: format!("profile-{session_id}"),
            pid: None,
            state,
            exit_code: None,
        };
        let direct_child = DirectChildIdentity {
            pid: 0,
            start_time_secs: 0,
        };

        #[cfg(windows)]
        let session = LaunchSession {
            info,
            job: None,
            process: None,
            direct_child,
        };

        #[cfg(not(windows))]
        let session = LaunchSession {
            info,
            child: None,
            direct_child,
        };

        self.inner
            .sessions
            .lock()
            .unwrap()
            .insert(session_id.to_string(), session);
    }

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

        let (info, _, _) =
            register_session(self, profile_id, working_directory, command, false, false)?;

        let runner = self.clone();
        let sid = info.launch_session_id.clone();
        let profile_id_owned = profile_id.to_string();
        std::thread::spawn(move || wait_for_child_no_emit(runner, sid, profile_id_owned));

        Ok(info)
    }

    #[cfg(windows)]
    fn test_session_job_handle(&self, session_id: &str) -> windows::Win32::Foundation::HANDLE {
        self.inner
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .expect("session")
            .job
            .as_ref()
            .expect("job")
            .raw()
    }
}

#[cfg(test)]
fn wait_for_child_no_emit(
    runner: CommandRunnerState,
    session_id: String,
    profile_id: String,
) {
    finalize_process_exit(&runner, &session_id, &profile_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
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
        panic!(
            "session {:?} did not reach terminal state within {:?}",
            session_id, timeout
        );
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
        process_start_time_secs(pid).is_some()
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
    fn terminal_state_marks_nonzero_natural_exit_failed_but_user_stop_stopped() {
        let runner = CommandRunnerState::new();
        runner.test_insert_session_with_state("natural-failure", LaunchSessionState::Running);
        runner.test_insert_session_with_state("user-stop", LaunchSessionState::Stopping);

        assert_eq!(
            terminal_state_after_wait(&runner, "natural-failure", Some(7), false),
            LaunchSessionState::Failed
        );
        assert_eq!(
            terminal_state_after_wait(&runner, "user-stop", Some(1), false),
            LaunchSessionState::Stopped
        );
        assert_eq!(
            terminal_state_after_wait(&runner, "user-stop", None, true),
            LaunchSessionState::Failed
        );
    }

    #[test]
    fn terminal_session_history_is_bounded() {
        let runner = CommandRunnerState::new();
        let total = MAX_TERMINAL_SESSIONS + 5;
        let ids: Vec<String> = (0..total)
            .map(|index| format!("terminal-{index}"))
            .collect();

        for id in &ids {
            runner.test_insert_session_with_state(id, LaunchSessionState::Stopped);
            remember_terminal_session(&runner, id);
        }

        assert_eq!(
            runner.inner.terminal_sessions.lock().unwrap().len(),
            MAX_TERMINAL_SESSIONS
        );
        assert!(runner.get(&ids[0]).is_none());
        assert!(runner.get(&ids[total - 1]).is_some());
        assert_eq!(
            runner
                .list_sessions()
                .into_iter()
                .filter(|session| matches!(
                    session.state,
                    LaunchSessionState::Stopped | LaunchSessionState::Failed
                ))
                .count(),
            MAX_TERMINAL_SESSIONS
        );
    }

    #[test]
    fn active_session_is_never_pruned_by_terminal_history() {
        let runner = CommandRunnerState::new();
        let active_id = "active-session";
        runner.test_insert_session_with_state(active_id, LaunchSessionState::Running);

        for index in 0..(MAX_TERMINAL_SESSIONS + 8) {
            let id = format!("terminal-with-active-{index}");
            runner.test_insert_session_with_state(&id, LaunchSessionState::Stopped);
            remember_terminal_session(&runner, &id);
        }

        let active = runner.get(active_id).expect("active session must remain");
        assert_eq!(active.state, LaunchSessionState::Running);
        assert_eq!(runner.list_sessions().len(), MAX_TERMINAL_SESSIONS + 1);
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
    fn session_reaches_failed_after_nonzero_natural_exit() {
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test("profile-exit-failed", ".", "exit /b 7")
            .expect("start");
        let session = wait_until_terminal(
            &runner,
            &info.launch_session_id,
            Duration::from_secs(5),
        );
        assert_eq!(session.state, LaunchSessionState::Failed);
        assert_eq!(session.exit_code, Some(7));
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
        assert_eq!(final_info.state, LaunchSessionState::Stopped);
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
        assert_eq!(final_info.state, LaunchSessionState::Stopped);
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

    #[test]
    fn pid_fallback_rejects_identity_mismatch_without_terminating() {
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test("profile-identity-mismatch", ".", "ping -n 30 127.0.0.1")
            .expect("start");
        let pid = info.pid.expect("pid");
        std::thread::sleep(Duration::from_millis(200));
        TEST_SKIP_JOB_TERMINATE.store(true, Ordering::SeqCst);
        runner.test_set_direct_child_start_time(&info.launch_session_id, 1);
        runner.test_take_process(&info.launch_session_id);
        let err = runner
            .stop(&info.launch_session_id)
            .expect_err("mismatch must not terminate");
        TEST_SKIP_JOB_TERMINATE.store(false, Ordering::SeqCst);
        assert!(
            err.contains("LAUNCH_PROCESS_IDENTITY_MISMATCH"),
            "unexpected error: {err}"
        );
        assert!(
            is_pid_alive(pid),
            "direct child should remain alive after identity mismatch"
        );
        TEST_SKIP_JOB_TERMINATE.store(true, Ordering::SeqCst);
        runner.stop(&info.launch_session_id).expect_err("still mismatched");
        runner.test_set_direct_child_start_time(
            &info.launch_session_id,
            process_start_time_secs(pid).expect("live pid"),
        );
        TEST_SKIP_JOB_TERMINATE.store(false, Ordering::SeqCst);
        runner.stop(&info.launch_session_id).expect("valid identity stop");
        assert_pid_exits(pid, Duration::from_secs(3));
        wait_until_terminal(&runner, &info.launch_session_id, Duration::from_secs(5));
    }

    #[test]
    fn stop_ok_when_direct_child_already_exited_before_pid_fallback() {
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test("profile-exit-fallback", ".", "exit 0")
            .expect("start");
        std::thread::sleep(Duration::from_millis(150));
        TEST_SKIP_JOB_TERMINATE.store(true, Ordering::SeqCst);
        runner.test_take_process(&info.launch_session_id);
        runner
            .stop(&info.launch_session_id)
            .expect("stop should succeed when pid is already gone");
        TEST_SKIP_JOB_TERMINATE.store(false, Ordering::SeqCst);
        wait_until_terminal(&runner, &info.launch_session_id, Duration::from_secs(5));
    }

    #[test]
    fn stopping_stop_err_when_process_still_alive_and_terminate_fails() {
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test("profile-stopping-fail", ".", "ping -n 30 127.0.0.1")
            .expect("start");
        std::thread::sleep(Duration::from_millis(200));
        TEST_SKIP_JOB_TERMINATE.store(true, Ordering::SeqCst);
        runner.test_take_process(&info.launch_session_id);
        {
            let mut sessions = runner.inner.sessions.lock().unwrap();
            let session = sessions.get_mut(&info.launch_session_id).expect("session");
            session.info.state = LaunchSessionState::Stopping;
        }
        TEST_FORCE_TASKKILL_FAIL.store(true, Ordering::SeqCst);
        let err = runner
            .stop(&info.launch_session_id)
            .expect_err("live process with failed terminate must error");
        TEST_FORCE_TASKKILL_FAIL.store(false, Ordering::SeqCst);
        TEST_SKIP_JOB_TERMINATE.store(false, Ordering::SeqCst);
        assert!(
            err.contains("LAUNCH_STOP_FAILED"),
            "unexpected error: {err}"
        );
        assert!(
            is_pid_alive(info.pid.expect("pid")),
            "process should remain alive when terminate failed"
        );
        runner
            .stop(&info.launch_session_id)
            .expect("stop should succeed once taskkill works again");
        assert_pid_exits(info.pid.expect("pid"), Duration::from_secs(3));
        wait_until_terminal(&runner, &info.launch_session_id, Duration::from_secs(5));
    }

    #[cfg(windows)]
    fn node_on_path() -> bool {
        std::process::Command::new("cmd")
            .args(["/C", "where node >nul 2>&1"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(windows)]
    fn npm_on_path() -> bool {
        std::process::Command::new("cmd")
            .args(["/C", "where npm >nul 2>&1"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(windows)]
    fn collect_process_tree_pids(root_pid: u32) -> Vec<u32> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().without_exe(),
        );
        let mut pids = vec![root_pid];
        let mut changed = true;
        while changed {
            changed = false;
            for (pid, process) in system.processes() {
                let pid_u32 = pid.as_u32();
                if pids.contains(&pid_u32) {
                    continue;
                }
                if pids.contains(&process.parent().map(|p| p.as_u32()).unwrap_or(0)) {
                    pids.push(pid_u32);
                    changed = true;
                }
            }
        }
        pids
    }

    #[cfg(windows)]
    fn wait_for_descendant(
        root_pid: u32,
        name_contains: &str,
        timeout: Duration,
    ) -> Option<u32> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let mut system = System::new();
            system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().without_exe(),
            );
            let tree = collect_process_tree_pids(root_pid);
            for pid in tree {
                if let Some(process) = system.process(Pid::from_u32(pid)) {
                    let name = process.name().to_string_lossy().to_lowercase();
                    if name.contains(name_contains) {
                        return Some(pid);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    }

    #[test]
    #[cfg(windows)]
    fn natural_root_exit_closes_job_and_kills_remaining_descendants() {
        if !node_on_path() {
            eprintln!(
                "SKIP natural_root_exit_closes_job_and_kills_remaining_descendants: node not on PATH"
            );
            return;
        }
        let runner = CommandRunnerState::new();
        let unrelated = runner
            .start_for_test("profile-natural-unrelated", ".", "ping -n 120 127.0.0.1")
            .expect("unrelated ping");
        let unrelated_pid = unrelated.pid.expect("pid");
        std::thread::sleep(Duration::from_millis(200));

        // Avoid `[]` in the script — `cmd /C` misparses them. Node exits; ping stays in the Job.
        let spawn_and_exit = "node -e \"require('child_process').spawn('ping -n 120 127.0.0.1',{shell:true,stdio:'ignore'});setTimeout(function(){process.exit(0)},800)\"";
        let info = runner
            .start_for_test("profile-natural-job-close", ".", spawn_and_exit)
            .expect("start");
        let session_id = info.launch_session_id.clone();
        let root_pid = info.pid.expect("pid");

        let observe_deadline = Instant::now() + Duration::from_secs(15);
        let mut lingering_ping = None;
        while Instant::now() < observe_deadline {
            if let Some(ping_pid) = wait_for_descendant(root_pid, "ping", Duration::from_millis(100))
            {
                lingering_ping = Some(ping_pid);
                break;
            }
            if let Some(session) = runner.get(&session_id) {
                if matches!(
                    session.state,
                    LaunchSessionState::Stopped | LaunchSessionState::Failed
                ) {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let lingering_ping =
            lingering_ping.expect("spawned ping should appear under session root before root exits");

        let terminal = wait_until_terminal(&runner, &session_id, Duration::from_secs(15));
        assert_eq!(terminal.state, LaunchSessionState::Stopped);

        let session_after = runner.get(&session_id).expect("terminal session still queryable");
        assert_eq!(session_after.state, LaunchSessionState::Stopped);
        assert_eq!(session_after.launch_session_id, session_id);

        assert_pid_exits(lingering_ping, Duration::from_secs(5));
        assert!(
            is_pid_alive(unrelated_pid),
            "unrelated session ping must survive other session job close"
        );

        runner.stop(&unrelated.launch_session_id).expect("cleanup");
        assert_pid_exits(unrelated_pid, Duration::from_secs(5));
        wait_until_terminal(&runner, &unrelated.launch_session_id, Duration::from_secs(5));
    }

    #[test]
    #[cfg(windows)]
    fn stop_session_kills_node_child() {
        if !node_on_path() {
            eprintln!("SKIP stop_session_kills_node_child: node not on PATH");
            return;
        }
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test(
                "profile-node-stop",
                ".",
                "node -e \"setInterval(()=>{}, 1e6)\"",
            )
            .expect("start");
        let root_pid = info.pid.expect("pid");
        let node_pid = wait_for_descendant(root_pid, "node", Duration::from_secs(10))
            .expect("node descendant should appear");
        std::thread::sleep(Duration::from_millis(300));
        runner.stop(&info.launch_session_id).expect("stop");
        assert_pid_exits(node_pid, Duration::from_secs(5));
        assert_pid_exits(root_pid, Duration::from_secs(5));
        wait_until_terminal(&runner, &info.launch_session_id, Duration::from_secs(5));
    }

    #[test]
    #[cfg(windows)]
    fn stop_session_kills_npm_node_chain() {
        if !npm_on_path() || !node_on_path() {
            eprintln!("SKIP stop_session_kills_npm_node_chain: npm/node not on PATH");
            return;
        }
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test(
                "profile-npm-stop",
                ".",
                "npm exec --yes -- node -e \"setInterval(()=>{}, 1e6)\"",
            )
            .expect("start");
        let root_pid = info.pid.expect("pid");
        let node_pid = wait_for_descendant(root_pid, "node", Duration::from_secs(20))
            .expect("npm→node chain should produce node descendant");
        std::thread::sleep(Duration::from_millis(500));
        runner.stop(&info.launch_session_id).expect("stop");
        assert_pid_exits(node_pid, Duration::from_secs(8));
        assert_pid_exits(root_pid, Duration::from_secs(8));
        wait_until_terminal(&runner, &info.launch_session_id, Duration::from_secs(8));
    }

    #[test]
    #[cfg(windows)]
    fn job_all_observed_descendants_in_job() {
        if !node_on_path() {
            eprintln!("SKIP job_all_observed_descendants_in_job: node not on PATH");
            return;
        }
        let runner = CommandRunnerState::new();
        let info = runner
            .start_for_test(
                "profile-job-membership",
                ".",
                "node -e \"setInterval(()=>{}, 1e6)\"",
            )
            .expect("start");
        let root_pid = info.pid.expect("pid");
        let _node_pid = wait_for_descendant(root_pid, "node", Duration::from_secs(10))
            .expect("node descendant");
        std::thread::sleep(Duration::from_millis(300));
        let job = runner.test_session_job_handle(&info.launch_session_id);
        let tree = collect_process_tree_pids(root_pid);
        for pid in tree {
            assert!(
                windows_job::is_pid_in_job(pid, job),
                "pid {} should belong to session job",
                pid
            );
        }
        runner.stop(&info.launch_session_id).expect("stop");
        wait_until_terminal(&runner, &info.launch_session_id, Duration::from_secs(5));
    }

    #[test]
    #[cfg(windows)]
    fn unrelated_process_survives_session_stop() {
        let runner = CommandRunnerState::new();
        let unrelated = runner
            .start_for_test("profile-unrelated-ping", ".", "ping -n 30 127.0.0.1")
            .expect("unrelated ping");
        let unrelated_pid = unrelated.pid.expect("pid");
        std::thread::sleep(Duration::from_millis(200));

        let session = runner
            .start_for_test("profile-session-stop", ".", "ping -n 30 127.0.0.1")
            .expect("session ping");
        let session_pid = session.pid.expect("pid");
        std::thread::sleep(Duration::from_millis(200));

        runner.stop(&session.launch_session_id).expect("stop session");
        assert_pid_exits(session_pid, Duration::from_secs(5));
        assert!(
            is_pid_alive(unrelated_pid),
            "unrelated ping must survive session stop"
        );
        runner.stop(&unrelated.launch_session_id).expect("cleanup");
        assert_pid_exits(unrelated_pid, Duration::from_secs(5));
        wait_until_terminal(&runner, &session.launch_session_id, Duration::from_secs(5));
        wait_until_terminal(&runner, &unrelated.launch_session_id, Duration::from_secs(5));
    }

    #[test]
    #[cfg(windows)]
    fn port_released_after_stop() {
        if !node_on_path() {
            eprintln!("SKIP port_released_after_stop: node not on PATH");
            return;
        }
        use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo};
        use std::net::TcpListener;

        fn port_is_listening(port: u16) -> bool {
            let sockets = get_sockets_info(
                AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
                ProtocolFlags::TCP,
            )
            .unwrap_or_default();
            sockets.iter().any(|sock| {
                matches!(
                    &sock.protocol_socket_info,
                    ProtocolSocketInfo::Tcp(t) if t.local_port == port
                )
            })
        }

        let port = 37655u16;
        let runner = CommandRunnerState::new();
        let command = format!(
            "node -e \"require('http').createServer((q,s)=>s.end('ok')).listen({port},'127.0.0.1')\""
        );
        let info = runner
            .start_for_test("profile-port-release", ".", &command)
            .expect("start");
        let root_pid = info.pid.expect("pid");
        let _node_pid = wait_for_descendant(root_pid, "node", Duration::from_secs(10))
            .expect("node server");

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if port_is_listening(port) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(port_is_listening(port), "port should be bound before stop");

        runner.stop(&info.launch_session_id).expect("stop");
        wait_until_terminal(&runner, &info.launch_session_id, Duration::from_secs(8));

        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if !port_is_listening(port) && TcpListener::bind(("127.0.0.1", port)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("port {} not released after stop", port);
    }
}
