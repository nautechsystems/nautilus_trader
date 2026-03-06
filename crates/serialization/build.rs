fn main() {
    println!("cargo::rustc-check-cfg=cfg(docsrs)");

    #[cfg(feature = "capnp")]
    {
        compile_capnp_schemas();
    }
}

#[cfg(feature = "capnp")]
fn compile_capnp_schemas() {
    use std::{env, path::PathBuf};

    // Skip schema compilation when building docs (docs.rs doesn't have capnp compiler)
    if env::var("DOCS_RS").is_ok() {
        println!("cargo:rustc-cfg=docsrs");
        return;
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = PathBuf::from(&manifest_dir);

    // Schemas are bundled with the crate for self-contained builds
    let schema_dir = manifest_path.join("schemas/capnp");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));

    println!("cargo:rerun-if-changed={}", schema_dir.display());

    // Collect all .capnp files
    let schema_files: Vec<PathBuf> = walkdir::WalkDir::new(&schema_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s == "capnp")
        })
        .map(|e| e.path().to_path_buf())
        .collect();

    if schema_files.is_empty() {
        println!("cargo:warning=No Cap'n Proto schema files found");
        return;
    }

    // Compile all schema files
    let mut command = capnpc::CompilerCommand::new();
    command.src_prefix(&schema_dir).output_path(&out_dir);

    {
        let import_path = &(&schema_dir);
        command.import_path(import_path);
    }

    for file in &schema_files {
        command.file(file);
    }

    command
        .run()
        .expect("Failed to compile Cap'n Proto schemas");

    println!("Cap'n Proto schemas compiled to: {}", out_dir.display());
}
