use crate::stub_type::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Extract type identifier from a pre-qualified type name
///
/// If the type name is qualified (e.g., "sub_mod.ClassA"), extract the bare identifier.
/// Returns None if the type is unqualified or is a known builtin/typing type.
fn extract_type_identifier(type_name: &str) -> Option<&str> {
    // Check if it contains a dot (qualified name)
    if let Some(pos) = type_name.rfind('.') {
        let bare_name = &type_name[pos + 1..];
        // Skip known typing/builtin modules
        if type_name.starts_with("typing.") || type_name.starts_with("collections.") {
            return None;
        }
        Some(bare_name)
    } else {
        None
    }
}

/// Build type_refs HashMap from inner TypeInfo for compound types
///
/// If the inner type is locally-defined and qualified, track it for context-aware rendering.
fn build_type_refs_from_inner(inner: &TypeInfo) -> HashMap<String, TypeIdentifierRef> {
    let mut type_refs = HashMap::new();

    // If inner type is locally defined with a module, track it
    if let Some(ref source_module) = inner.source_module {
        if let Some(_module_name) = source_module.get() {
            // Extract bare type identifier from the (potentially qualified) name
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

    // Also inherit any existing type_refs from inner type (for nested compounds)
    type_refs.extend(inner.type_refs.clone());

    type_refs
}

impl<T: PyStubType> PyStubType for Option<T> {
    fn type_input() -> TypeInfo {
        let inner = T::type_input();
        let name = inner.name.clone();
        let mut import = inner.import.clone();
        import.insert("typing".into());

        let type_refs = build_type_refs_from_inner(&inner);

        TypeInfo {
            name: format!("typing.Optional[{name}]"),
            source_module: None,
            import,
            type_refs,
        }
    }
    fn type_output() -> TypeInfo {
        let inner = T::type_output();
        let name = inner.name.clone();
        let mut import = inner.import.clone();
        import.insert("typing".into());

        let type_refs = build_type_refs_from_inner(&inner);

        TypeInfo {
            name: format!("typing.Optional[{name}]"),
            source_module: None,
            import,
            type_refs,
        }
    }
}

impl<T: PyStubType> PyStubType for Box<T> {
    fn type_input() -> TypeInfo {
        T::type_input()
    }
    fn type_output() -> TypeInfo {
        T::type_output()
    }
}

impl<T: PyStubType, E> PyStubType for Result<T, E> {
    fn type_input() -> TypeInfo {
        T::type_input()
    }
    fn type_output() -> TypeInfo {
        T::type_output()
    }
}

impl<T: PyStubType> PyStubType for Vec<T> {
    fn type_input() -> TypeInfo {
        let inner = T::type_input();
        let name = inner.name.clone();
        let mut import = inner.import.clone();
        import.insert("typing".into());

        let type_refs = build_type_refs_from_inner(&inner);

        TypeInfo {
            name: format!("typing.Sequence[{name}]"),
            source_module: None,
            import,
            type_refs,
        }
    }
    fn type_output() -> TypeInfo {
        TypeInfo::list_of::<T>()
    }
}

impl<T: PyStubType, const N: usize> PyStubType for [T; N] {
    fn type_input() -> TypeInfo {
        let inner = T::type_input();
        let name = inner.name.clone();
        let mut import = inner.import.clone();
        import.insert("typing".into());

        let type_refs = build_type_refs_from_inner(&inner);

        TypeInfo {
            name: format!("typing.Sequence[{name}]"),
            source_module: None,
            import,
            type_refs,
        }
    }
    fn type_output() -> TypeInfo {
        TypeInfo::list_of::<T>()
    }
}

impl<T: PyStubType, State> PyStubType for HashSet<T, State> {
    fn type_output() -> TypeInfo {
        TypeInfo::set_of::<T>()
    }
}

impl<T: PyStubType> PyStubType for BTreeSet<T> {
    fn type_output() -> TypeInfo {
        TypeInfo::set_of::<T>()
    }
}

impl<T: PyStubType> PyStubType for indexmap::IndexSet<T> {
    fn type_output() -> TypeInfo {
        TypeInfo::set_of::<T>()
    }
}

macro_rules! impl_map_inner {
    () => {
        fn type_input() -> TypeInfo {
            let key_info = Key::type_input();
            let value_info = Value::type_input();

            let mut import = key_info.import.clone();
            import.extend(value_info.import.clone());
            import.insert("typing".into());

            let mut type_refs = build_type_refs_from_inner(&key_info);
            type_refs.extend(build_type_refs_from_inner(&value_info));

            TypeInfo {
                name: format!("typing.Mapping[{}, {}]", key_info.name, value_info.name),
                source_module: None,
                import,
                type_refs,
            }
        }
        fn type_output() -> TypeInfo {
            let key_info = Key::type_output();
            let value_info = Value::type_output();

            let mut import = key_info.import.clone();
            import.extend(value_info.import.clone());
            import.insert("builtins".into());

            let mut type_refs = build_type_refs_from_inner(&key_info);
            type_refs.extend(build_type_refs_from_inner(&value_info));

            TypeInfo {
                name: format!("builtins.dict[{}, {}]", key_info.name, value_info.name),
                source_module: None,
                import,
                type_refs,
            }
        }
    };
}

impl<Key: PyStubType, Value: PyStubType> PyStubType for BTreeMap<Key, Value> {
    impl_map_inner!();
}

impl<Key: PyStubType, Value: PyStubType, State> PyStubType for HashMap<Key, Value, State> {
    impl_map_inner!();
}

impl<Key: PyStubType, Value: PyStubType, State> PyStubType
    for indexmap::IndexMap<Key, Value, State>
{
    impl_map_inner!();
}

macro_rules! impl_tuple {
    ($($T:ident),*) => {
        impl<$($T: PyStubType),*> PyStubType for ($($T),* ,) {
            fn type_output() -> TypeInfo {
                let mut merged = HashSet::new();
                let mut names = Vec::new();
                let mut type_refs = HashMap::new();
                $(
                let info = $T::type_output();
                type_refs.extend(build_type_refs_from_inner(&info));
                names.push(info.name);
                merged.extend(info.import);
                )*
                TypeInfo {
                    name: format!("tuple[{}]", names.join(", ")),
                    source_module: None,
                    import: merged,
                    type_refs,
                }
            }
            fn type_input() -> TypeInfo {
                let mut merged = HashSet::new();
                let mut names = Vec::new();
                let mut type_refs = HashMap::new();
                $(
                let info = $T::type_input();
                type_refs.extend(build_type_refs_from_inner(&info));
                names.push(info.name);
                merged.extend(info.import);
                )*
                TypeInfo {
                    name: format!("tuple[{}]", names.join(", ")),
                    source_module: None,
                    import: merged,
                    type_refs,
                }
            }
        }
    };
}

impl_tuple!(T1);
impl_tuple!(T1, T2);
impl_tuple!(T1, T2, T3);
impl_tuple!(T1, T2, T3, T4);
impl_tuple!(T1, T2, T3, T4, T5);
impl_tuple!(T1, T2, T3, T4, T5, T6);
impl_tuple!(T1, T2, T3, T4, T5, T6, T7);
impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8);
impl_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
