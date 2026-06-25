//! JSON rendering and Sphinx extension embedding

use crate::docgen::ir::DocPackage;
use crate::Result;
use std::path::Path;

/// Render DocPackage to JSON string
pub fn render_to_json(package: &DocPackage) -> Result<String> {
    // Normalize for deterministic output
    let mut normalized = package.clone();
    normalized.normalize();
    Ok(serde_json::to_string_pretty(&normalized)?)
}

/// Copy the embedded Sphinx extension to the output directory
pub fn copy_sphinx_extension(output_dir: &Path) -> Result<()> {
    let extension_code = include_str!("sphinx_ext.py");
    let ext_path = output_dir.join("pyo3_stub_gen_ext.py");
    std::fs::write(ext_path, extension_code)?;
    Ok(())
}

/// Generate RST files for each module
pub fn generate_module_pages(package: &DocPackage, output_dir: &Path) -> Result<()> {
    // Sort modules to ensure consistent ordering
    let mut module_names: Vec<_> = package.modules.keys().collect();
    module_names.sort();

    for module_name in module_names {
        let rst_content = format!(
            "{}\n{}\n\n.. pyo3-api:: {}\n",
            module_name,
            "=".repeat(module_name.len()),
            module_name
        );

        // Convert module name to filename: mixed.main_mod -> mixed.main_mod.rst
        let filename = format!("{}.rst", module_name);
        let file_path = output_dir.join(&filename);

        std::fs::write(file_path, rst_content)?;
    }

    Ok(())
}

/// Generate index.rst that references all module pages
pub fn generate_index_rst(
    package: &DocPackage,
    output_dir: &Path,
    config: &crate::docgen::config::DocGenConfig,
) -> Result<()> {
    let mut content = String::new();

    // Title - use configured title or default to "{package_name} API Reference"
    let title = if let Some(custom_title) = &config.index_title {
        if custom_title.is_empty() {
            "API Reference".to_string()
        } else {
            custom_title.clone()
        }
    } else {
        format!("{} API Reference", package.name)
    };

    content.push_str(&format!("{}\n{}\n\n", title, "=".repeat(title.len())));

    // Add intro message (configurable or default)
    if let Some(intro) = &config.intro_message {
        if !intro.is_empty() {
            content.push_str(intro);
            content.push_str("\n\n");
        }
        // Empty string -> skip intro entirely
    } else {
        // Default message when not configured
        content.push_str(
            "This is the API reference documentation generated from Rust code using `pyo3-stub-gen <https://github.com/Jij-Inc/pyo3-stub-gen>`_.\n\n",
        );
    }

    // Create toctree
    content.push_str(".. toctree::\n");
    content.push_str("   :maxdepth: 2\n");
    content.push_str("   :caption: Modules:\n\n");

    // Sort modules to ensure consistent ordering
    let mut module_names: Vec<_> = package.modules.keys().collect();
    module_names.sort();

    for module_name in module_names {
        content.push_str(&format!("   {}\n", module_name));
    }

    let index_path = output_dir.join("index.rst");
    std::fs::write(index_path, content)?;

    Ok(())
}
