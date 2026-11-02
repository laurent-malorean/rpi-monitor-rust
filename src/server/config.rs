//! Configuration du service : défaut < variable d'environnement < ligne de commande.

use crate::cli::ServeOptions;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 8765;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub addr: SocketAddr,
    pub demo: bool,
    pub no_auth: bool,
    pub password_file: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
}

/// Valeur de la CLI, sinon de la variable d'environnement, avec le nom de la source.
fn pick(cli: Option<String>, cli_name: &str, var: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<(String, String)> {
    cli.map(|v| (v, cli_name.to_string())).or_else(|| env(var).map(|v| (v, var.to_string())))
}

fn parse_port(v: &str, source: &str) -> Result<u16, String> {
    match v.trim().parse::<u16>() {
        Ok(p) if p > 0 => Ok(p),
        _ => Err(format!("port invalide : « {v} » ({source}) ; attendu un entier de 1 à 65535")),
    }
}

fn parse_host(v: &str, source: &str) -> Result<IpAddr, String> {
    if v == "localhost" {
        return Ok(IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
    v.parse().map_err(|_| format!("hôte invalide : « {v} » ({source}) ; attendu une adresse IP (ex. 127.0.0.1, 0.0.0.0)"))
}

pub fn resolve(o: ServeOptions, env: &dyn Fn(&str) -> Option<String>) -> Result<Config, String> {
    let port = match pick(o.port, "--port", "RPI_MONITOR_PORT", env) {
        Some((v, src)) => parse_port(&v, &src)?,
        None => DEFAULT_PORT,
    };
    let host = match pick(o.host, "--host", "RPI_MONITOR_HOST", env) {
        Some((v, src)) => parse_host(&v, &src)?,
        None => IpAddr::V4(Ipv4Addr::LOCALHOST),
    };
    let password_file = pick(o.password_file, "--password-file", "RPI_MONITOR_PASSWORD_FILE", env)
        .map(|(v, _)| PathBuf::from(v))
        .or_else(|| env("CREDENTIALS_DIRECTORY").map(|d| PathBuf::from(d).join("password")));
    if password_file.is_none() && !o.no_auth {
        return Err("aucun mot de passe : indiquer --password-file <fichier> (ou RPI_MONITOR_PASSWORD_FILE), \
                    ou --no-auth pour désactiver explicitement l'authentification"
            .into());
    }
    let data_dir = pick(o.data_dir, "--data-dir", "RPI_MONITOR_DATA_DIR", env)
        .map(|(v, _)| PathBuf::from(v))
        .or_else(|| env("STATE_DIRECTORY").map(PathBuf::from));
    Ok(Config { addr: SocketAddr::new(host, port), demo: o.demo, no_auth: o.no_auth, password_file, data_dir })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    fn opts() -> ServeOptions {
        ServeOptions { no_auth: true, ..Default::default() }
    }

    #[test]
    fn defaults_listen_on_loopback_8765() {
        let c = resolve(opts(), &env(&[])).unwrap();
        assert_eq!(c.addr.to_string(), "127.0.0.1:8765");
        assert!(c.data_dir.is_none());
    }

    #[test]
    fn environment_overrides_default() {
        let c = resolve(opts(), &env(&[("RPI_MONITOR_PORT", "9000"), ("RPI_MONITOR_HOST", "0.0.0.0")])).unwrap();
        assert_eq!(c.addr.to_string(), "0.0.0.0:9000");
    }

    #[test]
    fn command_line_overrides_environment() {
        let o = ServeOptions { port: Some("9100".into()), host: Some("::1".into()), ..opts() };
        let c = resolve(o, &env(&[("RPI_MONITOR_PORT", "9000"), ("RPI_MONITOR_HOST", "0.0.0.0")])).unwrap();
        assert_eq!(c.addr.to_string(), "[::1]:9100");
    }

    #[test]
    fn invalid_ports_are_explicit_errors() {
        for bad in ["abc", "0", "70000", "-1", ""] {
            let o = ServeOptions { port: Some(bad.into()), ..opts() };
            let e = resolve(o, &env(&[])).unwrap_err();
            assert!(e.contains("port") && e.contains(&format!("« {bad} »")), "{e}");
        }
        let e = resolve(opts(), &env(&[("RPI_MONITOR_PORT", "x")])).unwrap_err();
        assert!(e.contains("RPI_MONITOR_PORT"), "{e}");
    }

    #[test]
    fn invalid_host_is_an_error_and_localhost_is_accepted() {
        let o = ServeOptions { host: Some("pas une ip".into()), ..opts() };
        assert!(resolve(o, &env(&[])).unwrap_err().contains("hôte"));
        let o = ServeOptions { host: Some("localhost".into()), ..opts() };
        assert_eq!(resolve(o, &env(&[])).unwrap().addr.ip().to_string(), "127.0.0.1");
    }

    #[test]
    fn a_password_is_required_unless_auth_is_disabled_explicitly() {
        let e = resolve(ServeOptions::default(), &env(&[])).unwrap_err();
        assert!(e.contains("--password-file") && e.contains("--no-auth"), "{e}");
        let o = ServeOptions { password_file: Some("/etc/pw".into()), ..Default::default() };
        let c = resolve(o, &env(&[])).unwrap();
        assert_eq!(c.password_file, Some(PathBuf::from("/etc/pw")));
        assert!(!c.no_auth);
    }

    #[test]
    fn password_file_from_environment_or_systemd_credentials() {
        let c = resolve(ServeOptions::default(), &env(&[("RPI_MONITOR_PASSWORD_FILE", "/a")])).unwrap();
        assert_eq!(c.password_file, Some(PathBuf::from("/a")));
        let c = resolve(ServeOptions::default(), &env(&[("CREDENTIALS_DIRECTORY", "/run/cred")])).unwrap();
        assert_eq!(c.password_file, Some(PathBuf::from("/run/cred/password")));
    }

    #[test]
    fn data_dir_from_cli_environment_or_systemd_state_directory() {
        let c = resolve(opts(), &env(&[("STATE_DIRECTORY", "/var/lib/rpi-monitor")])).unwrap();
        assert_eq!(c.data_dir, Some(PathBuf::from("/var/lib/rpi-monitor")));
        let c = resolve(opts(), &env(&[("RPI_MONITOR_DATA_DIR", "/e"), ("STATE_DIRECTORY", "/s")])).unwrap();
        assert_eq!(c.data_dir, Some(PathBuf::from("/e")));
        let o = ServeOptions { data_dir: Some("/c".into()), ..opts() };
        assert_eq!(resolve(o, &env(&[("RPI_MONITOR_DATA_DIR", "/e")])).unwrap().data_dir, Some(PathBuf::from("/c")));
    }
}
