//! Windows job object: the language server and every process it spawns die with the job.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::Child;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

pub(crate) struct Job(HANDLE);

// A job handle is just a kernel object handle; it may be used from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// New job configured with `KILL_ON_JOB_CLOSE` (also fires if our process crashes).
    pub(crate) fn new() -> io::Result<Job> {
        // SAFETY: null attributes/name are allowed; the result is checked.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Job(handle);
        // SAFETY: zeroed is a valid initial value for this plain-data struct.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` outlives the call and the size matches the information class.
        let ok = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    pub(crate) fn assign(&self, child: &Child) -> io::Result<()> {
        // SAFETY: both handles are valid; the child handle is owned by `child`.
        let ok = unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub(crate) fn terminate(&self) {
        // SAFETY: valid job handle.
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: we own the handle; closing it triggers KILL_ON_JOB_CLOSE.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
