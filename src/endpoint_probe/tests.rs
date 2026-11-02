use super::{create_private_dir, vless_probe_transport_settings, write_private_file};
use crate::protocol::{VLESS_XHTTP_PATH, VlessRealityTransport};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn probe_temp_files_are_not_world_readable() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("xp-probe-perm-test");

    create_private_dir(&dir).expect("create private dir");

    #[cfg(unix)]
    {
        let mode = std::fs::metadata(&dir)
            .expect("dir meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "dir mode should be 0700");
    }

    let file = dir.join("config.json");
    write_private_file(&file, b"{}").expect("write private file");

    #[cfg(unix)]
    {
        let mode = std::fs::metadata(&file)
            .expect("file meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "file mode should be 0600");
    }
}

#[test]
fn xhttp_probe_uses_xhttp_stream_without_vision_flow() {
    let (flow, network, settings) = vless_probe_transport_settings(VlessRealityTransport::Xhttp);

    assert_eq!(flow, "");
    assert_eq!(network, "xhttp");
    let settings = settings.unwrap();
    assert_eq!(settings["path"], VLESS_XHTTP_PATH);
    assert_eq!(settings["mode"], "stream-one");
}

#[test]
fn probe_child_configuration_enables_kill_on_drop() {
    let mut command = tokio::process::Command::new("sleep");

    super::configure_probe_child(&mut command);

    assert!(command.get_kill_on_drop());
}

#[cfg(target_os = "linux")]
#[test]
fn probe_child_parent_death_helper() {
    let Some(pid_path) = std::env::var_os("XP_PROBE_PARENT_DEATH_PID_FILE") else {
        return;
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .expect("tokio runtime");
    let worker_pid = runtime.block_on(async {
        let mut command = tokio::process::Command::new("sleep");
        command.arg("60");
        super::configure_probe_child(&mut command);
        let child = match command.spawn() {
            Ok(child) => child,
            Err(_) => unsafe { libc::_exit(111) },
        };
        let worker_pid = child.id().expect("worker pid");
        std::mem::forget(child);
        worker_pid
    });
    drop(runtime);
    if std::fs::write(pid_path, worker_pid.to_string()).is_err() {
        unsafe { libc::_exit(112) };
    }
    loop {
        unsafe {
            libc::pause();
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn probe_child_dies_when_probe_parent_is_killed() {
    use std::{
        fs,
        process::Command,
        thread,
        time::{Duration, Instant},
    };

    struct ProcessGuard {
        owner_pid: libc::pid_t,
        worker_pid: Option<libc::pid_t>,
    }

    struct ChildSubreaperGuard {
        previous: libc::c_int,
    }

    impl Drop for ChildSubreaperGuard {
        fn drop(&mut self) {
            unsafe {
                libc::prctl(libc::PR_SET_CHILD_SUBREAPER, self.previous);
            }
        }
    }

    impl Drop for ProcessGuard {
        fn drop(&mut self) {
            if self.owner_pid > 0 {
                unsafe {
                    libc::kill(self.owner_pid, libc::SIGKILL);
                    libc::waitpid(self.owner_pid, std::ptr::null_mut(), 0);
                }
            }
            if let Some(worker_pid) = self.worker_pid {
                unsafe {
                    libc::kill(worker_pid, libc::SIGKILL);
                    libc::waitpid(worker_pid, std::ptr::null_mut(), 0);
                }
            }
        }
    }

    fn process_state(pid: libc::pid_t) -> Option<char> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (_, fields) = stat.rsplit_once(") ")?;
        fields.chars().next()
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let pid_path = temp.path().join("probe-child.pid");
    let mut previous_subreaper = 0;
    assert_eq!(
        unsafe {
            libc::prctl(
                libc::PR_GET_CHILD_SUBREAPER,
                &mut previous_subreaper as *mut libc::c_int,
            )
        },
        0,
        "read child subreaper state"
    );
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) },
        0,
        "enable child subreaper"
    );
    let _subreaper = ChildSubreaperGuard {
        previous: previous_subreaper,
    };
    let mut owner = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("endpoint_probe::tests::probe_child_parent_death_helper")
        .arg("--nocapture")
        .env("XP_PROBE_PARENT_DEATH_PID_FILE", &pid_path)
        .spawn()
        .expect("spawn helper");
    let owner_pid = owner.id() as libc::pid_t;
    let mut guard = ProcessGuard {
        owner_pid,
        worker_pid: None,
    };

    let deadline = Instant::now() + Duration::from_secs(5);
    let worker_pid = loop {
        if let Some(pid) = fs::read_to_string(&pid_path)
            .ok()
            .and_then(|value| value.trim().parse::<libc::pid_t>().ok())
        {
            break pid;
        }
        if owner.try_wait().expect("poll helper").is_some() {
            panic!("probe parent helper exited before publishing its child pid");
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for probe child pid"
        );
        thread::sleep(Duration::from_millis(10));
    };
    guard.worker_pid = Some(worker_pid);

    assert_eq!(unsafe { libc::kill(owner_pid, libc::SIGKILL) }, 0);
    assert_eq!(
        unsafe { libc::waitpid(owner_pid, std::ptr::null_mut(), 0) },
        owner_pid
    );
    guard.owner_pid = 0;

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match process_state(worker_pid) {
            None => {
                guard.worker_pid = None;
                break;
            }
            Some('Z') => {
                assert_eq!(
                    unsafe { libc::waitpid(worker_pid, std::ptr::null_mut(), 0) },
                    worker_pid,
                    "reap probe child"
                );
                guard.worker_pid = None;
                break;
            }
            Some(_) if Instant::now() >= deadline => {
                panic!("probe child survived its parent");
            }
            Some(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
}
