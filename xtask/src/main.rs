use std::path::Path;

use git_leviathan_plugin_api::schema::stubs;

mod plugin;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen-stubs") => gen_stubs(),
        Some("gen-docs") => gen_docs(),
        Some("gen-schema") => gen_schema(),
        Some("gen-all") => {
            gen_stubs()?;
            gen_docs()?;
            gen_schema()
        }
        Some("plugin") => plugin::run(&args[2..]),
        _ => {
            eprintln!("usage: cargo xtask <gen-stubs|gen-docs|gen-schema|gen-all|plugin>");
            std::process::exit(2);
        }
    }
}

fn gen_stubs() -> Result<(), Box<dyn std::error::Error>> {
    let content = stubs::emit_stubs();

    let target_dir = Path::new("target/plugin-stubs");
    std::fs::create_dir_all(target_dir)?;
    let target_path = target_dir.join("leviathan.lua");
    std::fs::write(&target_path, &content)?;
    println!("wrote {}", target_path.display());

    let generated_dir = Path::new("docs/generated/lua");
    std::fs::create_dir_all(generated_dir)?;
    let generated_path = generated_dir.join("leviathan-v1.lua");
    std::fs::write(&generated_path, content)?;
    println!("wrote {}", generated_path.display());

    let docs_dir = Path::new("docs/lua");
    std::fs::create_dir_all(docs_dir)?;
    let docs_path = docs_dir.join("leviathan-v1.lua");
    let content = std::fs::read_to_string(&generated_path)?;
    std::fs::write(&docs_path, content)?;
    println!("wrote {}", docs_path.display());
    Ok(())
}

fn gen_docs() -> Result<(), Box<dyn std::error::Error>> {
    let docs = stubs::emit_api_docs();

    let public_docs_dir = Path::new("docs/plugins/api");
    std::fs::create_dir_all(public_docs_dir)?;
    for (filename, content) in &docs {
        std::fs::write(public_docs_dir.join(filename), content)?;
    }
    println!("wrote docs to {}", public_docs_dir.display());

    let generated_dir = Path::new("docs/generated/api");
    std::fs::create_dir_all(generated_dir)?;
    for (filename, content) in &docs {
        std::fs::write(generated_dir.join(filename), content)?;
    }
    let reference_path = Path::new("docs/generated/plugin-api-v1.md");
    let reference = stubs::emit_api_reference();
    std::fs::write(reference_path, &reference)?;
    std::fs::write(Path::new("docs/plugin-api-v1.md"), reference)?;
    println!("wrote docs to {}", generated_dir.display());
    println!("wrote {}", reference_path.display());
    Ok(())
}

fn gen_schema() -> Result<(), Box<dyn std::error::Error>> {
    let schema_dir = Path::new("docs/generated/schema");
    std::fs::create_dir_all(schema_dir)?;

    write_schema(
        schema_dir,
        "api-descriptor.json",
        stubs::emit_api_descriptor_json(),
    )?;
    write_schema(
        schema_dir,
        "manifest.schema.json",
        stubs::emit_manifest_schema_json(),
    )?;
    write_schema(
        schema_dir,
        "widget.schema.json",
        stubs::emit_widget_schema_json(),
    )?;
    write_schema(
        schema_dir,
        "widgets.json",
        stubs::emit_widget_descriptor_json(),
    )?;
    write_schema(
        schema_dir,
        "regions.json",
        stubs::emit_region_descriptor_json(),
    )?;
    write_schema(
        schema_dir,
        "devtools-extension-points.json",
        stubs::emit_devtools_extension_point_summary_json(),
    )?;
    println!("wrote schemas to {}", schema_dir.display());
    Ok(())
}

fn write_schema(
    schema_dir: &Path,
    filename: &str,
    content: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = schema_dir.join(filename);
    std::fs::write(&path, content)?;
    println!("wrote {}", path.display());
    Ok(())
}
