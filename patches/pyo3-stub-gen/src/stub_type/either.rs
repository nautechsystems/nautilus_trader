use std::collections::{HashMap, HashSet};

use super::{ImportKind, PyStubType, TypeIdentifierRef, TypeInfo};

/// Extract type identifier from a pre-qualified type name
fn extract_type_identifier(type_name: &str) -> Option<&str> {
    if let Some(pos) = type_name.rfind('.') {
        let bare_name = &type_name[pos + 1..];
        if type_name.starts_with("typing.") || type_name.starts_with("collections.") {
            return None;
        }
        Some(bare_name)
    } else {
        None
    }
}

/// Build type_refs HashMap from inner TypeInfo for compound types
fn build_type_refs_from_inner(inner: &TypeInfo) -> HashMap<String, TypeIdentifierRef> {
    let mut type_refs = HashMap::new();
    if let Some(ref source_module) = inner.source_module {
        if let Some(_module_name) = source_module.get() {
            if let Some(bare_name) = extract_type_identifier(&inner.name) {
                type_refs.insert(
                    bare_name.to_string(),
                    TypeIdentifierRef {
                        module: source_module.clone(),
                        import_kind: ImportKind::Module,
                    },
                );
            }
        }
    }
    type_refs.extend(inner.type_refs.clone());
    type_refs
}

impl<L: PyStubType, R: PyStubType> PyStubType for either::Either<L, R> {
    fn type_input() -> TypeInfo {
        let info_l = L::type_input();
        let info_r = R::type_input();

        let mut import: HashSet<_> = info_l
            .import
            .iter()
            .cloned()
            .chain(info_r.import.iter().cloned())
            .collect();
        import.insert("typing".into());

        let mut type_refs = build_type_refs_from_inner(&info_l);
        type_refs.extend(build_type_refs_from_inner(&info_r));

        TypeInfo {
            name: format!("typing.Union[{}, {}]", info_l.name, info_r.name),
            source_module: None,
            import,
            type_refs,
        }
    }
    fn type_output() -> TypeInfo {
        let info_l = L::type_output();
        let info_r = R::type_output();

        let mut import: HashSet<_> = info_l
            .import
            .iter()
            .cloned()
            .chain(info_r.import.iter().cloned())
            .collect();
        import.insert("typing".into());

        let mut type_refs = build_type_refs_from_inner(&info_l);
        type_refs.extend(build_type_refs_from_inner(&info_r));

        TypeInfo {
            name: format!("typing.Union[{}, {}]", info_l.name, info_r.name),
            source_module: None,
            import,
            type_refs,
        }
    }
}
