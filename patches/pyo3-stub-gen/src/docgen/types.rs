//! Type expression rendering for documentation

use crate::docgen::ir::{DocTypeExpr, ItemKind, LinkTarget};
use crate::docgen::link::LinkResolver;
use crate::docgen::util::prefix_stripper;
use crate::generate::qualifier::{tokenize, Token};
use crate::TypeInfo;

/// Type alias definition placeholder
/// TODO: This will be used for type alias expansion in future versions
pub struct TypeAliasDef;

/// Parsed structure of a type expression
#[derive(Debug, Clone, PartialEq)]
enum TypeStructure {
    /// Simple type: "int", "ClassA"
    Simple(String),
    /// Generic type: "Optional[A]", "Dict[str, int]"
    Generic {
        base: String,
        args: Vec<TypeStructure>,
    },
    /// Union type: "A | B | C"
    Union(Vec<TypeStructure>),
}

impl TypeStructure {
    /// Parse a type expression string into a TypeStructure
    fn parse(expr: &str) -> Self {
        let tokens = tokenize(expr);
        Self::parse_tokens(&tokens)
    }

    /// Parse a slice of tokens into a TypeStructure
    fn parse_tokens(tokens: &[Token]) -> Self {
        // Filter out whitespace for easier parsing
        let filtered_tokens: Vec<Token> = tokens
            .iter()
            .filter(|t| !matches!(t, Token::Whitespace(_)))
            .cloned()
            .collect();

        if filtered_tokens.is_empty() {
            return TypeStructure::Simple(String::new());
        }

        // Try to parse as union first (lowest precedence)
        if let Some(union_parts) = Self::try_parse_union(&filtered_tokens) {
            let variants: Vec<TypeStructure> = union_parts
                .into_iter()
                .map(|part| Self::parse_tokens(&part))
                .collect();
            return TypeStructure::Union(variants);
        }

        // Try to parse as generic
        if let Some((base_tokens, arg_token_lists)) = Self::try_parse_generic(&filtered_tokens) {
            let base = Self::tokens_to_string(&base_tokens);
            let args: Vec<TypeStructure> = arg_token_lists
                .into_iter()
                .map(|arg_tokens| Self::parse_tokens(&arg_tokens))
                .collect();
            return TypeStructure::Generic { base, args };
        }

        // Simple type
        TypeStructure::Simple(Self::tokens_to_string(&filtered_tokens))
    }

    /// Try to parse tokens as a union (A | B | C)
    /// Returns Some(vec![tokens_for_A, tokens_for_B, tokens_for_C]) if successful
    fn try_parse_union(tokens: &[Token]) -> Option<Vec<Vec<Token>>> {
        let mut parts = Vec::new();
        let mut current_part = Vec::new();
        let mut depth = 0;

        for token in tokens {
            match token {
                Token::OpenBracket(_) => {
                    depth += 1;
                    current_part.push(token.clone());
                }
                Token::CloseBracket(_) => {
                    depth -= 1;
                    current_part.push(token.clone());
                }
                Token::Pipe if depth == 0 => {
                    if !current_part.is_empty() {
                        parts.push(current_part);
                        current_part = Vec::new();
                    }
                }
                _ => {
                    current_part.push(token.clone());
                }
            }
        }

        if !current_part.is_empty() {
            parts.push(current_part);
        }

        // Only return Some if we actually found pipes (i.e., more than one part)
        if parts.len() > 1 {
            Some(parts)
        } else {
            None
        }
    }

    /// Try to parse tokens as a generic type
    /// Returns Some((base_tokens, vec![arg1_tokens, arg2_tokens, ...])) if successful
    fn try_parse_generic(tokens: &[Token]) -> Option<(Vec<Token>, Vec<Vec<Token>>)> {
        // Find the first opening bracket
        let bracket_pos = tokens
            .iter()
            .position(|t| matches!(t, Token::OpenBracket('[')))?;

        // Everything before the bracket is the base
        let base_tokens = tokens[..bracket_pos].to_vec();

        // Find matching closing bracket
        let mut depth = 0;
        let mut close_pos = None;
        for (i, token) in tokens[bracket_pos..].iter().enumerate() {
            match token {
                Token::OpenBracket(_) => depth += 1,
                Token::CloseBracket(']') => {
                    depth -= 1;
                    if depth == 0 {
                        close_pos = Some(bracket_pos + i);
                        break;
                    }
                }
                _ => {}
            }
        }

        let close_pos = close_pos?;

        // Extract the contents inside the brackets
        let inner_tokens = &tokens[bracket_pos + 1..close_pos];

        // Split by commas at depth 0
        let arg_token_lists = Self::split_by_comma_at_depth_zero(inner_tokens);

        Some((base_tokens, arg_token_lists))
    }

    /// Split tokens by commas at depth 0
    fn split_by_comma_at_depth_zero(tokens: &[Token]) -> Vec<Vec<Token>> {
        let mut parts = Vec::new();
        let mut current_part = Vec::new();
        let mut depth = 0;

        for token in tokens {
            match token {
                Token::OpenBracket(_) => {
                    depth += 1;
                    current_part.push(token.clone());
                }
                Token::CloseBracket(_) => {
                    depth -= 1;
                    current_part.push(token.clone());
                }
                Token::Comma if depth == 0 => {
                    if !current_part.is_empty() {
                        parts.push(current_part);
                        current_part = Vec::new();
                    }
                }
                _ => {
                    current_part.push(token.clone());
                }
            }
        }

        if !current_part.is_empty() {
            parts.push(current_part);
        }

        if parts.is_empty() {
            vec![tokens.to_vec()]
        } else {
            parts
        }
    }

    /// Convert tokens to a string representation
    fn tokens_to_string(tokens: &[Token]) -> String {
        let mut result = String::new();
        for token in tokens {
            match token {
                Token::Identifier(s) => result.push_str(s),
                Token::DottedPath(parts) => result.push_str(&parts.join(".")),
                Token::OpenBracket(ch) => result.push(*ch),
                Token::CloseBracket(ch) => result.push(*ch),
                Token::Comma => result.push(','),
                Token::Pipe => result.push_str(" | "),
                Token::Ellipsis => result.push_str("..."),
                Token::StringLiteral(s) => {
                    result.push('"');
                    result.push_str(s);
                    result.push('"');
                }
                Token::Whitespace(ws) => result.push_str(ws),
            }
        }
        result
    }
}

/// Renderer for type expressions
pub struct TypeRenderer<'a> {
    link_resolver: &'a LinkResolver<'a>,
    current_module: &'a str,
}

impl<'a> TypeRenderer<'a> {
    pub fn new(link_resolver: &'a LinkResolver<'a>, current_module: &'a str) -> Self {
        Self {
            link_resolver,
            current_module,
        }
    }

    /// Render a type expression
    /// 1. Check if this is a type alias - preserve name, don't expand
    /// 2. Strip module prefixes from display text
    /// 3. Resolve link target using LinkResolver
    /// 4. Recursively handle generic parameters
    pub fn render_type(&self, type_info: &TypeInfo) -> DocTypeExpr {
        // Parse the type expression into a structure
        let type_structure = TypeStructure::parse(&type_info.name);

        // Recursively render the structure
        self.render_type_structure(&type_structure, type_info)
    }

    /// Recursively render a TypeStructure into a DocTypeExpr
    fn render_type_structure(
        &self,
        structure: &TypeStructure,
        type_info: &TypeInfo,
    ) -> DocTypeExpr {
        match structure {
            TypeStructure::Simple(name) => {
                // Simple type - try to create a link
                let display = self.strip_module_prefix(name);
                let link_target = self.try_create_link_for_name(name, type_info);

                DocTypeExpr {
                    display,
                    link_target,
                    children: Vec::new(),
                }
            }

            TypeStructure::Generic { base, args } => {
                // Generic type - render base and recursively render args
                let base_display = self.strip_module_prefix(base);
                let base_link = self.try_create_link_for_name(base, type_info);

                // Recursively render all arguments
                let children: Vec<DocTypeExpr> = args
                    .iter()
                    .map(|arg| self.render_type_structure(arg, type_info))
                    .collect();

                // Build display string with brackets
                let args_display: Vec<String> =
                    children.iter().map(|child| child.display.clone()).collect();
                let display = format!("{}[{}]", base_display, args_display.join(", "));

                DocTypeExpr {
                    display,
                    link_target: base_link,
                    children,
                }
            }

            TypeStructure::Union(variants) => {
                // Union type - recursively render all variants
                let children: Vec<DocTypeExpr> = variants
                    .iter()
                    .map(|variant| self.render_type_structure(variant, type_info))
                    .collect();

                // Build display string with pipe operators
                let variants_display: Vec<String> =
                    children.iter().map(|child| child.display.clone()).collect();
                let display = variants_display.join(" | ");

                DocTypeExpr {
                    display,
                    link_target: None, // Union itself has no link
                    children,
                }
            }
        }
    }

    /// Try to create a link target for a simple type name
    /// Uses type_refs to look up module information
    fn try_create_link_for_name(&self, name: &str, type_info: &TypeInfo) -> Option<LinkTarget> {
        // Extract bare identifier from potentially qualified name
        // e.g., "main_mod.A" -> "A"
        let bare_name = name.split('.').next_back().unwrap_or(name);

        // Look up in type_refs
        let type_ref = type_info.type_refs.get(bare_name)?;

        // Get module path from TypeIdentifierRef
        let module_path = type_ref.module.get()?;

        // Create FQN (fully qualified name)
        let fqn = format!("{}.{}", module_path, bare_name);

        // Use export_map to find the correct doc module
        let doc_module = self
            .link_resolver
            .export_map()
            .get(&fqn)
            .cloned()
            .unwrap_or_else(|| self.current_module.to_string());

        Some(LinkTarget {
            fqn,
            doc_module,
            kind: ItemKind::Class, // Assume class for now
            attribute: None,
        })
    }

    /// Strip module prefixes from type names
    /// Remove "typing.", "builtins.", "package.submod."
    /// Keep only bare names: "Optional[ClassA]" not "typing.Optional[sub_mod.ClassA]"
    fn strip_module_prefix(&self, type_name: &str) -> String {
        prefix_stripper::strip_type_prefixes(type_name, self.current_module)
    }
}
