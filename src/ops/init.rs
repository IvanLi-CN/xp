use crate::ops::cli::{ExitError, InitArgs, InitSystemArg};
use crate::ops::paths::Paths;
use crate::ops::platform::{Distro, InitSystem, detect_distro, detect_init_system};
use crate::ops::util::{Mode, chmod, ensure_dir, is_test_root, write_string_if_changed};
use serde_json::json;
use std::path::Path;
use std::process::Command;

pub async fn cmd_init(paths: Paths, args: InitArgs) -> Result<(), ExitError> {
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };
    let distro = detect_distro(&paths).map_err(|e| ExitError::new(2, e))?;
    let init_system = detect_init_system(
        distro,
        match args.init_system {
            InitSystemArg::Auto => None,
            InitSystemArg::Systemd => Some(InitSystem::Systemd),
            InitSystemArg::Openrc => Some(InitSystem::OpenRc),
            InitSystemArg::None => Some(InitSystem::None),
        },
    );

    if mode == Mode::DryRun {
        eprintln!("init system: {init_system:?}");
    }

    ensure_layout(&paths, distro, &args, mode)?;

    match init_system {
        InitSystem::Systemd => {
            write_systemd_units(&paths, &args, mode)?;
            if args.enable_services {
                enable_systemd_services(&paths, mode)?;
            }
        }
        InitSystem::OpenRc => {
            write_openrc_scripts(&paths, &args, mode)?;
            if args.enable_services {
                enable_openrc_services(mode)?;
            }
        }
        InitSystem::None => {}
    }

    Ok(())
}

fn ensure_layout(
    paths: &Paths,
    distro: Distro,
    args: &InitArgs,
    mode: Mode,
) -> Result<(), ExitError> {
    let xp_work_dir = paths.map_abs(&args.xp_work_dir);
    let xp_data_dir = paths.map_abs(&args.xp_data_dir);
    let xray_work_dir = paths.map_abs(&args.xray_work_dir);
    let etc_xray_dir = paths.etc_xray_dir();
    let etc_xp_dir = paths.etc_xp_dir();
    let etc_xp_ops_cf_dir = paths.etc_xp_ops_cloudflare_dir();
    let etc_cloudflared_dir = paths.etc_cloudflared_dir();

    let dirs = [
        xp_work_dir.as_path(),
        xp_data_dir.as_path(),
        xray_work_dir.as_path(),
        etc_xray_dir.as_path(),
        etc_xp_dir.as_path(),
        etc_xp_ops_cf_dir.as_path(),
        etc_cloudflared_dir.as_path(),
    ];

    for d in dirs {
        if mode == Mode::DryRun {
            eprintln!("would create dir: {}", d.display());
            continue;
        }
        ensure_dir(d).map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    }

    if mode == Mode::Real {
        write_xray_config(paths)?;
    } else {
        eprintln!("would write: /etc/xray/config.json");
    }

    if mode == Mode::Real && !is_test_root(paths.root()) {
        ensure_user_group(distro, "xp", &args.xp_work_dir)?;
        ensure_user_group(distro, "xray", &args.xray_work_dir)?;

        run_or_fail(
            "chown",
            &["-R", "xp:xp", args.xp_work_dir.to_string_lossy().as_ref()],
        )?;
        run_or_fail(
            "chown",
            &["-R", "xp:xp", args.xp_data_dir.to_string_lossy().as_ref()],
        )?;
        run_or_fail(
            "chown",
            &[
                "-R",
                "xray:xray",
                args.xray_work_dir.to_string_lossy().as_ref(),
            ],
        )?;
    } else if mode == Mode::DryRun {
        eprintln!("would ensure users: xp, xray");
        eprintln!("would chown: xp_work_dir/xp_data_dir to xp:xp; xray_work_dir to xray:xray");
    }

    Ok(())
}

fn write_xray_config(paths: &Paths) -> Result<(), ExitError> {
    let cfg = json!({
      "log": { "loglevel": "warning" },
      "api": {
        "tag": "api",
        "listen": "127.0.0.1:10085",
        "services": ["HandlerService", "StatsService"]
      },
      "stats": {},
      "policy": {
        "levels": {
          "0": { "statsUserUplink": true, "statsUserDownlink": true }
        }
      },
      "inbounds": [],
      "outbounds": [
        { "tag": "direct", "protocol": "freedom", "settings": {} },
        { "tag": "block", "protocol": "blackhole", "settings": {} }
      ]
    });

    let content = serde_json::to_string_pretty(&cfg)
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    write_string_if_changed(&paths.etc_xray_config(), &(content + "\n"))
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    chmod(&paths.etc_xray_config(), 0o644).ok();
    Ok(())
}

fn write_systemd_units(paths: &Paths, args: &InitArgs, mode: Mode) -> Result<(), ExitError> {
    let dir = paths.systemd_unit_dir();
    if mode == Mode::DryRun {
        eprintln!("would write systemd units under: {}", dir.display());
        return Ok(());
    }

    ensure_dir(&dir).map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;

    let xp_unit = systemd_xp_unit(args);
    let xray_unit = systemd_xray_unit(args);

    let xp_path = dir.join("xp.service");
    let xray_path = dir.join("xray.service");

    write_string_if_changed(&xp_path, &xp_unit)
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    write_string_if_changed(&xray_path, &xray_unit)
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;

    Ok(())
}

fn systemd_xp_unit(args: &InitArgs) -> String {
    format!(
        "[Unit]\n\
Description=xp (Xray control plane)\n\
Wants=network-online.target\n\
After=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
User=xp\n\
Group=xp\n\
WorkingDirectory={}\n\
Environment=XP_DATA_DIR={}\n\
EnvironmentFile=-/etc/xp/xp.env\n\
ExecStart=/usr/local/bin/xp run\n\
Restart=always\n\
RestartSec=2s\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        args.xp_work_dir.display(),
        args.xp_data_dir.display()
    )
}

fn systemd_xray_unit(args: &InitArgs) -> String {
    format!(
        "[Unit]\n\
Description=xray (local proxy runtime)\n\
Wants=network-online.target\n\
After=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
User=xray\n\
Group=xray\n\
WorkingDirectory={}\n\
ExecStart=/usr/local/bin/xray run -c /etc/xray/config.json\n\
Restart=always\n\
RestartSec=2s\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        args.xray_work_dir.display()
    )
}

fn enable_systemd_services(paths: &Paths, mode: Mode) -> Result<(), ExitError> {
    if mode == Mode::DryRun {
        eprintln!("would run: systemctl daemon-reload");
        eprintln!("would run: systemctl enable --now xray.service");
        eprintln!("would run: systemctl enable --now xp.service");
        return Ok(());
    }
    if is_test_root(paths.root()) {
        return Ok(());
    }
    run_or_fail("systemctl", &["daemon-reload"])?;
    run_or_fail("systemctl", &["enable", "--now", "xray.service"])?;
    run_or_fail("systemctl", &["enable", "--now", "xp.service"])?;
    Ok(())
}

fn write_openrc_scripts(paths: &Paths, _args: &InitArgs, mode: Mode) -> Result<(), ExitError> {
    let initd = paths.openrc_initd_dir();
    let confd = paths.openrc_confd_dir();

    if mode == Mode::DryRun {
        eprintln!("would write OpenRC scripts under: {}", initd.display());
        eprintln!("would write OpenRC conf.d under: {}", confd.display());
        return Ok(());
    }

    ensure_dir(&initd).map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    ensure_dir(&confd).map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;

    let xp_script = openrc_xp_script();
    let xray_script = openrc_xray_script();

    let xp_path = initd.join("xp");
    let xray_path = initd.join("xray");

    write_string_if_changed(&xp_path, &xp_script)
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    write_string_if_changed(&xray_path, &xray_script)
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {e}")))?;
    chmod(&xp_path, 0o755).ok();
    chmod(&xray_path, 0o755).ok();

    Ok(())
}

fn openrc_xp_script() -> String {
    "#!/sbin/openrc-run\n\nname=\"xp\"\ndescription=\"xp (Xray control plane)\"\n\ncommand=\"/bin/sh\"\ncommand_args=\"-c 'set -a; [ -f /etc/xp/xp.env ] && . /etc/xp/xp.env; set +a; exec /usr/local/bin/xp run --data-dir /var/lib/xp/data'\"\ncommand_user=\"xp:xp\"\ncommand_background=\"yes\"\npidfile=\"/run/xp.pid\"\n\ndepend() {\n  need net\n}\n".to_string()
}

fn openrc_xray_script() -> String {
    "#!/sbin/openrc-run\n\nname=\"xray\"\ndescription=\"xray (local proxy runtime)\"\n\ncommand=\"/usr/local/bin/xray\"\ncommand_args=\"run -c /etc/xray/config.json\"\ncommand_user=\"xray:xray\"\ncommand_background=\"yes\"\npidfile=\"/run/xray.pid\"\n\ndepend() {\n  need net\n}\n".to_string()
}

fn enable_openrc_services(mode: Mode) -> Result<(), ExitError> {
    if mode == Mode::DryRun {
        eprintln!("would run: rc-update add xray default");
        eprintln!("would run: rc-update add xp default");
        eprintln!("would run: rc-service xray start");
        eprintln!("would run: rc-service xp start");
        return Ok(());
    }
    run_or_fail("rc-update", &["add", "xray", "default"])?;
    run_or_fail("rc-update", &["add", "xp", "default"])?;
    run_or_fail("rc-service", &["xray", "start"])?;
    run_or_fail("rc-service", &["xp", "start"])?;
    Ok(())
}

fn ensure_user_group(distro: Distro, user: &str, home: &Path) -> Result<(), ExitError> {
    let status = Command::new("id").args(["-u", user]).status();
    if matches!(status, Ok(s) if s.success()) {
        return Ok(());
    }

    match distro {
        Distro::Arch | Distro::Debian => {
            let home_str = home.to_string_lossy();
            let args = vec![
                "--system".to_string(),
                "--home".to_string(),
                home_str.to_string(),
                "--shell".to_string(),
                "/usr/sbin/nologin".to_string(),
                "--user-group".to_string(),
                user.to_string(),
            ];
            run_or_fail_owned("useradd", &args)
        }
        Distro::Alpine => {
            run_or_fail("addgroup", &["-S", user])?;
            run_or_fail_owned(
                "adduser",
                &[
                    "-S".to_string(),
                    "-D".to_string(),
                    "-H".to_string(),
                    "-s".to_string(),
                    "/sbin/nologin".to_string(),
                    "-G".to_string(),
                    user.to_string(),
                    user.to_string(),
                ],
            )
        }
    }
}

fn run_or_fail(program: &str, args: &[&str]) -> Result<(), ExitError> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {program}: {e}")))?;
    if !status.success() {
        return Err(ExitError::new(
            4,
            format!(
                "filesystem_error: {program} exit={}",
                status.code().unwrap_or(-1)
            ),
        ));
    }
    Ok(())
}

fn run_or_fail_owned(program: &str, args: &[String]) -> Result<(), ExitError> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| ExitError::new(4, format!("filesystem_error: {program}: {e}")))?;
    if !status.success() {
        return Err(ExitError::new(
            4,
            format!(
                "filesystem_error: {program} exit={}",
                status.code().unwrap_or(-1)
            ),
        ));
    }
    Ok(())
}
