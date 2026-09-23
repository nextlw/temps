// SPDX-FileCopyrightText: 2024-2026 Temps Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Host-side spawner for a build.
//!
//! # Why the build runs here and not inside the workspace
//!
//! A developer's workspace is a microVM sized for editing and running their
//! own code. A build is the opposite workload: minutes of every core and heavy
//! disk, for a result nobody interacts with. Putting one inside the other
//! makes the developer wait on a machine chosen for their editor, and makes a
//! per-workspace disk quota into a build failure. So the build runs on the
//! host, with its working directory in the project's directory — the same
//! directory the workspace sees, which is what lets the two exchange work
//! without either reaching into the other.
//!
//! # What this module is responsible for, and what it is not
//!
//! It is responsible for the shape of the child process: a cleared
//! environment, its own process group, a wall-clock ceiling, and a result read
//! only from tagged output. It is **not** responsible for deciding whether the
//! caller may run this build — that is settled before anything is spawned,
//! because a check made after a credential is in a child's environment is not
//! a check.
//!
//! # The environment is the security boundary
//!
//! A build runs third-party code by definition: a dependency tree's install
//! scripts run with the build. So the environment starts empty
//! ([`std::process::Command::env_clear`]) and is filled back one name at a
//! time. Nothing from the server's environment reaches the child unless it was
//! named here.
//!
//! Credentials are [`BuildInvocation::credentials`], chosen by the caller per
//! environment, and the intended use is to pass **none** for the build step
//! itself. Cloning a repository and pushing an image are separate spawns with
//! their own, narrower environments. That is what keeps a malicious dependency
//! from reaching a registry token: the token was never in the process that ran
//! it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use temps_build_protocol::{BuildRequest, BuildResultEnvelope, ENVELOPE_SCHEMA};

/// Bytes of captured output kept for an error message.
///
/// Bounded on purpose: a build that fails after printing a gigabyte of
/// progress should still produce an error a human can read, and should not
/// decide how much memory the server spends reporting it.
const OUTPUT_TAIL_BYTES: usize = 8 * 1024;

/// Everything the host decides about one child process.
///
/// The [`BuildRequest`] says what to build. Every field here says something the
/// request is deliberately not allowed to influence: which program runs, where
/// it runs, what it can reach, and how long it may take.
pub struct BuildInvocation<'a> {
    pub request: &'a BuildRequest,

    /// The build program. Chosen by the host, never named by the request — a
    /// request that could pick its own executable would be a request that can
    /// run anything.
    pub program: PathBuf,

    /// Working directory: the project's host-side directory.
    pub working_dir: PathBuf,

    /// `PATH` for the child. Explicit because the environment is cleared, and
    /// inheriting the server's `PATH` would quietly re-open it.
    pub path: String,

    /// `HOME` for the child. Build tools that write caches or configuration
    /// need one, and letting them default to the server's would share state
    /// between projects.
    pub home: PathBuf,

    /// What this environment is allowed to hand this child, by name.
    ///
    /// Pass an empty map for the build step. A credential here is reachable by
    /// every install script in the dependency tree.
    pub credentials: BTreeMap<String, String>,

    /// Wall-clock ceiling. On expiry the whole process group is killed, not
    /// just the child: a build spawns compilers and package managers that
    /// outlive it otherwise.
    pub wall_timeout: Duration,
}

/// Why a build produced no envelope.
#[derive(Debug, thiserror::Error)]
pub enum BuildExecError {
    #[error("failed to spawn build program {program}: {reason}")]
    Spawn { program: String, reason: String },

    #[error("build {build_id} exceeded its wall-clock ceiling of {timeout_secs}s and its process group was killed")]
    Timeout {
        build_id: String,
        timeout_secs: u64,
    },

    #[error("build {build_id} exited with status {status}; last output: {output_tail}")]
    NonZeroExit {
        build_id: String,
        status: i32,
        output_tail: String,
    },

    #[error("build {build_id} exited without a status, which on unix means it was killed by a signal; last output: {output_tail}")]
    NoExitStatus {
        build_id: String,
        output_tail: String,
    },

    #[error("build {build_id} succeeded but printed no result envelope; its stdout is the program's, so an untagged line is output, not a result; last output: {output_tail}")]
    NoEnvelope {
        build_id: String,
        output_tail: String,
    },

    #[error("build {build_id} reported schema {found}, this host understands {expected}; refusing to guess at a shape it was not written for")]
    SchemaMismatch {
        build_id: String,
        expected: String,
        found: String,
    },

    #[error("failed to wait on build {build_id}: {reason}")]
    Wait { build_id: String, reason: String },
}

/// Run one build to completion and return its envelope.
///
/// Authorization is the caller's, and must already have happened.
pub async fn run_build(inv: BuildInvocation<'_>) -> Result<BuildResultEnvelope, BuildExecError> {
    let build_id = inv.request.build_id.to_string();

    let mut cmd = tokio::process::Command::new(&inv.program);
    cmd.current_dir(&inv.working_dir)
        // Empty first. Every name below is one the host chose to grant.
        .env_clear()
        .env("PATH", &inv.path)
        .env("HOME", &inv.home)
        .env("TEMPS_BUILD_ID", &build_id)
        .env("TEMPS_BUILD_SCHEMA", ENVELOPE_SCHEMA)
        // The child reads its own work from stdin-free, structured input on
        // argv-adjacent env rather than inheriting ambient state.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (name, value) in &inv.credentials {
        cmd.env(name, value);
    }

    #[cfg(unix)]
    {
        // Its own group, so the timeout reaches the compilers and package
        // managers the build spawned, not only the build.
        cmd.process_group(0);
    }

    let child = cmd.spawn().map_err(|e| BuildExecError::Spawn {
        program: inv.program.display().to_string(),
        reason: e.to_string(),
    })?;

    #[cfg(unix)]
    let group = child.id().and_then(|pid| {
        i32::try_from(pid)
            .ok()
            .map(nix::unistd::Pid::from_raw)
    });

    let output = match tokio::time::timeout(inv.wall_timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(BuildExecError::Wait {
                build_id,
                reason: e.to_string(),
            })
        }
        Err(_) => {
            #[cfg(unix)]
            if let Some(group) = group {
                // Signalling by group number is only safe while the child has
                // not been reaped; after that the number can belong to someone
                // else. `wait_with_output` has not returned, so it has not.
                let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL);
            }
            return Err(BuildExecError::Timeout {
                build_id,
                timeout_secs: inv.wall_timeout.as_secs(),
            });
        }
    };

    let stdout_tail = tail(&output.stdout);
    let stderr_tail = tail(&output.stderr);

    match output.status.code() {
        Some(0) => {}
        Some(status) => {
            return Err(BuildExecError::NonZeroExit {
                build_id,
                status,
                output_tail: stderr_tail,
            })
        }
        None => {
            return Err(BuildExecError::NoExitStatus {
                build_id,
                output_tail: stderr_tail,
            })
        }
    }

    let envelope = parse_envelope(&output.stdout).ok_or(BuildExecError::NoEnvelope {
        build_id: build_id.clone(),
        output_tail: stdout_tail,
    })?;

    if !envelope.schema_matches() {
        return Err(BuildExecError::SchemaMismatch {
            build_id,
            expected: ENVELOPE_SCHEMA.to_string(),
            found: envelope.schema,
        });
    }

    Ok(envelope)
}

/// Read the last line of stdout as an envelope.
///
/// The last line, not the whole stream: everything before it belongs to the
/// build — compiler output, a package manager's progress — and a build has no
/// obligation to be quiet. Returning `None` for anything unparseable is the
/// point; a caller that wants to know what the build printed reads the output,
/// not this.
#[must_use]
pub fn parse_envelope(stdout: &[u8]) -> Option<BuildResultEnvelope> {
    let text = std::str::from_utf8(stdout).ok()?;
    let last = text.lines().rev().find(|l| !l.trim().is_empty())?;
    serde_json::from_str(last.trim()).ok()
}

/// Last [`OUTPUT_TAIL_BYTES`] of captured output, for an error message.
fn tail(data: &[u8]) -> String {
    let start = data.len().saturating_sub(OUTPUT_TAIL_BYTES);
    String::from_utf8_lossy(&data[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use temps_build_protocol::{
        Arch, BuildBudget, BuildContext, BuildRecipe, BuildTarget, CacheScope, Os, Priority,
        Requester,
    };

    /// A request is required to invoke the executor, but nothing in it steers
    /// these tests: what is being exercised is the shape of the child process,
    /// which the request is deliberately not allowed to influence.
    fn a_request() -> BuildRequest {
        BuildRequest {
            build_id: uuid::Uuid::nil(),
            context: BuildContext::Archive {
                upload_id: uuid::Uuid::nil(),
                digest: "sha256:0".to_string(),
            },
            recipe: BuildRecipe::Native {
                command: vec!["true".to_string()],
                env: BTreeMap::new(),
                working_dir: None,
            },
            target: BuildTarget {
                os: Os::Linux,
                arch: Arch::Amd64,
                capabilities: vec![],
            },
            budget: BuildBudget {
                timeout_secs: 5,
                cpu_limit_micros: None,
                memory_limit_bytes: None,
                priority: Priority::Development,
            },
            requester: Requester {
                user_id: None,
                project_id: uuid::Uuid::nil(),
                environment_id: Some(1),
            },
            outputs: vec![],
            cache: CacheScope {
                project_id: uuid::Uuid::nil(),
                read: true,
            },
        }
    }

    /// Runs `script` through `/bin/sh` with the given credentials and ceiling.
    async fn run_script(
        script: &str,
        credentials: BTreeMap<String, String>,
        timeout: Duration,
    ) -> Result<BuildResultEnvelope, BuildExecError> {
        let request = a_request();
        let dir = std::env::temp_dir();
        let mut program = PathBuf::from("/bin/sh");
        if !program.exists() {
            program = PathBuf::from("/usr/bin/sh");
        }
        let mut cmd_inv = BuildInvocation {
            request: &request,
            program,
            working_dir: dir.clone(),
            path: "/usr/bin:/bin".to_string(),
            home: dir,
            credentials,
            wall_timeout: timeout,
        };
        // `/bin/sh -c <script>` is the shape every case here needs; the
        // executor takes a program, so the script rides as an argument via a
        // wrapper file to keep the executor's surface honest.
        let script_file = std::env::temp_dir().join(format!(
            "temps-build-exec-test-{}.sh",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&script_file, script).expect("the test writes its own script");
        cmd_inv.program = script_file.clone();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_file, std::fs::Permissions::from_mode(0o700))
                .expect("the test script is executable");
        }
        let result = run_build(cmd_inv).await;
        let _ = std::fs::remove_file(&script_file);
        result
    }

    fn envelope_line() -> String {
        format!(
            r#"{{"schema":"{ENVELOPE_SCHEMA}","build_id":"00000000-0000-0000-0000-000000000000","digest":null,"platforms":[],"config":null,"size_bytes":null,"artifacts":[],"scan":null,"started_at":"1970-01-01T00:00:00Z","finished_at":"1970-01-01T00:00:00Z"}}"#
        )
    }

    /// The build prints whatever it likes and ends with a tagged envelope.
    /// Everything before the last line is the program's output and must not
    /// change the outcome.
    #[tokio::test]
    async fn a_build_that_ends_with_a_tagged_envelope_succeeds() {
        let script = format!(
            "#!/bin/sh\necho 'compiling...'\necho 'warning: unused variable'\necho '{}'\n",
            envelope_line()
        );
        let result = run_script(&script, BTreeMap::new(), Duration::from_secs(10)).await;
        let envelope = result.expect("a build ending in a tagged envelope succeeds");
        assert!(
            envelope.schema_matches(),
            "the envelope that comes back is the one this host understands"
        );
    }

    /// The security property this module exists for. A build runs third-party
    /// code — a dependency tree's install scripts run with it. If the server's
    /// environment reached the child, every secret the server holds would be
    /// readable by any package in that tree.
    ///
    /// This test sets a variable in the parent and asserts the child cannot see
    /// it, while the names the host granted are present.
    #[tokio::test]
    async fn the_server_environment_does_not_reach_the_build() {
        // Safety: single-threaded within this test's scope; the value is only
        // read by the child through its own environment, which is cleared.
        unsafe {
            std::env::set_var("TEMPS_TEST_SERVER_SECRET", "this-must-not-leak");
        }

        let script = format!(
            "#!/bin/sh\nif [ -n \"$TEMPS_TEST_SERVER_SECRET\" ]; then echo LEAKED; exit 9; fi\n\
             if [ -z \"$TEMPS_BUILD_ID\" ]; then echo MISSING_GRANTED_NAME; exit 8; fi\n\
             echo '{}'\n",
            envelope_line()
        );
        let result = run_script(&script, BTreeMap::new(), Duration::from_secs(10)).await;

        match result {
            Ok(_) => {}
            Err(BuildExecError::NonZeroExit { status: 9, .. }) => panic!(
                "the server's environment reached the build: a variable set in \
                 the parent was readable by the child, which means every \
                 install script in a dependency tree can read it too"
            ),
            Err(BuildExecError::NonZeroExit { status: 8, .. }) => {
                panic!("a name the host explicitly granted did not reach the child")
            }
            Err(e) => panic!("the build should have succeeded, got: {e}"),
        }

        unsafe {
            std::env::remove_var("TEMPS_TEST_SERVER_SECRET");
        }
    }

    /// Credentials are the caller's decision, per environment. What the caller
    /// passed must arrive; nothing else may.
    #[tokio::test]
    async fn only_the_credentials_the_caller_passed_reach_the_build() {
        let mut credentials = BTreeMap::new();
        credentials.insert("REGISTRY_TOKEN".to_string(), "granted".to_string());

        let script = format!(
            "#!/bin/sh\n[ \"$REGISTRY_TOKEN\" = granted ] || exit 7\n\
             [ -z \"$OTHER_TOKEN\" ] || exit 6\n\
             echo '{}'\n",
            envelope_line()
        );
        run_script(&script, credentials, Duration::from_secs(10))
            .await
            .expect("the granted credential arrives and no other does");
    }

    /// The reason the child gets its own process group. A build spawns
    /// compilers and package managers; killing only the build leaves them
    /// running, holding the disk and the cores the timeout was meant to free.
    ///
    /// The script starts a grandchild that appends to a file forever, then
    /// sleeps past the ceiling. After the timeout the file must stop growing.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_timeout_kills_the_whole_process_group_not_just_the_build() {
        let marker = std::env::temp_dir().join(format!("temps-build-pg-{}", uuid::Uuid::new_v4()));
        // The first write is synchronous and happens before anything is
        // backgrounded, so "did the grandchild ever run" never depends on how
        // the suite happened to be scheduled. Everything after it is the
        // survivor's doing.
        let script = format!(
            "#!/bin/sh\necho tick > '{m}'\n(while true; do echo tick >> '{m}'; sleep 0.1; done) &\nsleep 30\n",
            m = marker.display()
        );

        // Generous on purpose. The whole suite runs in parallel and this is
        // the only test whose meaning depends on wall-clock scheduling; a
        // tight ceiling makes it fail for reasons that have nothing to do with
        // process groups. Seconds here buy determinism.
        let result = run_script(&script, BTreeMap::new(), Duration::from_secs(3)).await;
        assert!(
            matches!(result, Err(BuildExecError::Timeout { .. })),
            "a build past its ceiling reports a timeout, got: {result:?}"
        );

        let size = |p: &std::path::Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        // Settle, then measure twice across a window long enough that a
        // survivor writing every 100ms cannot hide inside it.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let after_kill = size(&marker);
        tokio::time::sleep(Duration::from_millis(900)).await;
        let later = size(&marker);

        let _ = std::fs::remove_file(&marker);

        // Without this the test passes when the grandchild never ran at all,
        // which is the shape a false-comfort test takes: two zeroes compare
        // equal and prove nothing.
        assert!(
            after_kill > 0,
            "the script never wrote its first tick, so this test would have \
             compared two zeroes and claimed the process group works"
        );
        assert_eq!(
            after_kill, later,
            "the grandchild outlived the timeout: killing the child alone \
             leaves the compilers and package managers it spawned holding the \
             cores and the disk that the ceiling exists to free"
        );
    }

    /// A build's stdout belongs to the program being built. JSON on it is not
    /// a result unless it carries the tag.
    #[tokio::test]
    async fn untagged_json_on_stdout_is_output_and_not_a_result() {
        let script = "#!/bin/sh\necho '{\"tests\":12,\"failed\":0}'\n";
        let result = run_script(script, BTreeMap::new(), Duration::from_secs(10)).await;
        assert!(
            matches!(result, Err(BuildExecError::NoEnvelope { .. })),
            "a project whose test suite prints JSON must not be read as a \
             successful build, got: {result:?}"
        );
    }

    /// A shape this host was not written for is refused, never guessed at. A
    /// peer that does not recognise a schema cannot know what it is missing.
    #[tokio::test]
    async fn an_envelope_from_a_future_schema_is_refused() {
        let line = envelope_line().replace(ENVELOPE_SCHEMA, "temps.build/v2");
        let script = format!("#!/bin/sh\necho '{line}'\n");
        let result = run_script(&script, BTreeMap::new(), Duration::from_secs(10)).await;
        match result {
            Err(BuildExecError::SchemaMismatch { found, expected, .. }) => {
                assert_eq!(found, "temps.build/v2");
                assert_eq!(expected, ENVELOPE_SCHEMA);
            }
            other => panic!("a future schema must be refused explicitly, got: {other:?}"),
        }
    }

    /// A failed build must carry enough of its own output for someone to see
    /// why, and no more than a bounded amount of it.
    #[tokio::test]
    async fn a_failed_build_reports_its_status_and_the_tail_of_its_output() {
        let script = "#!/bin/sh\necho 'error: missing Dockerfile' >&2\nexit 3\n";
        let result = run_script(script, BTreeMap::new(), Duration::from_secs(10)).await;
        match result {
            Err(BuildExecError::NonZeroExit {
                status,
                output_tail,
                ..
            }) => {
                assert_eq!(status, 3, "the build's own exit status is reported");
                assert!(
                    output_tail.contains("missing Dockerfile"),
                    "the reason the build failed must survive into the error; got: {output_tail}"
                );
            }
            other => panic!("a non-zero exit is reported as such, got: {other:?}"),
        }
    }
}
