//! Haddock-style link resolution for documentation

use crate::docgen::ir::{ItemKind, LinkTarget};
use std::collections::BTreeMap;

/// Link resolver implementing Haddock-style resolution
pub struct LinkResolver<'a> {
    export_map: &'a BTreeMap<String, String>,
}

impl<'a> LinkResolver<'a> {
    pub fn new(export_map: &'a BTreeMap<String, String>) -> Self {
        Self { export_map }
    }

    /// Get the export map for resolving type links
    pub fn export_map(&self) -> &BTreeMap<String, String> {
        self.export_map
    }

    /// Resolve a link using Haddock rules:
    /// 1. If exported from current_module (incl. re-exports): link to current
    /// 2. Else if exported from public module: link there
    /// 3. Else (private module only): no link
    ///
    /// Returns (doc_module, item_kind) if linkable, None otherwise
    pub fn resolve_link(&self, item_fqn: &str, current_module: &str) -> Option<(String, ItemKind)> {
        // Check if item is exported from current module
        if let Some(export_module) = self.export_map.get(item_fqn) {
            if export_module == current_module {
                // Item is exported from current module - link to it
                return Some((current_module.to_string(), ItemKind::Class)); // TODO: determine actual kind
            }

            // Check if the export module is public
            if !self.is_private_module(export_module) {
                return Some((export_module.clone(), ItemKind::Class)); // TODO: determine actual kind
            }
        }

        // Item is only in private modules - no link
        None
    }

    /// Check if a module is private (has underscore segments)
    fn is_private_module(&self, module_name: &str) -> bool {
        module_name.split('.').any(|part| part.starts_with('_'))
    }

    /// Resolve a link to a class attribute (e.g., "C.C1" → link to C1 variant of class C)
    ///
    /// This is used for linking to enum variants or class attributes in default values.
    pub fn resolve_attribute_link(
        &self,
        class_name: &str,
        attribute_name: &str,
        current_module: &str,
    ) -> Option<LinkTarget> {
        // 1. Resolve the class itself
        let (doc_module, kind) = self.resolve_link(class_name, current_module)?;

        // 2. Verify it's a class
        if kind != ItemKind::Class {
            return None;
        }

        // 3. Build FQN with attribute: "module.Class.attribute"
        let fqn = format!("{}.{}", class_name, attribute_name);

        Some(LinkTarget {
            fqn,
            doc_module,
            kind: ItemKind::Class,
            attribute: Some(attribute_name.to_string()),
        })
    }
}
