//! Builder for converting StubInfo to DocPackage

use crate::docgen::{
    export::ExportResolver,
    ir::{
        DeprecatedInfo, DocAttribute, DocClass, DocFunction, DocItem, DocModule, DocPackage,
        DocParameter, DocSignature, DocSubmodule, DocTypeAlias, DocTypeExpr, DocVariable,
    },
    types::TypeRenderer,
    util::{is_hidden_module, prefix_stripper},
};
use crate::generate::StubInfo;
use crate::Result;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Helper to check if item already exists in the list
fn matches_item_name(item: &DocItem, name: &str) -> bool {
    match item {
        DocItem::Function(f) => f.name == name,
        DocItem::Class(c) => c.name == name,
        DocItem::TypeAlias(t) => t.name == name,
        DocItem::Variable(v) => v.name == name,
        DocItem::Module(m) => m.name == name,
    }
}

/// Context for building documentation items, containing shared rendering components
struct DocBuildContext<'a> {
    link_resolver: crate::docgen::link::LinkResolver<'a>,
    module: &'a str,
}

impl<'a> DocBuildContext<'a> {
    /// Get a type renderer for this context
    fn type_renderer(&self) -> TypeRenderer<'_> {
        TypeRenderer::new(&self.link_resolver, self.module)
    }

    /// Get a default value parser for this context
    fn default_parser(&self) -> crate::docgen::default_parser::DefaultValueParser<'_> {
        crate::docgen::default_parser::DefaultValueParser::new(&self.link_resolver, self.module)
    }
}

/// Builder for converting StubInfo to DocPackage
pub struct DocPackageBuilder<'a> {
    stub_info: &'a StubInfo,
    export_resolver: ExportResolver<'a>,
    export_map: BTreeMap<String, String>,
    default_module_name: String,
}

impl<'a> DocPackageBuilder<'a> {
    pub fn new(stub_info: &'a StubInfo) -> Self {
        let export_resolver = ExportResolver::new(&stub_info.modules);
        let export_map = export_resolver.build_export_map();

        // Get the default module name from the first module (they all share the same default_module_name)
        let default_module_name = stub_info
            .modules
            .values()
            .next()
            .map(|m| m.default_module_name.clone())
            .unwrap_or_default();

        Self {
            stub_info,
            export_resolver,
            export_map,
            default_module_name,
        }
    }

    /// Create a build context for a specific module
    fn create_context<'b>(&'b self, module: &'b str) -> DocBuildContext<'b> {
        let link_resolver = crate::docgen::link::LinkResolver::new(&self.export_map);
        DocBuildContext {
            link_resolver,
            module,
        }
    }

    pub fn build(self) -> Result<DocPackage> {
        let mut modules = BTreeMap::new();

        for (module_name, module) in &self.stub_info.modules {
            // Skip modules with any component starting with '_'
            if is_hidden_module(module_name) {
                continue;
            }

            let doc_module = self.build_module(module_name, module)?;
            modules.insert(module_name.clone(), doc_module);
        }

        let export_map = self.export_map;

        // Convert config to use relative POSIX path for JSON serialization
        let mut json_config = self.stub_info.config.doc_gen.clone().unwrap_or_default();
        if let Some(pyproject_dir) = &self.stub_info.pyproject_dir {
            let relative_posix = json_config.to_relative_posix_path(pyproject_dir);
            json_config.output_dir = PathBuf::from(relative_posix);
        }

        Ok(DocPackage {
            name: self.default_module_name.clone(),
            modules,
            export_map,
            config: json_config,
        })
    }

    fn build_module(&self, name: &str, module: &crate::generate::Module) -> Result<DocModule> {
        let exports = self.export_resolver.resolve_exports(module);
        let mut items = Vec::new();

        // Process functions - handle overloads (Requirement #1)
        for (func_name, func_defs) in &module.function {
            if exports.contains(*func_name) {
                items.push(self.build_function(name, func_defs)?);
            }
        }

        // Process type aliases (Requirement #2)
        for (alias_name, alias_def) in &module.type_aliases {
            if exports.contains(*alias_name) {
                items.push(self.build_type_alias(name, alias_def)?);
            }
        }

        // Process classes (sorted by name for deterministic output)
        let mut classes: Vec<_> = module.class.values().collect();
        classes.sort_by_key(|c| c.name);
        for class_def in classes {
            if exports.contains(class_def.name) {
                items.push(self.build_class(name, class_def)?);
            }
        }

        // Process enums (sorted by name for deterministic output)
        let mut enums: Vec<_> = module.enum_.values().collect();
        enums.sort_by_key(|e| e.name);
        for enum_def in enums {
            if exports.contains(enum_def.name) {
                items.push(self.build_enum_as_class(name, enum_def)?);
            }
        }

        // Process variables
        for (var_name, var_def) in &module.variables {
            if exports.contains(*var_name) {
                items.push(self.build_variable(name, var_def)?);
            }
        }

        // Process module re-exports (from reexport_module_members!)
        for re_export in &module.module_re_exports {
            if let Some(source_module) = self.stub_info.modules.get(&re_export.source_module) {
                for item_name in &re_export.items {
                    // Skip if already added (prefer directly-defined items)
                    if items.iter().any(|item| matches_item_name(item, item_name)) {
                        continue;
                    }

                    // Build re-exported item (link targets will be corrected later)
                    if let Some(item) = self.build_reexported_item(
                        &re_export.source_module,
                        source_module,
                        item_name,
                    )? {
                        items.push(item);
                    }
                }
            }
        }

        // Process submodules - convert to DocItem::Module entries
        for submod_name in &module.submodules {
            if !submod_name.starts_with('_') && exports.contains(submod_name) {
                // Construct FQN for the submodule
                let submod_fqn = if name.is_empty() {
                    submod_name.clone()
                } else {
                    format!("{}.{}", name, submod_name)
                };

                // Retrieve the submodule's doc from stub_info.modules
                let submod_doc = self
                    .stub_info
                    .modules
                    .get(&submod_fqn)
                    .map(|m| m.doc.clone())
                    .unwrap_or_default();

                items.push(DocItem::Module(DocSubmodule {
                    name: submod_name.clone(),
                    doc: submod_doc,
                    fqn: submod_fqn,
                }));
            }
        }

        // Correct all link targets and display text in all items
        // This ensures both directly-defined and re-exported items have correct references
        for item in &mut items {
            self.correct_link_targets(item, name);
        }

        Ok(DocModule {
            name: name.to_string(),
            doc: module.doc.clone(),
            items,
            submodules: module
                .submodules
                .iter()
                .filter(|s| !s.starts_with('_'))
                .cloned()
                .collect(),
        })
    }

    fn build_function(
        &self,
        module: &str,
        func_defs: &[crate::generate::FunctionDef],
    ) -> Result<DocItem> {
        // Sort overloads by source location for deterministic ordering (same as stub generation)
        let mut sorted_defs = func_defs.to_vec();
        sorted_defs.sort_by_key(|func| (func.file, func.line, func.column, func.index));

        // Requirement #1: Include ALL overload signatures
        let signatures: Vec<DocSignature> = sorted_defs
            .iter()
            .map(|def| self.build_signature(module, def))
            .collect::<Result<_>>()?;

        // Use first def's doc (they should all have same doc)
        let doc = sorted_defs
            .first()
            .map(|d| d.doc.to_string())
            .unwrap_or_default();

        let deprecated = sorted_defs.first().and_then(|d| {
            d.deprecated.as_ref().map(|dep| DeprecatedInfo {
                since: dep.since.map(|s| s.to_string()),
                note: dep.note.map(|s| s.to_string()),
            })
        });

        Ok(DocItem::Function(DocFunction {
            name: sorted_defs[0].name.to_string(),
            doc,
            signatures,
            is_async: sorted_defs[0].is_async,
            deprecated,
        }))
    }

    fn build_signature_from_params(
        &self,
        module: &str,
        parameters: &crate::generate::Parameters,
        return_type: &crate::TypeInfo,
    ) -> Result<DocSignature> {
        let ctx = self.create_context(module);
        let type_renderer = ctx.type_renderer();
        let default_parser = ctx.default_parser();

        let params: Vec<DocParameter> = parameters
            .positional_only
            .iter()
            .chain(parameters.positional_or_keyword.iter())
            .chain(parameters.keyword_only.iter())
            .chain(parameters.varargs.iter())
            .chain(parameters.varkw.iter())
            .map(|param| DocParameter {
                name: param.name.to_string(),
                type_: type_renderer.render_type(&param.type_info),
                default: match &param.default {
                    crate::generate::ParameterDefault::None => None,
                    crate::generate::ParameterDefault::Expr(s) => {
                        // Parse default value and identify type references
                        Some(default_parser.parse(s, &param.type_info))
                    }
                },
            })
            .collect();

        let ret_type = Some(type_renderer.render_type(return_type));

        Ok(DocSignature {
            parameters: params,
            return_type: ret_type,
        })
    }

    fn build_signature(
        &self,
        module: &str,
        def: &crate::generate::FunctionDef,
    ) -> Result<DocSignature> {
        self.build_signature_from_params(module, &def.parameters, &def.r#return)
    }

    fn build_signature_from_method(
        &self,
        module: &str,
        def: &crate::generate::MethodDef,
    ) -> Result<DocSignature> {
        self.build_signature_from_params(module, &def.parameters, &def.r#return)
    }

    fn build_type_alias(
        &self,
        module: &str,
        alias: &crate::generate::TypeAliasDef,
    ) -> Result<DocItem> {
        // Requirement #2: Preserve alias definition + docstring
        let ctx = self.create_context(module);
        let type_renderer = ctx.type_renderer();

        Ok(DocItem::TypeAlias(DocTypeAlias {
            name: alias.name.to_string(),
            doc: alias.doc.to_string(),
            definition: type_renderer.render_type(&alias.type_),
        }))
    }

    fn build_class(&self, module: &str, class: &crate::generate::ClassDef) -> Result<DocItem> {
        let ctx = self.create_context(module);
        let type_renderer = ctx.type_renderer();

        let bases: Vec<DocTypeExpr> = class
            .bases
            .iter()
            .map(|base| type_renderer.render_type(base))
            .collect();

        let mut methods = Vec::new();
        for (method_name, method_overloads) in &class.methods {
            let signatures: Vec<DocSignature> = method_overloads
                .iter()
                .map(|method| self.build_signature_from_method(module, method))
                .collect::<Result<_>>()?;

            let deprecated = method_overloads.first().and_then(|d| {
                d.deprecated.as_ref().map(|dep| DeprecatedInfo {
                    since: dep.since.map(|s| s.to_string()),
                    note: dep.note.map(|s| s.to_string()),
                })
            });

            methods.push(DocFunction {
                name: method_name.to_string(),
                doc: method_overloads
                    .first()
                    .map(|m| m.doc.to_string())
                    .unwrap_or_default(),
                signatures,
                is_async: method_overloads
                    .first()
                    .map(|m| m.is_async)
                    .unwrap_or(false),
                deprecated,
            });
        }

        // Convert plain #[pyo3(get, set)] field attributes
        let mut attributes: Vec<DocAttribute> = class
            .attrs
            .iter()
            .map(|attr| DocAttribute {
                name: attr.name.to_string(),
                doc: attr.doc.to_string(),
                type_: Some(type_renderer.render_type(&attr.r#type)),
                is_property: false,
                is_readonly: false,
                deprecated: attr.deprecated.as_ref().map(|dep| DeprecatedInfo {
                    since: dep.since.map(|s| s.to_string()),
                    note: dep.note.map(|s| s.to_string()),
                }),
            })
            .collect();

        // Convert #[getter]/#[setter] method properties
        for (prop_name, (getter, setter)) in &class.getter_setters {
            let member = getter.as_ref().or(setter.as_ref());
            if let Some(member) = member {
                // Use deprecation from getter preferentially, fall back to setter
                let deprecated = getter
                    .as_ref()
                    .and_then(|g| g.deprecated.as_ref())
                    .or_else(|| setter.as_ref().and_then(|s| s.deprecated.as_ref()))
                    .map(|dep| DeprecatedInfo {
                        since: dep.since.map(|s| s.to_string()),
                        note: dep.note.map(|s| s.to_string()),
                    });
                attributes.push(DocAttribute {
                    name: prop_name.clone(),
                    doc: member.doc.to_string(),
                    type_: Some(type_renderer.render_type(&member.r#type)),
                    is_property: true,
                    is_readonly: setter.is_none(),
                    deprecated,
                });
            }
        }

        Ok(DocItem::Class(DocClass {
            name: class.name.to_string(),
            doc: class.doc.to_string(),
            bases,
            methods,
            attributes,
            deprecated: None, // ClassDef doesn't have deprecated field
        }))
    }

    fn build_enum_as_class(
        &self,
        module: &str,
        enum_def: &crate::generate::EnumDef,
    ) -> Result<DocItem> {
        let ctx = self.create_context(module);
        let type_renderer = ctx.type_renderer();

        // Convert enum variants to DocAttributes
        let mut attributes: Vec<DocAttribute> = enum_def
            .variants
            .iter()
            .map(|(variant_name, variant_doc)| DocAttribute {
                name: (*variant_name).to_string(),
                doc: (*variant_doc).to_string(),
                type_: None, // Enum variants don't have explicit type annotations
                is_property: false,
                is_readonly: false,
                deprecated: None,
            })
            .collect();

        // Collect setter names for determining readonly status
        let setter_names: std::collections::HashSet<&str> =
            enum_def.setters.iter().map(|s| s.name).collect();

        // Convert enum getters to property attributes
        for getter in &enum_def.getters {
            attributes.push(DocAttribute {
                name: getter.name.to_string(),
                doc: getter.doc.to_string(),
                type_: Some(type_renderer.render_type(&getter.r#type)),
                is_property: true,
                is_readonly: !setter_names.contains(getter.name),
                deprecated: getter.deprecated.as_ref().map(|dep| DeprecatedInfo {
                    since: dep.since.map(|s| s.to_string()),
                    note: dep.note.map(|s| s.to_string()),
                }),
            });
        }

        Ok(DocItem::Class(DocClass {
            name: enum_def.name.to_string(),
            doc: enum_def.doc.to_string(),
            bases: Vec::new(), // Enums don't have bases in our structure
            methods: Vec::new(),
            attributes,
            deprecated: None,
        }))
    }

    fn build_variable(&self, module: &str, var: &crate::generate::VariableDef) -> Result<DocItem> {
        let ctx = self.create_context(module);
        let type_renderer = ctx.type_renderer();

        Ok(DocItem::Variable(DocVariable {
            name: var.name.to_string(),
            doc: String::new(), // VariableDef doesn't have doc field
            type_: Some(type_renderer.render_type(&var.type_)),
        }))
    }

    /// Build a re-exported item from a source module
    fn build_reexported_item(
        &self,
        source_module_name: &str,
        source_module: &crate::generate::Module,
        item_name: &str,
    ) -> Result<Option<DocItem>> {
        // Try functions
        if let Some(func_defs) = source_module.function.get(item_name) {
            return Ok(Some(self.build_function(source_module_name, func_defs)?));
        }

        // Try classes
        for class_def in source_module.class.values() {
            if class_def.name == item_name {
                return Ok(Some(self.build_class(source_module_name, class_def)?));
            }
        }

        // Try enums
        for enum_def in source_module.enum_.values() {
            if enum_def.name == item_name {
                return Ok(Some(
                    self.build_enum_as_class(source_module_name, enum_def)?,
                ));
            }
        }

        // Try type aliases
        if let Some(alias_def) = source_module.type_aliases.get(item_name) {
            return Ok(Some(self.build_type_alias(source_module_name, alias_def)?));
        }

        // Try variables
        if let Some(var_def) = source_module.variables.get(item_name) {
            return Ok(Some(self.build_variable(source_module_name, var_def)?));
        }

        Ok(None)
    }

    /// Correct link targets in a re-exported item to point to the target module
    fn correct_link_targets(&self, item: &mut DocItem, _target_module: &str) {
        match item {
            DocItem::Function(func) => {
                for sig in &mut func.signatures {
                    if let Some(ret) = &mut sig.return_type {
                        self.correct_type_expr(ret);
                    }
                    for param in &mut sig.parameters {
                        self.correct_type_expr(&mut param.type_);
                    }
                }
            }
            DocItem::Class(cls) => {
                for base in &mut cls.bases {
                    self.correct_type_expr(base);
                }
                for method in &mut cls.methods {
                    for sig in &mut method.signatures {
                        if let Some(ret) = &mut sig.return_type {
                            self.correct_type_expr(ret);
                        }
                        for param in &mut sig.parameters {
                            self.correct_type_expr(&mut param.type_);
                        }
                    }
                }
                for attr in &mut cls.attributes {
                    if let Some(type_) = &mut attr.type_ {
                        self.correct_type_expr(type_);
                    }
                }
            }
            DocItem::TypeAlias(alias) => {
                self.correct_type_expr(&mut alias.definition);
            }
            DocItem::Variable(var) => {
                if let Some(type_) = &mut var.type_ {
                    self.correct_type_expr(type_);
                }
            }
            DocItem::Module(_) => {}
        }
    }

    /// Correct a type expression to use export_map for link targets
    fn correct_type_expr(&self, type_expr: &mut DocTypeExpr) {
        if let Some(link_target) = &mut type_expr.link_target {
            // Try to find the correct export module and FQN
            // First try the original FQN
            let (exported_fqn, exported_module) =
                if let Some(module) = self.export_map.get(&link_target.fqn) {
                    (link_target.fqn.clone(), module.clone())
                } else {
                    // If not found, try to extract the type name and look for it under other modules
                    // e.g., "hidden_module_docgen_test._core.A" -> try "hidden_module_docgen_test.A"
                    if let Some(type_name) = link_target.fqn.split('.').next_back() {
                        // Try each module in export_map to find a match
                        if let Some((fqn, module)) = self
                            .export_map
                            .iter()
                            .find(|(fqn, _)| fqn.ends_with(&format!(".{}", type_name)))
                        {
                            (fqn.clone(), module.clone())
                        } else {
                            (link_target.fqn.clone(), link_target.doc_module.clone())
                        }
                    } else {
                        (link_target.fqn.clone(), link_target.doc_module.clone())
                    }
                };

            link_target.fqn = exported_fqn;
            link_target.doc_module = exported_module;

            // Update display text to strip internal module prefixes
            // e.g., "_core.A" -> "A"
            type_expr.display = self.strip_internal_module_prefix(&type_expr.display);
        }
        for child in &mut type_expr.children {
            self.correct_type_expr(child);
        }
    }

    /// Strip internal module prefixes (modules starting with '_') from display text
    /// e.g., "_core.A" -> "A", "_internal.Foo" -> "Foo"
    fn strip_internal_module_prefix(&self, display: &str) -> String {
        prefix_stripper::strip_internal_prefixes(display)
    }
}
