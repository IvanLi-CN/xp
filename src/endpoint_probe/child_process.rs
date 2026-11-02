use tokio::process::Command;

pub(super) fn configure_probe_child(command: &mut Command) {
    command.kill_on_drop(true);

    #[cfg(target_os = "linux")]
    {
        let parent_pid = unsafe { libc::getpid() };
        // Ensure an OOM-killed or otherwise terminated XP cannot orphan probe Xray processes.
        unsafe {
            command.pre_exec(move || configure_linux_parent_death_signal(parent_pid));
        }
    }
}

#[cfg(target_os = "linux")]
fn configure_linux_parent_death_signal(parent_pid: libc::pid_t) -> std::io::Result<()> {
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } != 0 {
        return Err(std::io::Error::last_os_error());
    }

    // The parent may die between fork and prctl; fail closed instead of leaving an orphan.
    if unsafe { libc::getppid() } != parent_pid {
        unsafe { libc::_exit(1) };
    }

    Ok(())
}
