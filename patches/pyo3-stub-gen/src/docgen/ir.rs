//! Intermediate representation for documentation generation

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::docgen::config::DocGenConfig;

/// Root documentation package structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocPackage {
    pub name: String,
    pub modules: BTreeMap<String, DocModule>,
    /// Maps item FQN to the public module where it's exported
    pub export_map: BTreeMap<String, String>,
    /// Documentation generation configuration
    pub config: DocGenConfig,
}

/// A single module's documentation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocModule {
    pub name: String,
    pub doc: String,
    pub items: Vec<DocItem>,
    pub submodules: Vec<String>,
}

/// A reference to a submodule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocSubmodule {
    pub name: String,
    pub doc: String,
    pub fqn: String,
}

/// A documented item (function, class, type alias, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DocItem {
    Function(DocFunction),
    Class(DocClass),
    TypeAlias(DocTypeAlias),
    Variable(DocVariable),
    Module(DocSubmodule),
}

/// A function with all its overload signatures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocFunction {
    pub name: String,
    /// MyST-formatted docstring
    pub doc: String,
    /// ALL overload cases
    pub signatures: Vec<DocSignature>,
    pub is_async: bool,
    pub deprecated: Option<DeprecatedInfo>,
}

/// A single function signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocSignature {
    pub parameters: Vec<DocParameter>,
    pub return_type: Option<DocTypeExpr>,
}

/// A function parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocParameter {
    pub name: String,
    pub type_: DocTypeExpr,
    pub default: Option<DocDefaultValue>,
}

/// A type alias definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocTypeAlias {
    pub name: String,
    pub doc: String,
    pub definition: DocTypeExpr,
}

/// A module-level variable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocVariable {
    pub name: String,
    pub doc: String,
    pub type_: Option<DocTypeExpr>,
}

/// A class definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocClass {
    pub name: String,
    pub doc: String,
    pub bases: Vec<DocTypeExpr>,
    pub methods: Vec<DocFunction>,
    pub attributes: Vec<DocAttribute>,
    pub deprecated: Option<DeprecatedInfo>,
}

/// A class attribute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocAttribute {
    pub name: String,
    pub doc: String,
    pub type_: Option<DocTypeExpr>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_property: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_readonly: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<DeprecatedInfo>,
}

/// Type expression with separate display and link target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocTypeExpr {
    /// Display text (stripped of module prefixes): "ClassA"
    pub display: String,
    /// Where to link (FQN after Haddock resolution)
    pub link_target: Option<LinkTarget>,
    /// Generic parameters (recursively)
    pub children: Vec<DocTypeExpr>,
}

/// Default value that may contain type references
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DocDefaultValue {
    /// Simple literal (no type references): "42", "None", etc.
    Simple { value: String },
    /// Expression containing type references: "C.C1", "SomeClass()"
    Expression(DocDefaultExpression),
}

/// Default value expression with embedded type references
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocDefaultExpression {
    /// Full expression text: "C.C1"
    pub display: String,
    /// Identified type references within expression
    pub type_refs: Vec<DocTypeRef>,
}

/// Type reference within a default value expression
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocTypeRef {
    /// Full reference text: "C.C1"
    pub text: String,
    /// Character offset in display string
    pub offset: usize,
    /// Can link to class or class attribute
    pub link_target: Option<LinkTarget>,
}

/// Link target information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkTarget {
    /// Fully qualified name (can be "module.Class" or "module.Class.attribute")
    pub fqn: String,
    /// Module where the item is documented (after Haddock resolution)
    pub doc_module: String,
    /// Item kind
    pub kind: ItemKind,
    /// Attribute name for attribute-level links (e.g., "C1" for enum variant)
    pub attribute: Option<String>,
}

/// Kind of documented item
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    Class,
    Function,
    TypeAlias,
    Variable,
    Module,
}

/// Deprecation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeprecatedInfo {
    pub since: Option<String>,
    pub note: Option<String>,
}

impl DocPackage {
    /// Normalize all Vec fields for deterministic JSON output
    pub fn normalize(&mut self) {
        for module in self.modules.values_mut() {
            module.normalize();
        }
    }
}

impl DocModule {
    fn normalize(&mut self) {
        // Sort items by type priority and then by name for deterministic output
        self.items.sort_by(|a, b| {
            use DocItem::*;
            match (a, b) {
                (Function(f1), Function(f2)) => f1.name.cmp(&f2.name),
                (Class(c1), Class(c2)) => c1.name.cmp(&c2.name),
                (TypeAlias(t1), TypeAlias(t2)) => t1.name.cmp(&t2.name),
                (Variable(v1), Variable(v2)) => v1.name.cmp(&v2.name),
                (Module(m1), Module(m2)) => m1.name.cmp(&m2.name),
                // Sort by type priority if types differ, then by name
                _ => Self::item_type_priority(a)
                    .cmp(&Self::item_type_priority(b))
                    .then_with(|| Self::item_name(a).cmp(Self::item_name(b))),
            }
        });

        // Sort submodules alphabetically
        self.submodules.sort();

        // Normalize nested items
        for item in &mut self.items {
            match item {
                DocItem::Class(c) => c.normalize(),
                DocItem::Function(f) => f.normalize(),
                _ => {}
            }
        }
    }

    fn item_type_priority(item: &DocItem) -> u8 {
        match item {
            DocItem::Module(_) => 0,
            DocItem::Class(_) => 1,
            DocItem::Function(_) => 2,
            DocItem::TypeAlias(_) => 3,
            DocItem::Variable(_) => 4,
        }
    }

    fn item_name(item: &DocItem) -> &str {
        match item {
            DocItem::Function(f) => &f.name,
            DocItem::Class(c) => &c.name,
            DocItem::TypeAlias(t) => &t.name,
            DocItem::Variable(v) => &v.name,
            DocItem::Module(m) => &m.name,
        }
    }
}

impl DocClass {
    fn normalize(&mut self) {
        // Sort methods by name
        self.methods.sort_by(|a, b| a.name.cmp(&b.name));

        // Sort attributes by name
        self.attributes.sort_by(|a, b| a.name.cmp(&b.name));

        // Sort bases by display name
        self.bases.sort_by(|a, b| a.display.cmp(&b.display));

        // Normalize methods
        for method in &mut self.methods {
            method.normalize();
        }
    }
}

impl DocFunction {
    fn normalize(&mut self) {
        // Sort signatures by parameter count, then by parameter names for consistent overload ordering
        self.signatures.sort_by(|a, b| {
            a.parameters.len().cmp(&b.parameters.len()).then_with(|| {
                // Compare parameter names lexicographically
                for (p1, p2) in a.parameters.iter().zip(b.parameters.iter()) {
                    match p1.name.cmp(&p2.name) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                }
                std::cmp::Ordering::Equal
            })
        });
    }
}
