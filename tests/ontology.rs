//! L'ontologie (`docs/ontology.md`) ne doit pas dériver du code : chaque fichier et chaque
//! symbole cité dans la colonne « Où dans le code » doit exister.

use std::fs;
use std::path::Path;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// (terme, éléments entre accents graves de la colonne « Où dans le code »)
fn rows() -> Vec<(String, Vec<String>)> {
    let text = fs::read_to_string(root().join("docs/ontology.md")).expect("docs/ontology.md");
    text.lines()
        .filter(|l| l.starts_with('|'))
        .map(|l| l.split('|').map(str::trim).collect::<Vec<_>>())
        .filter(|c| c.len() >= 5 && c[1] != "Terme" && !c[1].starts_with("---"))
        .map(|c| {
            let refs = c[3].split('`').skip(1).step_by(2).map(String::from).collect();
            (c[1].to_string(), refs)
        })
        .collect()
}

#[test]
fn ontology_lists_the_business_terms() {
    let terms: Vec<String> = rows().into_iter().map(|(t, _)| t).collect();
    for expected in ["Indicateur", "Historique", "Throttling", "Consommation", "Mode démo"] {
        assert!(terms.iter().any(|t| t == expected), "terme absent : {expected}");
    }
}

#[test]
fn every_file_cited_in_the_ontology_exists() {
    for (term, refs) in rows() {
        let files: Vec<&String> = refs.iter().filter(|r| r.starts_with("src/")).collect();
        assert!(!files.is_empty(), "« {term} » ne cite aucun fichier");
        for f in files {
            assert!(root().join(f).is_file(), "« {term} » cite {f}, qui n'existe pas");
        }
    }
}

#[test]
fn every_symbol_cited_in_the_ontology_exists_in_the_cited_files() {
    for (term, refs) in rows() {
        let sources: String = refs
            .iter()
            .filter(|r| r.starts_with("src/"))
            .filter_map(|f| fs::read_to_string(root().join(f)).ok())
            .collect();
        for sym in refs.iter().filter(|r| !r.starts_with("src/")) {
            let name = sym.rsplit("::").next().unwrap_or(sym);
            assert!(sources.contains(name), "« {term} » cite {sym}, introuvable dans ses fichiers");
        }
    }
}
