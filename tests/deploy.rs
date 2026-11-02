//! L'unité systemd et le script d'installation respectent le contrat du service.

use std::fs;
use std::path::Path;

fn read(rel: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap_or_else(|_| panic!("{rel} manquant"))
}

#[test]
fn unit_starts_serve_at_boot_and_restarts_on_failure() {
    let u = read("deploy/rpi-monitor.service");
    for needed in ["ExecStart=/opt/rpi-monitor/rpi-monitor serve", "Restart=always", "WantedBy=multi-user.target", "TimeoutStopSec="] {
        assert!(u.contains(needed), "unité : {needed} absent");
    }
}

#[test]
fn unit_configures_port_by_environment_not_by_code() {
    let u = read("deploy/rpi-monitor.service");
    assert!(u.contains("RPI_MONITOR_PORT="), "le port se règle par variable d'environnement dans l'unité");
    let exec = u.lines().find(|l| l.starts_with("ExecStart=")).unwrap();
    assert!(!exec.contains("--port"), "pas de port figé dans ExecStart");
}

#[test]
fn everything_lives_in_one_directory_and_the_password_is_not_in_the_unit() {
    let u = read("deploy/rpi-monitor.service");
    assert!(u.contains("ReadWritePaths=/opt/rpi-monitor"), "seul dossier inscriptible : celui de l'exécutable");
    assert!(!u.contains("/etc/rpi-monitor") && !u.contains("LoadCredential") && !u.contains("StateDirectory"));
    assert!(!u.to_lowercase().contains("password=") && !u.contains("--no-auth"));
}

#[test]
fn unit_is_hardened_and_can_read_the_pi_sensors() {
    let u = read("deploy/rpi-monitor.service");
    for needed in ["User=rpi-monitor", "NoNewPrivileges=yes", "ProtectSystem=strict", "ProtectHome=yes", "SupplementaryGroups=video"] {
        assert!(u.contains(needed), "unité : {needed} absent");
    }
    assert!(!u.contains("PrivateDevices=yes"), "vcgencmd a besoin de /dev/vcio");
}

#[test]
fn install_script_is_valid_shell_and_never_embeds_a_password() {
    let s = read("scripts/install-service.sh");
    assert!(s.starts_with("#!/bin/sh"));
    assert!(s.contains("chmod 600 /opt/rpi-monitor/password") && s.contains("systemctl enable --now rpi-monitor"));
    assert!(!s.contains("/etc/rpi-monitor"));
    let ok = std::process::Command::new("sh").arg("-n").arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/install-service.sh")).status().unwrap();
    assert!(ok.success());
}

#[test]
fn post_commit_hook_rebuilds_the_pi_binary_without_ever_failing_the_commit() {
    use std::os::unix::fs::PermissionsExt;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".githooks/post-commit");
    let h = read(".githooks/post-commit");
    assert!(h.starts_with("#!/bin/sh") && h.contains("scripts/build-pi.sh"));
    assert!(h.contains("|| ") , "un échec de build doit être signalé sans faire échouer le hook");
    assert!(fs::metadata(&path).unwrap().permissions().mode() & 0o111 != 0, "le hook doit être exécutable");
    assert!(std::process::Command::new("sh").arg("-n").arg(&path).status().unwrap().success());
}

#[test]
fn build_script_puts_everything_to_copy_in_dist() {
    let s = read("scripts/build-pi.sh");
    assert!(s.contains("deploy/rpi-monitor.service") && s.contains("scripts/install-service.sh"), "dist/ doit contenir l'unité et le script d'installation");
    assert!(s.contains("dist/rpi-monitor"));
}

#[test]
fn install_script_refuses_a_binary_without_the_serve_command() {
    let s = read("scripts/install-service.sh");
    assert!(s.contains("--help") && s.contains("serve"), "le script doit vérifier que le binaire connaît `serve`");
    let check = s.find("--help").unwrap();
    assert!(check < s.find("install -m 755").unwrap(), "vérifier avant d'installer");
}
