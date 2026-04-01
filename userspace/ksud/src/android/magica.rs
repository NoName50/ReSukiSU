use adb_client::ADBDeviceExt;
use adb_client::tcp::ADBTcpDevice;
use anyhow::{Context, Result, bail};
use log::{error, info};
use prop_rs_android::resetprop::ResetProp;
use prop_rs_android::sys_prop;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::Command;

const fn resetprop() -> ResetProp {
    ResetProp {
        skip_svc: true,
        persistent: false,
        persist_only: false,
        verbose: false,
        show_context: false,
    }
}

fn exec_shell_commands(commands: &[(&str, &[&str])], log_prefix: &str) -> Result<()> {
    for (cmd, args) in commands {
        info!("{log_prefix}: {cmd} {}", args.join(" "));
        let status = Command::new(cmd)
            .args(*args)
            .status()
            .with_context(|| format!("Failed to execute {cmd}"))?;
        if !status.success() {
            bail!("{cmd} {} exited with {status}", args.join(" "));
        }
    }
    Ok(())
}

fn root_self() -> Result<()> {
    const AID_ROOT: libc::uid_t = 0;
    const AID_SYSTEM: libc::gid_t = 1000;
    const AID_ADB: libc::gid_t = 1011;
    const AID_LOG: libc::gid_t = 1007;
    const AID_INPUT: libc::gid_t = 1004;
    const AID_INET: libc::gid_t = 3003;
    const AID_NET_BT: libc::gid_t = 3002;
    const AID_NET_BT_ADMIN: libc::gid_t = 3001;
    const AID_SDCARD_R: libc::gid_t = 1028;
    const AID_SDCARD_RW: libc::gid_t = 1015;
    const AID_NET_BW_STATS: libc::gid_t = 3006;
    const AID_READPROC: libc::gid_t = 3009;
    const AID_UHID: libc::gid_t = 3011;
    const AID_EXT_DATA_RW: libc::gid_t = 1078;
    const AID_EXT_OBB_RW: libc::gid_t = 1079;
    const AID_READTRACEFS: libc::gid_t = 3012;

    unsafe {
        if libc::setresuid(AID_ROOT, AID_ROOT, AID_ROOT) != 0 {
            let err = std::io::Error::last_os_error();
            bail!("setresuid failed: {err}");
        }

        if libc::geteuid() != AID_ROOT {
            bail!("failed to become root after setresuid");
        }

        if libc::setresgid(AID_ROOT, AID_ROOT, AID_ROOT) != 0 {
            let err = std::io::Error::last_os_error();
            bail!("setresgid failed: {err}");
        }

        let groups: [libc::gid_t; 15] = [
            AID_SYSTEM,
            AID_ADB,
            AID_LOG,
            AID_INPUT,
            AID_INET,
            AID_NET_BT,
            AID_NET_BT_ADMIN,
            AID_SDCARD_R,
            AID_SDCARD_RW,
            AID_NET_BW_STATS,
            AID_READPROC,
            AID_UHID,
            AID_EXT_DATA_RW,
            AID_EXT_OBB_RW,
            AID_READTRACEFS,
        ];

        if libc::setgroups(groups.len(), groups.as_ptr()) != 0 {
            let err = std::io::Error::last_os_error();
            bail!("setgroups failed: {err}");
        }
    }

    info!("We Are Root!!!");

    Ok(())
}

fn enable_adb_root(port: u16) -> Result<()> {
    // We are in limited root by magica
    anyhow::ensure!(
        rustix::process::getuid().as_raw() == 0,
        "must be run as root"
    );

    sys_prop::init().context("Failed to initialize system property API")?;
    let rp = resetprop();

    let debuggable_context = sys_prop::get_context("ro.debuggable")
        .context("Failed to get context for ro.debuggable")?;
    info!("ro.debuggable context: {debuggable_context}");

    let adb_secure_context = sys_prop::get_context("ro.adb.secure")
        .context("Failed to get context for ro.adb.secure")?;
    info!("ro.adb.secure context: {adb_secure_context}");

    let props_serial = "/dev/__properties__/properties_serial";
    let debuggable_context = format!("/dev/__properties__/{debuggable_context}");
    let adb_secure_context = format!("/dev/__properties__/{adb_secure_context}");
    let port_str = port.to_string();

    // chmod property files to writable
    exec_shell_commands(
        &[
            ("chmod", &["0644", props_serial]),
            ("chmod", &["0644", &debuggable_context]),
            ("chmod", &["0644", &adb_secure_context]),
        ],
        "Executing",
    )?;

    // Set properties via internal API
    rp.set("ro.debuggable", "1")
        .context("Failed to set ro.debuggable")?;
    info!("Executing: resetprop -n ro.debuggable 1");
    rp.set("ro.adb.secure", "0")
        .context("Failed to set ro.adb.secure")?;
    info!("Executing: resetprop -n ro.adb.secure 0");

    // Restore permissions and restart adbd
    exec_shell_commands(
        &[
            ("chmod", &["0444", props_serial]),
            ("chmod", &["0444", &debuggable_context]),
            ("chmod", &["0444", &adb_secure_context]),
            ("setprop", &["service.adb.root", "1"]),
            ("setprop", &["service.adb.tcp.port", &port_str]),
            ("setprop", &["ctl.restart", "adbd"]),
        ],
        "Executing",
    )?;

    Ok(())
}

pub fn disable_adb_root() -> Result<()> {
    // We have full root now, no need to chmod
    sys_prop::init().context("Failed to initialize system property API")?;
    let rp = resetprop();

    info!("Restoring: resetprop -n ro.debuggable 0");
    rp.set("ro.debuggable", "0")
        .context("Failed to set ro.debuggable")?;

    info!("Restoring: resetprop -n ro.adb.secure 1");
    rp.set("ro.adb.secure", "1")
        .context("Failed to set ro.adb.secure")?;

    for prop in &[
        "service.adb.root",
        "service.adb.tcp.port",
        "ro.boot.selinux",
    ] {
        info!("Restoring: resetprop --delete {prop}");
        let _ = rp.delete(prop);
        if let Ok(ctx) = sys_prop::get_context(prop) {
            let _ = sys_prop::compact(Some(&ctx));
        }
    }

    // Restore permissions and restart adbd
    exec_shell_commands(
        &[
            ("chmod", &["0444", "/dev/__properties__/u:object_r:adbd_config_prop:s0"]),
            ("chmod", &["0444", "/dev/__properties__/u:object_r:shell_prop:s0"]),
            ("setprop", &["ctl.restart", "adbd"]),
        ],
        "Restoring",
    )?;

    Ok(())
}

fn connect_to_device(port: u16) -> Result<ADBTcpDevice> {
    const MAX_RETRIES: u32 = 30;
    for attempt in 1..=MAX_RETRIES {
        info!("Waiting for adbd to restart... (attempt {attempt}/{MAX_RETRIES})");
        std::thread::sleep(std::time::Duration::from_secs(1));

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        info!("Connecting to ADB device at {addr}");
        match ADBTcpDevice::new(addr).context("Failed to create ADBTcpDevice") {
            Ok(device) => return Ok(device),
            Err(e) => {
                error!("Failed to connect to ADB device: {e:?}, retry after 1s");
            }
        }
    }
    bail!("Failed to connect to ADB device after {MAX_RETRIES} attempts")
}

fn unload_oplus_secure_guard(device: &mut ADBTcpDevice) -> Result<()> {
    // Oplus Secure Guard will kill new processes with root privilege, so we need to unload it first.
    // We can just unload the kernel module via adb shell since we have root there.
    let cmd = "lsmod | grep oplus_secure_guard";
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    device.shell_command(&cmd, Some(&mut stdout), Some(&mut stderr))?;
    let output = String::from_utf8_lossy(&stdout);
    if output.contains("oplus_secure_guard") {
        info!("Oplus Secure Guard is loaded, unloading it...");
        let cmd = "rmmod oplus_secure_guard";
        device.shell_command(&cmd, None, None)?;
        info!("Unloaded Oplus Secure Guard");
    } else {
        info!("Oplus Secure Guard is not loaded, no need to unload");
    }
    Ok(())
}

pub fn run(port: u16) -> Result<()> {
    root_self()?;

    enable_adb_root(port)?;

    let mut device = connect_to_device(port)?;

    unload_oplus_secure_guard(&mut device)?;

    let self_path = std::env::current_exe().context("Failed to get self exe path")?;

    // Execute late-load with --post-magica via adb shell.
    // The late-load process has full root + su domain and will:
    // 1. Load kernelsu.ko, enforce SELinux, run stage scripts
    // 2. Restore adb properties (disable adb root/tcp mode)
    let cmd = format!("{} late-load --post-magica", self_path.display());
    info!("Executing '{cmd}' via adb shell...");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Err(e) = device.shell_command(&cmd, Some(&mut stdout), Some(&mut stderr)) {
        info!("adb shell finished with error (may be expected): {e}");
    }
    if !stdout.is_empty() {
        info!("stdout: {}", String::from_utf8_lossy(&stdout));
    }
    if !stderr.is_empty() {
        info!("stderr: {}", String::from_utf8_lossy(&stderr));
    }

    Ok(())
}
