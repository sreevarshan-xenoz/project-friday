//! FRIDAY OS root helper service and D-Bus interface.
//!
//! Provides `org.friday.Helper1` over the system D-Bus, delegating to `Helper`.
//! Guarantees that every mutating method is authorized by PolicyKit and uses fixed
//! execution arguments.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::auth::{Authorizer, PolkitAuthorizer, ACTION_INSTALL, ACTION_REMOVE, ACTION_UPDATE};
use crate::caller::{allowed_from_env, CallerCheck, DbusCallers};
use crate::jobs::{JobState, JobTable};
use crate::runner::{
    check_pkg_argv, install_argv, remove_argv, update_argv, Runner, SystemRunner, TIMEOUT_INSTALL,
    TIMEOUT_QUERY, TIMEOUT_REMOVE, TIMEOUT_UPDATE,
};
use crate::validator::{is_protected_package, valid_package_name};

/// Core business logic for the FRIDAY OS root helper.
pub struct Helper {
    callers: Arc<dyn CallerCheck>,
    authorizer: Arc<dyn Authorizer>,
    runner: Arc<dyn Runner>,
    jobs: Arc<Mutex<JobTable>>,
    next_id: AtomicU64,
}

impl Helper {
    /// Creates a new `Helper` with custom authorizer and runner implementations.
    pub fn new(
        authorizer: Arc<dyn Authorizer>,
        runner: Arc<dyn Runner>,
        callers: Arc<dyn CallerCheck>,
    ) -> Self {
        Self {
            callers,
            authorizer,
            runner,
            jobs: Arc::new(Mutex::new(JobTable::new())),
            next_id: AtomicU64::new(1),
        }
    }

    /// Creates a `Helper` configured for production on the system bus.
    pub async fn new_system() -> Result<Self, zbus::Error> {
        let conn = zbus::connection::Connection::system().await?;
        let allowed = allowed_from_env(std::env::var("FRIDAY_HELPER_ALLOW_EXE").ok().as_deref());
        let callers = Arc::new(DbusCallers::new(conn.clone(), allowed));
        let authorizer = Arc::new(PolkitAuthorizer::new(conn));
        let runner = Arc::new(SystemRunner);
        Ok(Self::new(authorizer, runner, callers))
    }

    /// Generates a sequential job ID.
    fn allocate_job_id(&self) -> String {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("job-{now}-{seq}")
    }

    /// Returns general status without requiring authorization.
    pub fn status(&self) -> HashMap<String, zbus::zvariant::Value<'static>> {
        let mut map = HashMap::new();
        let table = self.jobs.lock().unwrap();
        let is_running = table.is_running();
        map.insert(
            "version".to_string(),
            zbus::zvariant::Value::from(env!("CARGO_PKG_VERSION")),
        );
        map.insert(
            "running".to_string(),
            zbus::zvariant::Value::from(is_running),
        );
        map.insert(
            "jobs_count".to_string(),
            zbus::zvariant::Value::from(table.len() as u32),
        );
        map
    }

    /// Queries the status and log tail of a job without requiring authorization.
    pub fn job(&self, id: &str) -> Result<(String, String), String> {
        let table = self.jobs.lock().unwrap();
        match table.get(id) {
            Some(j) => Ok((j.state.as_str().to_string(), j.log.tail())),
            None => Err(format!("job \"{id}\" not found")),
        }
    }

    /// Triggers a full system update (`pacman -Syu --noconfirm`).
    pub async fn update(&self, sender: &str) -> Result<String, String> {
        self.callers.check(sender).await?;
        let auth_result = self
            .authorizer
            .check(sender, ACTION_UPDATE, None)
            .await
            .map_err(|e| format!("authorizer error: {e}"))?;

        if !auth_result {
            return Err("authorization denied".into());
        }

        let job_id = {
            let mut table = self.jobs.lock().unwrap();
            if table.is_running() {
                return Err("another job is running".into());
            }
            let id = self.allocate_job_id();
            table.add_job(id.clone())?;
            id
        };

        let jobs = self.jobs.clone();
        let runner = self.runner.clone();
        let jid = job_id.clone();
        tokio::spawn(async move {
            Self::execute_command(jobs, runner, jid, update_argv(), TIMEOUT_UPDATE).await;
        });

        Ok(job_id)
    }

    /// Triggers a package installation (`pacman -S --needed --noconfirm -- <name>`).
    pub async fn install(&self, sender: &str, name: &str) -> Result<String, String> {
        if !valid_package_name(name) {
            return Err(format!("invalid package name \"{name}\""));
        }

        self.callers.check(sender).await?;
        let auth_result = self
            .authorizer
            .check(sender, ACTION_INSTALL, Some(name))
            .await
            .map_err(|e| format!("authorizer error: {e}"))?;

        if !auth_result {
            return Err("authorization denied".into());
        }

        {
            let table = self.jobs.lock().unwrap();
            if table.is_running() {
                return Err("another job is running".into());
            }
        }

        // Verify that the package exists in the repositories before launching the install job
        let check_argv = check_pkg_argv(name);
        let mut check_sink = |_line: &str| {};
        let check_status = self
            .runner
            .run(&check_argv, TIMEOUT_QUERY, &mut check_sink)
            .await
            .map_err(|e| format!("repo query failed: {e}"))?;

        if check_status != 0 {
            return Err(format!("package \"{name}\" not found in repositories"));
        }

        let job_id = {
            let mut table = self.jobs.lock().unwrap();
            if table.is_running() {
                return Err("another job is running".into());
            }
            let id = self.allocate_job_id();
            table.add_job(id.clone())?;
            id
        };

        let jobs = self.jobs.clone();
        let runner = self.runner.clone();
        let jid = job_id.clone();
        let argv = install_argv(name);
        tokio::spawn(async move {
            Self::execute_command(jobs, runner, jid, argv, TIMEOUT_INSTALL).await;
        });

        Ok(job_id)
    }

    /// Triggers package removal (`pacman -R --noconfirm -- <name>`).
    pub async fn remove(&self, sender: &str, name: &str) -> Result<String, String> {
        if !valid_package_name(name) {
            return Err(format!("invalid package name \"{name}\""));
        }

        if is_protected_package(name) {
            return Err(format!(
                "package \"{name}\" is protected and cannot be removed"
            ));
        }

        self.callers.check(sender).await?;
        let auth_result = self
            .authorizer
            .check(sender, ACTION_REMOVE, Some(name))
            .await
            .map_err(|e| format!("authorizer error: {e}"))?;

        if !auth_result {
            return Err("authorization denied".into());
        }

        let job_id = {
            let mut table = self.jobs.lock().unwrap();
            if table.is_running() {
                return Err("another job is running".into());
            }
            let id = self.allocate_job_id();
            table.add_job(id.clone())?;
            id
        };

        let jobs = self.jobs.clone();
        let runner = self.runner.clone();
        let jid = job_id.clone();
        let argv = remove_argv(name);
        tokio::spawn(async move {
            Self::execute_command(jobs, runner, jid, argv, TIMEOUT_REMOVE).await;
        });

        Ok(job_id)
    }

    /// Executes a job command asynchronously, streaming log output and updating state.
    async fn execute_command(
        jobs: Arc<Mutex<JobTable>>,
        runner: Arc<dyn Runner>,
        job_id: String,
        argv: Vec<String>,
        timeout: Duration,
    ) {
        {
            let mut table = jobs.lock().unwrap();
            table.set_state(&job_id, JobState::Running);
        }

        let jobs_for_line = jobs.clone();
        let jid_for_line = job_id.clone();
        let mut on_line = move |line: &str| {
            if let Ok(mut table) = jobs_for_line.lock() {
                table.add_log_line(&jid_for_line, line);
            }
        };

        let run_result = runner.run(&argv, timeout, &mut on_line).await;

        let final_state = match run_result {
            Ok(0) => JobState::Done,
            Ok(exit_code) => {
                if let Ok(mut table) = jobs.lock() {
                    table.add_log_line(&job_id, &format!("process exited with code {exit_code}"));
                }
                JobState::Failed
            }
            Err(err) => {
                if let Ok(mut table) = jobs.lock() {
                    table.add_log_line(&job_id, &format!("execution error: {err}"));
                }
                JobState::Failed
            }
        };

        if let Ok(mut table) = jobs.lock() {
            table.set_state(&job_id, final_state);
        }
    }
}

/// D-Bus interface wrapper for `org.friday.Helper1`.
pub struct HelperIface {
    helper: Arc<Helper>,
}

impl HelperIface {
    pub fn new(helper: Arc<Helper>) -> Self {
        Self { helper }
    }
}

#[zbus::interface(name = "org.friday.Helper1")]
impl HelperIface {
    /// Queries service status (version string, running state).
    #[zbus(name = "GetStatus")]
    async fn get_status(
        &self,
    ) -> zbus::fdo::Result<HashMap<String, zbus::zvariant::Value<'static>>> {
        Ok(self.helper.status())
    }

    /// Initiates a system update (`pacman -Syu`).
    #[zbus(name = "UpdateSystem")]
    async fn update_system(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = header
            .sender()
            .map(|s| s.as_str().to_string())
            .ok_or_else(|| zbus::fdo::Error::Failed("missing sender in message header".into()))?;

        self.helper
            .update(&sender)
            .await
            .map_err(zbus::fdo::Error::Failed)
    }

    /// Installs a package (`pacman -S --needed`).
    #[zbus(name = "InstallPackage")]
    async fn install_package(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        name: String,
    ) -> zbus::fdo::Result<String> {
        let sender = header
            .sender()
            .map(|s| s.as_str().to_string())
            .ok_or_else(|| zbus::fdo::Error::Failed("missing sender in message header".into()))?;

        self.helper
            .install(&sender, &name)
            .await
            .map_err(zbus::fdo::Error::Failed)
    }

    /// Removes a package (`pacman -R`).
    #[zbus(name = "RemovePackage")]
    async fn remove_package(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        name: String,
    ) -> zbus::fdo::Result<String> {
        let sender = header
            .sender()
            .map(|s| s.as_str().to_string())
            .ok_or_else(|| zbus::fdo::Error::Failed("missing sender in message header".into()))?;

        self.helper
            .remove(&sender, &name)
            .await
            .map_err(zbus::fdo::Error::Failed)
    }

    /// Retrieves job state and output log tail.
    #[zbus(name = "JobStatus")]
    async fn job_status(&self, job_id: String) -> zbus::fdo::Result<(String, String)> {
        self.helper.job(&job_id).map_err(zbus::fdo::Error::Failed)
    }
}

/// Runs the root helper daemon, serving `org.friday.Helper1` on the system bus.
pub async fn run_daemon() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    tracing::info!("starting friday-os-helper");

    let helper = Arc::new(Helper::new_system().await?);
    let iface = HelperIface::new(helper);

    let _conn = zbus::connection::Builder::system()?
        .name("org.friday.Helper1")?
        .serve_at("/org/friday/Helper1", iface)?
        .build()
        .await?;

    tracing::info!("friday-os-helper serving on org.friday.Helper1");
    std::future::pending::<()>().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::tests::StubAuthorizer;
    use crate::caller::tests::StubCallers;
    use crate::runner::tests::StubRunner;
    use tokio::sync::oneshot;

    impl Helper {
        /// Helper for tests to wait for an asynchronous job to reach completion.
        pub async fn wait_for_job(&self, job_id: &str) -> Result<JobState, String> {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let (state, _) = self.job(job_id)?;
                if state != "queued" && state != "running" {
                    return Ok(match state.as_str() {
                        "done" => JobState::Done,
                        "failed" => JobState::Failed,
                        _ => JobState::Queued,
                    });
                }
                if std::time::Instant::now() > deadline {
                    return Err("timed out waiting for job to complete".into());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }

    #[tokio::test]
    async fn test_get_status_without_authorization() {
        let auth = StubAuthorizer::new(false); // even if denied, status works
        let runner = StubRunner::new(&[]);
        let helper = Helper::new(auth.clone(), runner, StubCallers::allow());

        let status = helper.status();
        assert_eq!(auth.call_count(), 0); // No authorization check performed
        assert!(status.contains_key("version"));
        assert!(status.contains_key("running"));
        assert_eq!(
            status.get("running").unwrap().to_owned(),
            zbus::zvariant::Value::from(false).to_owned()
        );
    }

    #[tokio::test]
    async fn test_unauthorized_caller() {
        let auth = StubAuthorizer::new(false);
        let runner = StubRunner::new(&["htop"]);
        let helper = Helper::new(auth.clone(), runner.clone(), StubCallers::allow());

        let res_install = helper.install(":1.100", "htop").await;
        assert!(res_install.is_err());
        assert_eq!(res_install.unwrap_err(), "authorization denied");
        assert_eq!(runner.call_count(), 0);

        let res_update = helper.update(":1.100").await;
        assert!(res_update.is_err());
        assert_eq!(res_update.unwrap_err(), "authorization denied");
        assert_eq!(runner.call_count(), 0);

        let res_remove = helper.remove(":1.100", "htop").await;
        assert!(res_remove.is_err());
        assert_eq!(res_remove.unwrap_err(), "authorization denied");
        assert_eq!(runner.call_count(), 0);
    }

    #[tokio::test]
    async fn test_authorizer_error_denied() {
        let auth = StubAuthorizer::failing();
        let runner = StubRunner::new(&["htop"]);
        let helper = Helper::new(auth.clone(), runner.clone(), StubCallers::allow());

        let res = helper.install(":1.100", "htop").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("authorizer error"));
        assert_eq!(runner.call_count(), 0);
    }

    #[tokio::test]
    async fn test_protected_removal_refused() {
        let auth = StubAuthorizer::new(true);
        let runner = StubRunner::new(&["htop"]);
        let helper = Helper::new(auth, runner.clone(), StubCallers::allow());

        let protected_cases = [
            "base",
            "linux",
            "linux-lts",
            "systemd",
            "systemd-libs",
            "pacman",
            "friday-git",
            "polkit",
            "linux-firmware",
            "friday-os-helper",
            "systemd-boot",
        ];

        for pkg in protected_cases {
            let res = helper.remove(":1.100", pkg).await;
            assert!(
                res.is_err(),
                "removal of protected package '{pkg}' should be refused"
            );
            assert!(res
                .unwrap_err()
                .contains("is protected and cannot be removed"));
        }

        // Allowed package
        let res_htop = helper.remove(":1.100", "htop").await;
        assert!(res_htop.is_ok());
    }

    #[tokio::test]
    async fn test_authorized_install_argv() {
        let auth = StubAuthorizer::new(true);
        let runner = StubRunner::new(&["htop"]);
        let helper = Helper::new(auth, runner.clone(), StubCallers::allow());

        let job_id = helper.install(":1.100", "htop").await.unwrap();
        let final_state = helper.wait_for_job(&job_id).await.unwrap();
        assert_eq!(final_state, JobState::Done);

        let calls = runner.calls();
        assert_eq!(calls.len(), 2);
        // Call 0 is the check_pkg_argv
        assert_eq!(calls[0], vec!["-Si", "--", "htop"]);
        // Call 1 is the fixed install argv with "--" before name
        assert_eq!(
            calls[1],
            vec!["-S", "--needed", "--noconfirm", "--", "htop"]
        );
    }

    #[tokio::test]
    async fn test_remove_argv_never_rdd_rns() {
        let auth = StubAuthorizer::new(true);
        let runner = StubRunner::new(&["htop"]);
        let helper = Helper::new(auth, runner.clone(), StubCallers::allow());

        let job_id = helper.remove(":1.100", "htop").await.unwrap();
        let final_state = helper.wait_for_job(&job_id).await.unwrap();
        assert_eq!(final_state, JobState::Done);

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], vec!["-R", "--noconfirm", "--", "htop"]);
        assert!(!calls[0].contains(&"-Rdd".to_string()));
        assert!(!calls[0].contains(&"-Rns".to_string()));
    }

    #[tokio::test]
    async fn test_install_name_not_in_repos() {
        let auth = StubAuthorizer::new(true);
        // "htop" exists in repos, "nonexistent" does not
        let runner = StubRunner::new(&["htop"]);
        let helper = Helper::new(auth, runner.clone(), StubCallers::allow());

        let res = helper.install(":1.100", "nonexistent").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("not found in repositories"));

        // Only the check was performed; no install command was run
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], vec!["-Si", "--", "nonexistent"]);
    }

    #[tokio::test]
    async fn test_second_job_while_one_runs() {
        let (tx, rx) = oneshot::channel();
        let auth = StubAuthorizer::new(true);
        let runner = StubRunner::with_pause(&["htop"], rx);
        let helper = Helper::new(auth, runner, StubCallers::allow());

        let job1 = helper.update(":1.100").await.unwrap();

        // While job 1 is paused / running, a second job request fails
        let res2 = helper.update(":1.100").await;
        assert!(res2.is_err());
        assert_eq!(res2.unwrap_err(), "another job is running");

        // Allow job 1 to resume and finish
        let _ = tx.send(());
        let final_state = helper.wait_for_job(&job1).await.unwrap();
        assert_eq!(final_state, JobState::Done);
    }

    #[tokio::test]
    async fn test_job_log_tail_cap() {
        let mut lines = Vec::new();
        for i in 1..=250 {
            lines.push(format!("output line {i}"));
        }
        let auth = StubAuthorizer::new(true);
        let runner = StubRunner::with_output(&["htop"], lines);
        let helper = Helper::new(auth, runner, StubCallers::allow());

        let job_id = helper.update(":1.100").await.unwrap();
        let _ = helper.wait_for_job(&job_id).await.unwrap();

        let (state, log_tail) = helper.job(&job_id).unwrap();
        assert_eq!(state, "done");
        let tail_lines: Vec<&str> = log_tail.lines().collect();
        assert_eq!(tail_lines.len(), 200);
        assert_eq!(tail_lines.first().copied(), Some("output line 51"));
        assert_eq!(tail_lines.last().copied(), Some("output line 250"));
    }

    #[tokio::test]
    async fn test_job_table_keeps_last_20() {
        let auth = StubAuthorizer::new(true);
        let runner = StubRunner::new(&["htop"]);
        let helper = Helper::new(auth, runner, StubCallers::allow());

        let mut job_ids = Vec::new();
        for _ in 1..=25 {
            let jid = helper.update(":1.100").await.unwrap();
            let _ = helper.wait_for_job(&jid).await.unwrap();
            job_ids.push(jid);
        }

        // Oldest jobs (first 5) dropped
        for old_id in &job_ids[0..5] {
            assert!(helper.job(old_id).is_err());
        }

        // Newest jobs (last 20) present
        for new_id in &job_ids[5..25] {
            assert!(helper.job(new_id).is_ok());
        }
    }

    #[tokio::test]
    async fn test_timeout_path_marks_job_failed() {
        let auth = StubAuthorizer::new(true);
        let runner = StubRunner::timing_out();
        let helper = Helper::new(auth, runner, StubCallers::allow());

        let job_id = helper.update(":1.100").await.unwrap();
        let final_state = helper.wait_for_job(&job_id).await.unwrap();
        assert_eq!(final_state, JobState::Failed);

        let (state, log) = helper.job(&job_id).unwrap();
        assert_eq!(state, "failed");
        assert!(log.contains("timed out"));
    }

    #[tokio::test]
    async fn a_caller_that_is_not_friday_is_refused_before_polkit_is_asked() {
        let auth = StubAuthorizer::new(true);
        let runner = StubRunner::new(&[]);
        let callers = StubCallers::deny();
        let helper = Helper::new(auth.clone(), runner.clone(), callers.clone());
        for result in [
            helper.update(":1.9").await,
            helper.install(":1.9", "htop").await,
            helper.remove(":1.9", "htop").await,
        ] {
            assert!(result.unwrap_err().contains("only FRIDAY itself"));
        }
        assert_eq!(callers.asked(), 3);
        assert_eq!(
            auth.call_count(),
            0,
            "polkit was never asked on behalf of a stranger"
        );
        assert_eq!(runner.call_count(), 0);
    }

    #[tokio::test]
    async fn the_prompt_is_told_which_package_it_is_about() {
        let auth = StubAuthorizer::new(true);
        let runner = StubRunner::new(&["htop"]);
        let helper = Helper::new(auth.clone(), runner, StubCallers::allow());
        let id = helper.install(":1.9", "htop").await.unwrap();
        helper.wait_for_job(&id).await.unwrap();
        let id = helper.remove(":1.9", "htop").await.unwrap();
        helper.wait_for_job(&id).await.unwrap();
        let id = helper.update(":1.9").await.unwrap();
        helper.wait_for_job(&id).await.unwrap();
        assert_eq!(
            auth.packages(),
            vec![Some("htop".to_string()), Some("htop".to_string()), None]
        );
    }

    #[tokio::test]
    async fn status_needs_no_caller_check() {
        let callers = StubCallers::deny();
        let helper = Helper::new(
            StubAuthorizer::new(false),
            StubRunner::new(&[]),
            callers.clone(),
        );
        let _ = helper.status();
        assert_eq!(callers.asked(), 0);
    }
}
