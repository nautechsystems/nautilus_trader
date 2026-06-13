//! Parser for default parameter values with type reference extraction

use crate::docgen::ir::{DocDefaultExpression, DocDefaultValue, DocTypeRef};
use crate::docgen::link::LinkResolver;
use crate::docgen::util::prefix_stripper;
use crate::TypeInfo;

/// Parser for default values that identifies and links type references
pub struct DefaultValueParser<'a> {
    link_resolver: &'a LinkResolver<'a>,
    current_module: &'a str,
}

impl<'a> DefaultValueParser<'a> {
    pub fn new(link_resolver: &'a LinkResolver<'a>, current_module: &'a str) -> Self {
        Self {
            link_resolver,
            current_module,
        }
    }

    /// Parse a default value string and identify type references
    pub fn parse(&self, default_str: &str, type_info: &TypeInfo) -> DocDefaultValue {
        // Strip module prefixes from display text
        let display = self.strip_module_prefixes(default_str);

        // Try to identify attribute references like "C.C1"
        if let Some(type_ref) = self.try_parse_attribute_ref(&display, type_info) {
            return DocDefaultValue::Expression(DocDefaultExpression {
                display: display.clone(),
                type_refs: vec![type_ref],
            });
        }

        // For now, treat everything else as simple values
        DocDefaultValue::Simple {
            value: display.clone(),
        }
    }

    /// Strip module prefixes like "_core.", "typing.", "builtins." from the display text
    fn strip_module_prefixes(&self, text: &str) -> String {
        prefix_stripper::strip_default_value_prefixes(text)
    }

    /// Try to parse an attribute reference like "C.C1"
    fn try_parse_attribute_ref(&self, text: &str, _type_info: &TypeInfo) -> Option<DocTypeRef> {
        // Look for pattern "ClassName.AttributeName"
        let parts: Vec<&str> = text.split('.').collect();
        if parts.len() != 2 {
            return None;
        }

        let class_name = parts[0];
        let attribute_name = parts[1];

        // Search for the class in the export map
        // The class might be exported from any module, so we need to search for FQNs ending with ".ClassName"
        let class_suffix = format!(".{}", class_name);
        let class_fqn = self
            .link_resolver
            .export_map()
            .keys()
            .find(|fqn| fqn.ends_with(&class_suffix) || *fqn == class_name)?;

        // Try to resolve the attribute link
        let link_target = self.link_resolver.resolve_attribute_link(
            class_fqn,
            attribute_name,
            self.current_module,
        )?;

        Some(DocTypeRef {
            text: text.to_string(),
            offset: 0,
            link_target: Some(link_target),
        })
    }
}
