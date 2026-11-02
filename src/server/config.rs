//! Configuration du service : défaut < variable d'environnement < ligne de commande.

use crate::cli::ServeOptions;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 8765;
pub const PASSWORD_FILE: &str = "password";

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

/// `base` : dossier de l'exécutable, où vivent par défaut le mot de passe (`password`) et la
/// sauvegarde (`history.json`) : tout le service tient dans un seul dossier.
pub fn resolve(o: ServeOptions, env: &dyn Fn(&str) -> Option<String>, base: &Path) -> Result<Config, String> {
    let port = match pick(o.port, "--port", "RPI_MONITOR_PORT", env) {
        Some((v, src)) => parse_port(&v, &src)?,
        None => DEFAULT_PORT,
    };
    let host = match pick(o.host, "--host", "RPI_MONITOR_HOST", env) {
        Some((v, src)) => parse_host(&v, &src)?,
        None => IpAddr::V4(Ipv4Addr::LOCALHOST),
    };
    let explicit_password = pick(o.password_file, "--password-file", "RPI_MONITOR_PASSWORD_FILE", env).map(|(v, _)| PathBuf::from(v));
    if explicit_password.is_none() && !o.no_auth && !base.join(PASSWORD_FILE).is_file() {
        return Err(format!(
            "mot de passe introuvable : créer {} (droits 600), indiquer --password-file <fichier>, \
             ou --no-auth pour désactiver explicitement l'authentification",
            base.join(PASSWORD_FILE).display()
        ));
    }
    let password_file = explicit_password.unwrap_or_else(|| base.join(PASSWORD_FILE));
    let data_dir = pick(o.data_dir, "--data-dir", "RPI_MONITOR_DATA_DIR", env).map(|(v, _)| PathBuf::from(v)).unwrap_or_else(|| base.to_path_buf());
    Ok(Config { addr: SocketAddr::new(host, port), demo: o.demo, no_auth: o.no_auth, password_file: Some(password_file), data_dir: Some(data_dir) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    fn opts() -> ServeOptions {
        ServeOptions { no_auth: true, ..Default::default() }
    }

    /// Dossier de l'exécutable factice ; `with_password` y crée le fichier `password`.
    fn base(name: &str, with_password: bool) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rpimon-cfg-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        if with_password {
            std::fs::write(d.join("password"), "x").unwrap();
        }
        d
    }

    fn resolve_in(o: ServeOptions, e: &dyn Fn(&str) -> Option<String>) -> Result<Config, String> {
        resolve(o, e, Path::new("/opt/rpi-monitor"))
    }

    #[test]
    fn defaults_listen_on_loopback_8765() {
        let c = resolve_in(opts(), &env(&[])).unwrap();
        assert_eq!(c.addr.to_string(), "127.0.0.1:8765");
    }

    #[test]
    fn environment_overrides_default() {
        let c = resolve_in(opts(), &env(&[("RPI_MONITOR_PORT", "9000"), ("RPI_MONITOR_HOST", "0.0.0.0")])).unwrap();
        assert_eq!(c.addr.to_string(), "0.0.0.0:9000");
    }

    #[test]
    fn command_line_overrides_environment() {
        let o = ServeOptions { port: Some("9100".into()), host: Some("::1".into()), ..opts() };
        let c = resolve_in(o, &env(&[("RPI_MONITOR_PORT", "9000"), ("RPI_MONITOR_HOST", "0.0.0.0")])).unwrap();
        assert_eq!(c.addr.to_string(), "[::1]:9100");
    }

    #[test]
    fn invalid_ports_are_explicit_errors() {
        for bad in ["abc", "0", "70000", "-1", ""] {
            let o = ServeOptions { port: Some(bad.into()), ..opts() };
            let e = resolve_in(o, &env(&[])).unwrap_err();
            assert!(e.contains("port") && e.contains(&format!("« {bad} »")), "{e}");
        }
        let e = resolve_in(opts(), &env(&[("RPI_MONITOR_PORT", "x")])).unwrap_err();
        assert!(e.contains("RPI_MONITOR_PORT"), "{e}");
    }

    #[test]
    fn invalid_host_is_an_error_and_localhost_is_accepted() {
        let o = ServeOptions { host: Some("pas une ip".into()), ..opts() };
        assert!(resolve_in(o, &env(&[])).unwrap_err().contains("hôte"));
        let o = ServeOptions { host: Some("localhost".into()), ..opts() };
        assert_eq!(resolve_in(o, &env(&[])).unwrap().addr.ip().to_string(), "127.0.0.1");
    }

    #[test]
    fn password_and_data_live_next_to_the_executable_by_default() {
        let b = base("default", true);
        let c = resolve(ServeOptions::default(), &env(&[]), &b).unwrap();
        assert_eq!(c.password_file, Some(b.join("password")));
        assert_eq!(c.data_dir, Some(b));
        assert!(!c.no_auth);
    }

    #[test]
    fn missing_password_next_to_the_executable_is_an_explicit_error() {
        let b = base("nopw", false);
        let e = resolve(ServeOptions::default(), &env(&[]), &b).unwrap_err();
        assert!(e.contains(&b.join("password").display().to_string()) && e.contains("--no-auth"), "{e}");
    }

    #[test]
    fn explicit_password_file_and_data_dir_override_the_executable_directory() {
        let b = base("override", false);
        let o = ServeOptions { password_file: Some("/etc/pw".into()), data_dir: Some("/d".into()), ..Default::default() };
        let c = resolve(o, &env(&[]), &b).unwrap();
        assert_eq!((c.password_file, c.data_dir), (Some(PathBuf::from("/etc/pw")), Some(PathBuf::from("/d"))));
        let e = env(&[("RPI_MONITOR_PASSWORD_FILE", "/a"), ("RPI_MONITOR_DATA_DIR", "/e")]);
        let c = resolve(ServeOptions::default(), &e, &b).unwrap();
        assert_eq!((c.password_file, c.data_dir), (Some(PathBuf::from("/a")), Some(PathBuf::from("/e"))));
        let o = ServeOptions { data_dir: Some("/c".into()), ..opts() };
        assert_eq!(resolve(o, &e, &b).unwrap().data_dir, Some(PathBuf::from("/c")));
    }

    #[test]
    fn no_auth_needs_no_password_file() {
        let b = base("noauth", false);
        let c = resolve(opts(), &env(&[]), &b).unwrap();
        assert!(c.no_auth);
    }
}
