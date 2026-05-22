// Tauri runtime setup + Contract 08 §Constraints build-time check:
// "A markdown file exceeding a sane size limit MUST raise a build-time
// warning." We walk knowledgebase/articles/ and emit cargo::warning for
// any file over the limit so a maintainer notices at `cargo build` time
// rather than during a runtime scan.

use std::path::Path;

const KB_ARTICLE_MAX_BYTES: u64 = 64 * 1024;

fn main() {
    warn_oversized_kb_articles();
    tauri_build::build();
}

fn warn_oversized_kb_articles() {
    let dir = Path::new("knowledgebase").join("articles");
    if !dir.is_dir() {
        return;
    }
    // Re-run when articles change so the warning stays in sync.
    println!("cargo:rerun-if-changed=knowledgebase/articles");
    println!("cargo:rerun-if-changed=knowledgebase/mappings.json");

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let len = match std::fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        if len > KB_ARTICLE_MAX_BYTES {
            println!(
                "cargo:warning=knowledgebase article '{}' is {} bytes (>{}); consider trimming",
                path.display(),
                len,
                KB_ARTICLE_MAX_BYTES
            );
        }
    }
}
