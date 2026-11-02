//! Arguments de la ligne de commande.

/// Options brutes de `serve` ; la validation et la priorité avec l'environnement
/// sont dans `server::config`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ServeOptions {
    pub demo: bool,
    pub host: Option<String>,
    pub port: Option<String>,
    pub password_file: Option<String>,
    pub data_dir: Option<String>,
    pub no_auth: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run { demo: bool },
    Serve(ServeOptions),
    Help,
}

pub const USAGE: &str = "Moniteur Raspberry Pi (lecture seule)

Usage:
  rpi-monitor [tui] [--demo]     interface terminal autonome
  rpi-monitor serve [options]    service permanent : collecte + API + interface Web

Options communes :
  --demo   données synthétiques (automatique hors Linux)
  -h, --help

Options de serve (la CLI l'emporte sur l'environnement, qui l'emporte sur le défaut) :
  --host <ip>            adresse d'écoute (RPI_MONITOR_HOST, défaut 127.0.0.1)
  --port <n>             port d'écoute (RPI_MONITOR_PORT, défaut 8765)
  --password-file <f>    fichier du mot de passe (RPI_MONITOR_PASSWORD_FILE ;
                         défaut : « password » à côté de l'exécutable)
  --no-auth              pas de mot de passe (déconseillé : un tunnel rend le service public)
  --data-dir <d>         dossier de la sauvegarde de l'historique (RPI_MONITOR_DATA_DIR ;
                         défaut : le dossier de l'exécutable)

Touches TUI : q quitter · p pause · r reset · w fenêtre 1/5/30 min";

pub fn parse(args: impl IntoIterator<Item = String>, is_linux: bool) -> Result<Command, String> {
    let mut args = args.into_iter().peekable();
    let serve = args.peek().is_some_and(|a| a == "serve");
    if serve || args.peek().is_some_and(|a| a == "tui") {
        args.next();
    }
    let mut demo = !is_linux; // hors Linux : pas de capteurs réels
    let mut opts = ServeOptions::default();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--demo" => demo = true,
            "-h" | "--help" => return Ok(Command::Help),
            "--no-auth" if serve => opts.no_auth = true,
            "--host" | "--port" | "--password-file" | "--data-dir" if serve => {
                let v = args.next().ok_or_else(|| format!("{a} demande une valeur"))?;
                match a.as_str() {
                    "--host" => opts.host = Some(v),
                    "--port" => opts.port = Some(v),
                    "--password-file" => opts.password_file = Some(v),
                    _ => opts.data_dir = Some(v),
                }
            }
            other => return Err(format!("argument inconnu : {other}")),
        }
    }
    if serve {
        opts.demo = demo;
        Ok(Command::Serve(opts))
    } else {
        Ok(Command::Run { demo })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str], linux: bool) -> Result<Command, String> {
        parse(args.iter().map(|s| s.to_string()), linux)
    }

    #[test]
    fn real_sensors_on_linux_by_default() {
        assert_eq!(p(&[], true), Ok(Command::Run { demo: false }));
    }

    #[test]
    fn demo_is_automatic_outside_linux() {
        assert_eq!(p(&[], false), Ok(Command::Run { demo: true }));
    }

    #[test]
    fn demo_flag_forces_demo() {
        assert_eq!(p(&["--demo"], true), Ok(Command::Run { demo: true }));
    }

    #[test]
    fn help_flags() {
        assert_eq!(p(&["-h"], true), Ok(Command::Help));
        assert_eq!(p(&["--help"], false), Ok(Command::Help));
    }

    #[test]
    fn unknown_argument_is_an_error_naming_it() {
        assert!(p(&["--nope"], true).unwrap_err().contains("--nope"));
    }

    fn serve(args: &[&str], linux: bool) -> ServeOptions {
        match p(args, linux) {
            Ok(Command::Serve(o)) => o,
            other => panic!("attendu Serve, obtenu {other:?}"),
        }
    }

    #[test]
    fn serve_subcommand_defaults() {
        let o = serve(&["serve"], true);
        assert_eq!(o, ServeOptions::default());
        assert!(serve(&["serve"], false).demo);
    }

    #[test]
    fn serve_options_are_kept_raw_for_later_validation() {
        let o = serve(
            &["serve", "--demo", "--host", "0.0.0.0", "--port", "9090", "--password-file", "/x/pw", "--data-dir", "/d", "--no-auth"],
            true,
        );
        assert!(o.demo && o.no_auth);
        assert_eq!(o.host.as_deref(), Some("0.0.0.0"));
        assert_eq!(o.port.as_deref(), Some("9090"));
        assert_eq!(o.password_file.as_deref(), Some("/x/pw"));
        assert_eq!(o.data_dir.as_deref(), Some("/d"));
    }

    #[test]
    fn serve_option_without_value_is_an_error_naming_it() {
        assert!(p(&["serve", "--port"], true).unwrap_err().contains("--port"));
    }

    #[test]
    fn serve_rejects_unknown_option() {
        assert!(p(&["serve", "--nope"], true).unwrap_err().contains("--nope"));
    }

    #[test]
    fn tui_subcommand_is_the_standalone_tui() {
        assert_eq!(p(&["tui"], true), Ok(Command::Run { demo: false }));
        assert_eq!(p(&["tui", "--demo"], true), Ok(Command::Run { demo: true }));
    }

    #[test]
    fn serve_help() {
        assert_eq!(p(&["serve", "--help"], true), Ok(Command::Help));
    }
}
