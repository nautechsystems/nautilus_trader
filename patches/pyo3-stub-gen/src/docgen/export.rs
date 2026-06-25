//! Export resolution for determining which items are publicly accessible

use crate::docgen::util::is_hidden_module;
use crate::generate::Module;
use std::collections::{BTreeMap, BTreeSet};

/// Resolver for determining which items are exported from modules
pub struct ExportResolver<'a> {
    modules: &'a BTreeMap<String, Module>,
}

impl<'a> ExportResolver<'a> {
    pub fn new(modules: &'a BTreeMap<String, Module>) -> Self {
        Self { modules }
    }

    /// Resolve which items are exported from a module
    /// Rules:
    /// 1. If __all__ exists (re-exports or verbatim): use it
    /// 2. Otherwise: all non-underscore items
    /// 3. Add re-exported items
    /// 4. Add verbatim entries
    /// 5. Remove excluded items
    pub fn resolve_exports(&self, module: &Module) -> BTreeSet<String> {
        let mut exports = BTreeSet::new();

        // Add directly-defined public items
        for name in module.function.keys() {
            if !name.starts_with('_') {
                exports.insert((*name).to_string());
            }
        }

        for class in module.class.values() {
            if !class.name.starts_with('_') {
                exports.insert(class.name.to_string());
            }
        }

        for enum_def in module.enum_.values() {
            if !enum_def.name.starts_with('_') {
                exports.insert(enum_def.name.to_string());
            }
        }

        for type_alias in module.type_aliases.keys() {
            if !type_alias.starts_with('_') {
                exports.insert((*type_alias).to_string());
            }
        }

        for var in module.variables.keys() {
            if !var.starts_with('_') {
                exports.insert((*var).to_string());
            }
        }

        for submod in &module.submodules {
            if !submod.starts_with('_') {
                exports.insert(submod.to_string());
            }
        }

        // Add items from module re-exports (from reexport_module_members!)
        for re_export in &module.module_re_exports {
            exports.extend(re_export.items.iter().cloned());
        }

        // Add verbatim entries (allows explicitly exporting underscore items)
        exports.extend(module.verbatim_all_entries.iter().cloned());

        // Remove excluded items
        for excluded in &module.excluded_all_entries {
            exports.remove(excluded);
        }

        exports
    }

    /// Build global map: item_fqn → module_where_exported
    /// For re-exports: map to re-exporting module
    pub fn build_export_map(&self) -> BTreeMap<String, String> {
        let mut export_map = BTreeMap::new();

        for (module_name, module) in self.modules {
            // Skip hidden modules (any component starts with '_')
            if is_hidden_module(module_name) {
                continue;
            }

            let exports = self.resolve_exports(module);

            for item_name in exports {
                let fqn = format!("{}.{}", module_name, item_name);
                export_map.insert(fqn, module_name.clone());
            }
        }

        export_map
    }
}
