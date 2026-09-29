//! 通过 PTY 主端和进程表了解窗格里在跑什么(类 Unix;Windows 上全部返回 None,功能相应降级)。

use std::path::PathBuf;

/// PTY 主端的一个副本,用来问 tty 的状态(与 alacritty IO 线程持有的是同一个 pty)
pub struct PtyProbe {
    #[cfg(unix)]
    fd: std::fs::File,
}

impl PtyProbe {
    #[cfg(unix)]
    pub fn new(file: &std::fs::File) -> Option<PtyProbe> {
        file.try_clone().ok().map(|fd| PtyProbe { fd })
    }

    /// tty 处于 raw 模式(ICANON 关)= 前台程序自己读按键,即交互式全屏程序(claude、vim、less)。
    /// 普通命令如 `sleep 4` 跑的时候 ICANON 是开的(zsh exec 前恢复了规范模式)。
    /// 注意在提示符处 zle 也是 raw,所以只应在"有命令在跑"时问。
    pub fn is_raw_mode(&self) -> Option<bool> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let mut attrs: libc::termios = unsafe { std::mem::zeroed() };
            if unsafe { libc::tcgetattr(self.fd.as_raw_fd(), &mut attrs) } != 0 {
                return None;
            }
            Some(attrs.c_lflag & libc::ICANON == 0)
        }
        #[cfg(not(unix))]
        None
    }

    /// PTY 前台进程组的 pid(当前拿着键盘的程序)
    pub fn foreground_pid(&self) -> Option<u32> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let pgid = unsafe { libc::tcgetpgrp(self.fd.as_raw_fd()) };
            (pgid > 0).then_some(pgid as u32)
        }
        #[cfg(not(unix))]
        None
    }

    /// 前台是 Claude Code 时返回它的工作目录,否则 None。
    /// 用可执行文件**路径**判断而不是进程名:原生安装器 exec 的是 `~/.local/share/claude/versions/<版本号>`,
    /// 进程名只是个版本号,路径里才有 "claude"。npm 安装 exec 的是 node,认不出来。
    pub fn claude_working_directory(&self) -> Option<PathBuf> {
        let pid = self.foreground_pid()?;
        let exe = executable_path(pid)?;
        if !exe.to_string_lossy().to_lowercase().contains("claude") {
            return None;
        }
        process_cwd(pid)
    }
}

#[cfg(target_os = "linux")]
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(target_os = "linux")]
pub fn executable_path(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[cfg(target_os = "macos")]
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    use std::ffi::CStr;
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let n = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    if n != size {
        return None;
    }
    let path = unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr() as *const libc::c_char) };
    Some(PathBuf::from(path.to_string_lossy().into_owned()))
}

#[cfg(target_os = "macos")]
pub fn executable_path(pid: u32) -> Option<PathBuf> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let n = unsafe {
        libc::proc_pidpath(pid as libc::c_int, buf.as_mut_ptr() as *mut libc::c_void, buf.len() as u32)
    };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(PathBuf::from(String::from_utf8_lossy(&buf).into_owned()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn process_cwd(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn executable_path(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn inspects_own_process() {
        let pid = std::process::id();
        assert_eq!(process_cwd(pid), std::env::current_dir().ok());
        let exe = executable_path(pid).unwrap();
        assert_eq!(exe.canonicalize().ok(), std::env::current_exe().unwrap().canonicalize().ok());
    }
}
