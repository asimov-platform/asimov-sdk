// This is free and unencumbered software released into the public domain.

//! Child handles retaining the executor's process-tree ownership policy.

use std::{io, process::ExitStatus};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

#[cfg(unix)]
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};

/// A child spawned by [`crate::Executor::spawn_owned`].
///
/// With process-tree ownership enabled, termination includes the child's Unix
/// process group or Windows job. Dropping the handle requests termination
/// immediately and schedules reaping on the spawning Tokio runtime. Keep that
/// runtime running until cleanup completes; drop cannot synchronously await it.
/// Descendants on Unix are reaped by their parent or the system's orphan reaper.
///
/// Normal leader exit also terminates remaining members of an owned tree. Unix
/// programs that deliberately leave the process group are outside this policy.
/// Without tree ownership, the original Tokio child's drop policy applies.
#[derive(Debug)]
pub struct OwnedChild {
    /// The child's piped standard input, if configured and not taken.
    pub stdin: Option<ChildStdin>,
    /// The child's piped standard output, if configured and not taken.
    pub stdout: Option<ChildStdout>,
    /// The child's piped standard error, if configured and not taken.
    pub stderr: Option<ChildStderr>,
    inner: Option<ChildKind>,
    reaper: Option<tokio::runtime::Handle>,
    #[cfg(unix)]
    group: Option<Pid>,
}

#[derive(Debug)]
enum ChildKind {
    Direct(Child),
    #[cfg(windows)]
    Job(alloc::boxed::Box<dyn process_wrap::tokio::ChildWrapper>),
}

impl OwnedChild {
    pub(crate) fn spawn(command: &mut Command, tree: bool) -> io::Result<Self> {
        if !tree {
            return command.spawn().map(Self::from);
        }
        #[cfg(not(any(unix, windows)))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "process-tree ownership requires Unix or Windows",
        ));

        #[cfg(any(unix, windows))]
        {
            // Tree ownership always includes kill-on-drop, even if the raw
            // command's direct-child policy was overridden by the caller.
            command.kill_on_drop(true);
            #[cfg(unix)]
            {
                command.process_group(0);
                let mut child = Self::from(command.spawn()?);
                child.group = Pid::from_raw(child.id().expect("a new child has a PID") as i32);
                child.reaper = Some(tokio::runtime::Handle::current());
                Ok(child)
            }
            #[cfg(windows)]
            {
                use process_wrap::tokio::{CommandWrap, JobObject, KillOnDrop};

                // JobObject spawns suspended, assigns the job, then resumes:
                // descendants cannot escape between spawn and job assignment.
                let mut wrapped = CommandWrap::from(core::mem::replace(command, Command::new("")));
                let result = wrapped.wrap(KillOnDrop).wrap(JobObject).spawn();
                *command = wrapped.into_command();
                let mut child = result?;
                Ok(Self {
                    stdin: child.stdin().take(),
                    stdout: child.stdout().take(),
                    stderr: child.stderr().take(),
                    inner: Some(ChildKind::Job(child)),
                    reaper: Some(tokio::runtime::Handle::current()),
                })
            }
        }
    }

    /// Returns the leader's process ID until it has been reaped.
    pub fn id(&self) -> Option<u32> {
        match self.inner.as_ref().unwrap() {
            ChildKind::Direct(child) => child.id(),
            #[cfg(windows)]
            ChildKind::Job(child) => child.id(),
        }
    }

    /// Requests termination of the child and, when owned, its entire tree.
    /// Call [`wait`](Self::wait) to reap the leader and observe its exit status.
    /// OS signaling failures are returned as I/O errors.
    pub fn start_kill(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(group) = self.group {
            match kill_process_group(group, Signal::KILL) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {},
                // Darwin skips zombies when signaling a group and reports
                // EPERM if no signalable members remain. Reap the leader.
                #[cfg(target_vendor = "apple")]
                Err(rustix::io::Errno::PERM) if leader_exited(group)? => {},
                Err(error) => return Err(error.into()),
            }
            self.group = None;
            return Ok(());
        }
        match self.inner.as_mut().unwrap() {
            ChildKind::Direct(child) => child.start_kill(),
            #[cfg(windows)]
            ChildKind::Job(child) => child.start_kill(),
        }
    }

    /// Terminates the owned child/tree and waits for the leader to be reaped.
    /// Returns any signaling or wait error. Cancelling this future retains the
    /// handle's drop cleanup policy.
    pub async fn kill(&mut self) -> io::Result<()> {
        self.start_kill()?;
        self.wait().await?;
        Ok(())
    }

    /// Closes owned stdin and waits for the leader, returning its exit status.
    /// Any remaining owned descendants are terminated on leader exit. This
    /// method is cancellation-safe and returns OS signaling or wait errors.
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        drop(self.stdin.take());
        #[cfg(unix)]
        if let Some(group) = self.group {
            // Observe exit without reaping: keeping the leader's PID reserved
            // prevents a delayed cancellation from signaling a reused PGID.
            // Register before checking status to avoid missing SIGCHLD.
            let mut exits = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::child())?;
            while !leader_exited(group)? {
                exits
                    .recv()
                    .await
                    .ok_or_else(|| io::Error::other("SIGCHLD stream closed"))?;
            }
            self.start_kill()?;
        }
        let status = match self.inner.as_mut().unwrap() {
            ChildKind::Direct(child) => child.wait().await?,
            #[cfg(windows)]
            ChildKind::Job(child) => {
                // This wrapper has exactly one layer: the job owns a native
                // Tokio child. Observe the leader before waiting on the job.
                child.inner_mut().wait().await?;
                child.start_kill()?;
                child.wait().await?
            },
        };
        self.reaper = None;
        Ok(status)
    }

    /// Checks for leader exit without blocking, terminating remaining owned
    /// descendants and reaping the leader if it has exited. Returns OS errors.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        #[cfg(unix)]
        if let Some(group) = self.group {
            if !leader_exited(group)? {
                return Ok(None);
            }
            self.start_kill()?;
        }
        let status = match self.inner.as_mut().unwrap() {
            ChildKind::Direct(child) => child.try_wait()?,
            #[cfg(windows)]
            ChildKind::Job(child) => {
                let status = child.inner_mut().try_wait()?;
                if status.is_some() {
                    child.start_kill()?;
                }
                status
            },
        };
        if status.is_some() {
            self.reaper = None;
        }
        Ok(status)
    }
}

#[cfg(unix)]
fn leader_exited(pid: Pid) -> io::Result<bool> {
    loop {
        match waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        ) {
            Ok(status) => return Ok(status.is_some()),
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

impl From<Child> for OwnedChild {
    /// Wraps a Tokio child with its existing direct-child lifetime policy.
    fn from(mut child: Child) -> Self {
        Self {
            stdin: child.stdin.take(),
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            inner: Some(ChildKind::Direct(child)),
            reaper: None,
            #[cfg(unix)]
            group: None,
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(runtime) = self.reaper.take() {
            let _ = self.start_kill();
            let mut child = self.inner.take().unwrap();
            // Signal synchronously, then reap independently of the cancelled
            // request. Only cleanup runs detached; no input/output is consumed.
            runtime.spawn(async move {
                match &mut child {
                    ChildKind::Direct(child) => {
                        let _ = child.wait().await;
                    },
                    #[cfg(windows)]
                    ChildKind::Job(child) => {
                        let _ = child.wait().await;
                    },
                }
            });
        }
    }
}
