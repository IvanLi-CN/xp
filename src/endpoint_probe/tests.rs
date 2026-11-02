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
fn probe_child_parent_death_supervisor() {
    use std::{
        fs,
        process::Command,
        thread,
        time::{Duration, Instant},
    };

    let Some(worker_path) = std::env::var_os("XP_PROBE_PARENT_DEATH_WORKER_PID_FILE") else {
        return;
    };
    let Some(owner_path) = std::env::var_os("XP_PROBE_PARENT_DEATH_OWNER_PID_FILE") else {
        return;
    };
    let Some(result_path) = std::env::var_os("XP_PROBE_PARENT_DEATH_RESULT_FILE") else {
        return;
    };

    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) },
        0,
        "enable child subreaper"
    );

    let mut owner = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("endpoint_probe::tests::probe_child_parent_death_helper")
        .arg("--nocapture")
        .env("XP_PROBE_PARENT_DEATH_PID_FILE", &worker_path)
        .spawn()
        .expect("spawn helper");
    fs::write(owner_path, owner.id().to_string()).expect("write owner pid");

    let deadline = Instant::now() + Duration::from_secs(5);
    let worker_pid = loop {
        if let Some(pid) = fs::read_to_string(&worker_path)
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

    let owner_deadline = Instant::now() + Duration::from_secs(5);
    let owner_status = loop {
        if let Some(status) = owner.try_wait().expect("poll helper") {
            break status;
        }
        assert!(
            Instant::now() < owner_deadline,
            "timed out waiting for probe parent to exit"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert!(
        !owner_status.success(),
        "probe parent unexpectedly exited successfully"
    );

    let mut worker_status = 0;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match unsafe { libc::waitpid(worker_pid, &mut worker_status, libc::WNOHANG) } {
            pid if pid == worker_pid => break,
            0 if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            0 => panic!("probe child survived its parent"),
            -1 => panic!("wait for probe child: {}", std::io::Error::last_os_error()),
            pid => panic!("wait for unexpected probe child pid {pid}"),
        }
    }

    assert!(libc::WIFSIGNALED(worker_status));
    assert_eq!(libc::WTERMSIG(worker_status), libc::SIGKILL);
    fs::write(result_path, "terminated-by-parent-death").expect("write supervisor result");
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
        supervisor_pid: libc::pid_t,
        owner_pid: Option<libc::pid_t>,
    }

    impl Drop for ProcessGuard {
        fn drop(&mut self) {
            if let Some(owner_pid) = self.owner_pid {
                unsafe {
                    libc::kill(owner_pid, libc::SIGKILL);
                }
            }
            if self.supervisor_pid > 0 {
                unsafe {
                    libc::kill(self.supervisor_pid, libc::SIGKILL);
                    libc::waitpid(self.supervisor_pid, std::ptr::null_mut(), 0);
                }
            }
        }
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let worker_path = temp.path().join("probe-child.pid");
    let owner_path = temp.path().join("probe-owner.pid");
    let result_path = temp.path().join("probe-result");
    let mut supervisor = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("endpoint_probe::tests::probe_child_parent_death_supervisor")
        .arg("--nocapture")
        .env("XP_PROBE_PARENT_DEATH_WORKER_PID_FILE", &worker_path)
        .env("XP_PROBE_PARENT_DEATH_OWNER_PID_FILE", &owner_path)
        .env("XP_PROBE_PARENT_DEATH_RESULT_FILE", &result_path)
        .spawn()
        .expect("spawn supervisor");
    let supervisor_pid = supervisor.id() as libc::pid_t;
    let mut guard = ProcessGuard {
        supervisor_pid,
        owner_pid: None,
    };

    let deadline = Instant::now() + Duration::from_secs(5);
    let owner_pid = loop {
        if let Some(pid) = fs::read_to_string(&owner_path)
            .ok()
            .and_then(|value| value.trim().parse::<libc::pid_t>().ok())
        {
            break pid;
        }
        if supervisor.try_wait().expect("poll supervisor").is_some() {
            panic!("probe supervisor exited before publishing its owner pid");
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for probe owner pid"
        );
        thread::sleep(Duration::from_millis(10));
    };
    guard.owner_pid = Some(owner_pid);

    let _worker_pid = loop {
        if let Some(pid) = fs::read_to_string(&worker_path)
            .ok()
            .and_then(|value| value.trim().parse::<libc::pid_t>().ok())
        {
            break pid;
        }
        if supervisor.try_wait().expect("poll supervisor").is_some() {
            panic!("probe supervisor exited before publishing its worker pid");
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for probe child pid"
        );
        thread::sleep(Duration::from_millis(10));
    };

    assert_eq!(unsafe { libc::kill(owner_pid, libc::SIGKILL) }, 0);

    let deadline = Instant::now() + Duration::from_secs(3);
    let supervisor_status = loop {
        if let Some(status) = supervisor.try_wait().expect("poll supervisor") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for probe supervisor"
        );
        thread::sleep(Duration::from_millis(10));
    };
    guard.supervisor_pid = 0;
    guard.owner_pid = None;

    assert!(supervisor_status.success(), "probe supervisor failed");
    assert_eq!(
        fs::read_to_string(result_path).expect("read supervisor result"),
        "terminated-by-parent-death"
    );
}
