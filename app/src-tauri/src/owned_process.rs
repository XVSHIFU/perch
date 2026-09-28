use crate::store::{Failure, Result};
#[cfg(windows)]
use std::os::windows::{io::AsRawHandle, process::CommandExt};
use std::process::{Child, Command, Stdio};
#[cfg(windows)]
use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};

pub struct OwnedProcess {
    pub child: Child,
    #[cfg(windows)]
    job: isize,
}
impl OwnedProcess {
    // The host must await stdin before importing npm/DSH or spawning descendants.
    pub fn spawn(command: &mut Command) -> Result<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command
            .spawn()
            .map_err(|_| Failure::new("PROCESS_START", "无法创建引擎进程"))?;
        #[cfg(windows)]
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if job.is_null()
                || SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as _,
                    std::mem::size_of_val(&limits) as u32,
                ) == 0
                || AssignProcessToJobObject(job, child.as_raw_handle()) == 0
            {
                let _ = child.kill();
                let _ = child.wait();
                if !job.is_null() {
                    CloseHandle(job);
                }
                return Err(Failure::new(
                    "PROCESS_OWNERSHIP",
                    "无法安全管理进程树，启动已取消",
                ));
            }
            Ok(Self {
                child,
                job: job as isize,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = child.kill();
            Err(Failure::new(
                "UNSUPPORTED_PLATFORM",
                "真实引擎目前仅支持 Windows",
            ))
        }
    }
    pub fn terminate(&mut self) {
        #[cfg(windows)]
        unsafe {
            TerminateJobObject(self.job as _, 1);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        self.terminate();
        #[cfg(windows)]
        unsafe {
            CloseHandle(self.job as _);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
    #[test]
    fn dropping_owner_kills_grandchildren() {
        let node = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join("node.exe"))
            .find(|p| p.is_file())
            .unwrap();
        let mut command = Command::new(&node);
        command.env_remove("NODE_OPTIONS").arg("-e").arg("process.stdin.once('data',()=>{const c=require('node:child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{windowsHide:true,stdio:'ignore'});console.log(c.pid);setInterval(()=>{},1000)})");
        let mut owned = OwnedProcess::spawn(&mut command).unwrap();
        owned
            .child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"go\n")
            .unwrap();
        let mut pid = String::new();
        BufReader::new(owned.child.stdout.take().unwrap())
            .read_line(&mut pid)
            .unwrap();
        unsafe {
            let descendant = OpenProcess(0x00100000, 0, pid.trim().parse().unwrap());
            assert!(!descendant.is_null());
            drop(owned);
            assert_eq!(WaitForSingleObject(descendant, 5000), 0);
            CloseHandle(descendant);
        }
    }
}
