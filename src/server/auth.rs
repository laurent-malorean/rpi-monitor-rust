//! Authentification : un mot de passe (lu dans un fichier) donne un cookie de session.

use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

pub const COOKIE: &str = "rpi_session";
pub const SESSION_S: f64 = 30.0 * 24.0 * 3600.0;
const MAX_FAILURES: usize = 5; // par minute, tous clients confondus (derrière un tunnel, tout vient de 127.0.0.1)
const FAILURE_WINDOW_S: f64 = 60.0;
const MAX_SESSIONS: usize = 64;

#[derive(Debug, PartialEq, Eq)]
pub enum Login {
    Granted(String),
    Denied,
    TooManyAttempts,
}

struct State {
    sessions: VecDeque<(String, f64)>, // jeton, expiration
    failures: VecDeque<f64>,
}

pub struct Auth {
    password: Option<String>,
    state: Mutex<State>,
}

/// Comparaison en temps constant (la durée ne révèle pas le préfixe commun).
pub fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

/// 32 octets aléatoires du système, en hexadécimal.
pub fn random_token() -> std::io::Result<String> {
    let mut buf = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

impl Auth {
    /// `None` : authentification désactivée (`--no-auth`).
    pub fn new(password: Option<String>) -> Self {
        Self { password, state: Mutex::new(State { sessions: VecDeque::new(), failures: VecDeque::new() }) }
    }

    pub fn required(&self) -> bool {
        self.password.is_some()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn login(&self, candidate: &str, now: f64) -> Login {
        let Some(password) = &self.password else { return Login::Denied };
        let mut st = self.state();
        while st.failures.front().is_some_and(|&t| now - t >= FAILURE_WINDOW_S) {
            st.failures.pop_front();
        }
        if st.failures.len() >= MAX_FAILURES {
            return Login::TooManyAttempts;
        }
        if !same(candidate, password) {
            st.failures.push_back(now);
            return Login::Denied;
        }
        let Ok(token) = random_token() else { return Login::Denied };
        st.sessions.retain(|(_, exp)| *exp > now);
        if st.sessions.len() >= MAX_SESSIONS {
            st.sessions.pop_front();
        }
        st.sessions.push_back((token.clone(), now + SESSION_S));
        Login::Granted(token)
    }

    pub fn is_valid(&self, token: &str, now: f64) -> bool {
        if self.password.is_none() {
            return true;
        }
        !token.is_empty() && self.state().sessions.iter().any(|(t, exp)| *exp > now && same(t, token))
    }

    pub fn logout(&self, token: &str) {
        self.state().sessions.retain(|(t, _)| !same(t, token));
    }
}

pub fn token_from_cookie(header: &str) -> Option<&str> {
    header
        .split(';')
        .filter_map(|c| c.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v)
        .filter(|v| !v.is_empty())
}

pub fn set_cookie(token: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}{secure}", SESSION_S as u64)
}

/// Lit le mot de passe ; refuse un fichier lisible par d'autres utilisateurs.
pub fn load_password(path: &Path) -> Result<String, String> {
    let shown = path.display();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path).map_err(|e| format!("mot de passe illisible ({shown}) : {e}"))?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(format!("droits trop ouverts sur {shown} ({:o}) : exécuter chmod 600", mode & 0o777));
        }
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("mot de passe illisible ({shown}) : {e}"))?;
    let pw = text.trim_end_matches(['\n', '\r']).to_string();
    if pw.is_empty() {
        return Err(format!("fichier de mot de passe vide : {shown}"));
    }
    Ok(pw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn auth() -> Auth {
        Auth::new(Some("s3cret-passphrase".into()))
    }

    fn token(r: Login) -> String {
        match r {
            Login::Granted(t) => t,
            other => panic!("attendu Granted, obtenu {other:?}"),
        }
    }

    #[test]
    fn right_password_grants_a_session_token() {
        let a = auth();
        let t = token(a.login("s3cret-passphrase", 100.0));
        assert_eq!(t.len(), 64);
        assert!(a.is_valid(&t, 101.0));
    }

    #[test]
    fn tokens_are_unique_and_unknown_tokens_are_refused() {
        let a = auth();
        let (t1, t2) = (token(a.login("s3cret-passphrase", 1.0)), token(a.login("s3cret-passphrase", 2.0)));
        assert_ne!(t1, t2);
        assert!(!a.is_valid("0".repeat(64).as_str(), 3.0));
        assert!(!a.is_valid("", 3.0));
    }

    #[test]
    fn wrong_password_is_denied() {
        assert_eq!(auth().login("nope", 1.0), Login::Denied);
        assert_eq!(auth().login("", 1.0), Login::Denied);
    }

    #[test]
    fn sessions_expire_after_30_days() {
        let a = auth();
        let t = token(a.login("s3cret-passphrase", 0.0));
        assert!(a.is_valid(&t, SESSION_S - 1.0));
        assert!(!a.is_valid(&t, SESSION_S + 1.0));
    }

    #[test]
    fn too_many_failures_lock_logins_even_with_the_right_password_until_they_age_out() {
        let a = auth();
        for i in 0..MAX_FAILURES {
            assert_eq!(a.login("bad", i as f64), Login::Denied);
        }
        assert_eq!(a.login("s3cret-passphrase", 10.0), Login::TooManyAttempts);
        assert!(matches!(a.login("s3cret-passphrase", 100.0), Login::Granted(_)));
    }

    #[test]
    fn disabled_auth_accepts_everything() {
        let a = Auth::new(None);
        assert!(!a.required());
        assert!(a.is_valid("", 1.0));
    }

    #[test]
    fn session_count_is_bounded() {
        let a = auth();
        let first = token(a.login("s3cret-passphrase", 0.0));
        for i in 0..(MAX_SESSIONS + 5) {
            token(a.login("s3cret-passphrase", i as f64));
        }
        assert!(!a.is_valid(&first, 1.0), "la plus ancienne session est évincée");
    }

    #[test]
    fn cookie_is_extracted_from_the_header() {
        assert_eq!(token_from_cookie("a=1; rpi_session=abc123; b=2"), Some("abc123"));
        assert_eq!(token_from_cookie("a=1"), None);
        assert_eq!(token_from_cookie("rpi_session="), None);
    }

    #[test]
    fn set_cookie_is_hardened() {
        let c = set_cookie("tok", true);
        for part in ["rpi_session=tok", "HttpOnly", "SameSite=Strict", "Path=/", "Max-Age=2592000", "Secure"] {
            assert!(c.contains(part), "{c}");
        }
        assert!(!set_cookie("tok", false).contains("Secure"));
    }

    #[test]
    fn constant_time_compare_works() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd") && !same("abc", "ab") && !same("", "a"));
    }

    fn temp_file(name: &str, content: &str, mode: u32) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rpimon-auth-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("password");
        fs::write(&p, content).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn password_file_is_read_and_trimmed() {
        let p = temp_file("ok", "un-mot-de-passe-long\n", 0o600);
        assert_eq!(load_password(&p).unwrap(), "un-mot-de-passe-long");
    }

    #[test]
    fn password_file_open_to_others_is_refused() {
        let p = temp_file("open", "x-un-mot-de-passe", 0o644);
        assert!(load_password(&p).unwrap_err().contains("droits"));
    }

    #[test]
    fn empty_or_missing_password_file_is_an_error() {
        assert!(load_password(&temp_file("empty", "\n", 0o600)).is_err());
        assert!(load_password(std::path::Path::new("/nonexistent/pw")).unwrap_err().contains("/nonexistent/pw"));
    }
}
