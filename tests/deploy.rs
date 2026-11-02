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
