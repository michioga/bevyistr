//! Own only the local processes started for one solver/partitioner step.
//! MPI implementations that launch on remote hosts need separate scheduler
//! integration; this guard is for local workstation runs.

use std::io;
use std::process::{Child, Command};

#[cfg(windows)]
pub(super) struct ProcessTree(std::os::windows::io::OwnedHandle);

#[cfg(windows)]
impl ProcessTree {
    pub(super) fn configure(command: &mut Command) {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }

    pub(super) fn attach(child: &Child) -> io::Result<Self> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::JobObjects::*;
        // A private, unnamed, non-inherited job. Closing its last handle also
        // cleans up ranks if bevyistr exits while an MPI run is active.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let owned = OwnedHandle::from_raw_handle(handle);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle()) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(owned))
        }
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        // The handle belongs exclusively to this step, never to another run.
        if unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0.as_raw_handle(), 1)
        } == 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(unix)]
pub(super) struct ProcessTree(libc::pid_t);

#[cfg(unix)]
impl ProcessTree {
    pub(super) fn configure(command: &mut Command) {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    pub(super) fn attach(child: &Child) -> io::Result<Self> {
        Ok(Self(child.id() as libc::pid_t))
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        // process_group(0) creates a group whose ID is this child's PID.
        let result = unsafe { libc::killpg(self.0, libc::SIGKILL) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }
}

#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

#[cfg(not(any(unix, windows)))]
pub(super) struct ProcessTree;

#[cfg(not(any(unix, windows)))]
impl ProcessTree {
    pub(super) fn configure(_command: &mut Command) {}
    pub(super) fn attach(_child: &Child) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Local solver process control is not supported on this platform",
        ))
    }
    pub(super) fn terminate(&self) -> io::Result<()> {
        Ok(())
    }
}
