//! Interface Web : contrôles de structure et logique JS (Node, ignoré si absent).

use std::fs;
use std::path::Path;
use std::process::Command;

fn web(file: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("web").join(file)).unwrap()
}

#[test]
fn page_is_responsive_and_loads_only_local_files() {
    let html = web("index.html");
    assert!(html.contains("name=\"viewport\"") && html.contains("width=device-width"));
    assert!(html.contains("lang=\"fr\"") && html.contains("/app.js") && html.contains("/styles.css"));
}

#[test]
fn no_external_resource_is_needed() {
    // le Pi est souvent sur un réseau sans accès aux CDN ; la CSP du serveur l'interdit de toute façon
    for f in ["index.html", "styles.css", "app.js"] {
        let text = web(f);
        assert!(!text.contains("http://") && !text.contains("https://") && !text.contains("//cdn"), "{f}");
    }
}

#[test]
fn the_page_only_calls_routes_the_api_serves() {
    let known = ["/api/v1/health", "/api/v1/metrics/current", "/api/v1/metrics/history", "/api/v1/metrics/stats", "/api/v1/metrics/events", "/api/v1/login", "/api/v1/logout"];
    let js = web("app.js");
    let mut found = 0;
    for (i, _) in js.match_indices("/api/") {
        let route: String = js[i..].chars().take_while(|c| !matches!(c, '\'' | '"' | '`' | '?' | '$' | ' ' | ')')).collect();
        assert!(known.contains(&route.as_str()), "route inconnue côté serveur : {route}");
        found += 1;
    }
    assert!(found >= 5, "la page doit appeler l'API");
}

#[test]
fn page_never_writes_html_from_data() {
    // les valeurs viennent du serveur : textContent seulement, jamais innerHTML
    assert!(!web("app.js").contains("innerHTML"));
}

#[test]
fn js_logic_passes_its_node_tests() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("web");
    let Ok(out) = Command::new("node").arg(dir.join("app.test.js")).output() else {
        eprintln!("node absent : tests JS ignorés");
        return;
    };
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

#[test]
fn page_shows_which_build_is_running() {
    assert!(web("index.html").contains("id=\"version\""));
    let js = web("app.js");
    assert!(js.contains("/api/v1/health") && js.contains("version"));
}
