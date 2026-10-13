use std::path::PathBuf;

fn crate_src_dir(crate_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(crate_name)
}

fn collect_rs(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("read src dir");
    for e in entries {
        let p = e.expect("entry").path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            out.push(p);
        }
    }
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let code = match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        };
        out.push_str(code);
        out.push('\n');
    }
    out
}

#[test]
fn no_forbidden_imports_in_domain_and_datasources() {
    let forbidden = ["irondb_facade", "irondb_bridge", "tauri::State", "headless"];
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest.parent().unwrap().parent().unwrap().to_path_buf();
    for crate_name in ["crates/irondb-domain/src", "crates/irondb-datasources/src"] {
        let dir = workspace_root.join(crate_name);
        let mut files = vec![];
        collect_rs(&dir, &mut files);
        assert!(!files.is_empty(), "no sources in {crate_name}");
        for f in files {
            let src = std::fs::read_to_string(&f).expect("read");
            let code = strip_comments(&src);
            for pat in forbidden {
                assert!(
                    !code.contains(pat),
                    "forbidden import {pat} in {}",
                    f.display()
                );
            }
        }
    }
    let _ = crate_src_dir;
}
