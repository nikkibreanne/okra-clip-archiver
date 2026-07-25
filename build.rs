// Ensure web/dist exists so the rust-embed derive compiles even before the React
// app has been built (CI runs `npm run build` first, which overwrites this
// placeholder; a dev who forgot gets a helpful message instead of a build error).
use std::{fs, path::Path};

fn main() {
    let dist = Path::new("web/dist");
    let _ = fs::create_dir_all(dist);
    let index = dist.join("index.html");
    if !index.exists() {
        let _ = fs::write(
            &index,
            "<!doctype html><meta charset=utf-8><title>okra-clip-archiver</title>\
             <body style=\"font-family:system-ui,sans-serif;background:#14151a;color:#e6e6ea;padding:2rem\">\
             <h1>UI not built</h1><p>Run <code>npm --prefix web install &amp;&amp; npm --prefix web run build</code>, then rebuild.</p></body>",
        );
    }
    println!("cargo:rerun-if-changed=web/dist");
}
