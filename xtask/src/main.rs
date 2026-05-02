use std::path::Path;

use git_leviathan_plugin_api::schema::stubs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen-stubs") => gen_stubs(),
        Some("gen-docs") => {
            println!("gen-docs: populated in Task 6.3");
            Ok(())
        }
        _ => {
            eprintln!("usage: cargo xtask <gen-stubs|gen-docs>");
            std::process::exit(2);
        }
    }
}

fn gen_stubs() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = Path::new("target/plugin-stubs");
    std::fs::create_dir_all(out_dir)?;
    let path = out_dir.join("leviathan.lua");
    let content = stubs::emit_stubs();
    std::fs::write(&path, content)?;
    println!("wrote {}", path.display());
    Ok(())
}
