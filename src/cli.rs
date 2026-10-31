//! Arguments de la ligne de commande.

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run { demo: bool },
    Help,
}

pub const USAGE: &str = "Moniteur Raspberry Pi (lecture seule)\n\nUsage: rpi-monitor [--demo]\n\n  --demo   données synthétiques (automatique hors Linux)\n  -h, --help\n\nTouches : q quitter · p pause · r reset · w fenêtre 1/5/30 min";

pub fn parse(args: impl IntoIterator<Item = String>, is_linux: bool) -> Result<Command, String> {
    let mut demo = !is_linux; // hors Linux : pas de capteurs réels
    for a in args {
        match a.as_str() {
            "--demo" => demo = true,
            "-h" | "--help" => return Ok(Command::Help),
            other => return Err(format!("argument inconnu : {other}")),
        }
    }
    Ok(Command::Run { demo })
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
}
