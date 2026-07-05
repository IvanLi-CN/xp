# Systemd Web upgrade delegation should use a fixed helper, not polkit details alone

## Symptoms

- Web upgrade fails immediately with `trigger_failed: systemctl exit=1`.
- Running `systemctl --no-ask-password start xp-upgrade.service` as the unprivileged `xp` user
  reports interactive authentication required.
- The `xp-upgrade.service` journal is empty, so the one-shot unit was never started.
- A polkit debug rule on CentOS 7 / polkit 0.112 sees `unit=undefined` and `verb=undefined`
  for `org.freedesktop.systemd1.manage-units`.

## Root cause

Old systemd / polkit stacks do not reliably expose the `unit` and `verb` action details to
JavaScript polkit rules. A rule that authorizes only `unit == "xp-upgrade.service"` and
`verb == "start"` is narrow on newer systems, but it never matches on those older hosts.

That means a Web upgrade design that only depends on systemd manage-units details can report ready
or look installed while the actual start request still needs an interactive root authentication
challenge.

## Fix used here

Install a root-owned fixed helper at `/usr/local/libexec/xp-upgrade-trigger`. It accepts either no
argument, which starts only `xp-upgrade.service`, or `--check`, which exits without side effects.
The helper must invoke a fixed root-controlled `systemctl` path such as `/bin/systemctl` or
`/usr/bin/systemctl`; it must not resolve `systemctl` through the caller-controlled `PATH`.

Install a narrow sudoers drop-in at `/etc/sudoers.d/91-xp-upgrade` allowing the `xp` user to run
exactly:

- `/usr/local/libexec/xp-upgrade-trigger ""`
- `/usr/local/libexec/xp-upgrade-trigger --check`

The application detects systemd support by verifying the one-shot unit, a working no-op sudo helper
check, and a non-side-effect `sudo -n -l /usr/local/libexec/xp-upgrade-trigger` check for the
no-argument start grant. The older polkit path remains a compatibility fallback, but it is not the
only supported trigger path.

## Verification

- Unit tests must reject a systemd deployment that only has `xp-upgrade.service`.
- Unit tests must reject sudoers content that only permits the helper `--check` command.
- Real-root support detection must verify both the helper `--check` probe and the no-argument start
  grant before reporting systemd Web upgrade support.
- Real-root support detection must not treat a readable legacy polkit rule file as sufficient. It
  may accept polkit only after an actual authorization probe succeeds.
- The helper must not use `command -v systemctl` or otherwise allow the unprivileged caller's `PATH`
  to influence root command selection.
- Trigger tests must assert that systemd Web upgrade invokes the fixed helper through `sudo -n`, or
  the fixed unit through `systemctl start --no-block xp-upgrade.service` for the polkit fallback.
- On CentOS 7-class hosts, rerun `xp-ops init --init-system systemd`, then verify
  `sudo -u xp sudo -n /usr/local/libexec/xp-upgrade-trigger --check` succeeds before using Web
  upgrade.
