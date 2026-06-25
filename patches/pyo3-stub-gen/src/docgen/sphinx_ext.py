# type: ignore
"""Sphinx extension for pyo3-stub-gen API documentation"""

import json
import re
from functools import lru_cache
from pathlib import Path
from docutils import nodes
from sphinx.addnodes import (
    desc, desc_signature, desc_name, desc_parameterlist,
    desc_parameter, desc_returns, pending_xref, desc_content, desc_annotation,
    index
)
from myst_parser.parsers.docutils_ import Parser as MystParser
from sphinx.util.docutils import SphinxDirective

# Helper functions for building documentation nodes

def _parse_and_link_type(type_str):
    """Parse type string and create intersphinx links for external types"""
    # Known external modules that should be linked via intersphinx
    EXTERNAL_MODULES = {
        'builtins', 'typing', 'collections', 'collections.abc',
        'typing_extensions', 'decimal', 'datetime', 'pathlib',
        'numpy', 'numpy.typing'
    }

    # Special constants
    SPECIAL_CONSTANTS = {'None': 'constants', 'True': 'constants', 'False': 'constants'}

    # Recursively parse and link types
    def parse_recursive(s):
        result = []
        i = 0

        while i < len(s):
            # Skip whitespace
            if s[i].isspace():
                result.append(nodes.Text(s[i]))
                i += 1
                continue

            # Handle brackets and special chars
            if s[i] in '[](),|':
                result.append(nodes.Text(s[i]))
                i += 1
                continue

            # Try to match a qualified name or identifier
            match = re.match(r'([a-zA-Z_][a-zA-Z0-9_.]*)', s[i:])
            if match:
                name = match.group(1)
                i += len(name)

                # Check for special constants (not in intersphinx, render as text)
                if name in SPECIAL_CONSTANTS:
                    result.append(nodes.Text(name))
                    continue

                # Check if it's a qualified external type
                if '.' in name:
                    parts = name.split('.')
                    # Try to find which part is the module
                    for k in range(len(parts), 0, -1):
                        module = '.'.join(parts[:k])
                        if module in EXTERNAL_MODULES:
                            # Skip builtins - they're not in intersphinx inventory
                            if module == 'builtins':
                                result.append(nodes.Text(name))
                                break

                            # Determine reftype based on module and type name
                            type_name = parts[-1]
                            if module == 'typing':
                                # Check if it's py:data or py:class in typing module
                                if type_name in ('Any', 'Optional', 'Literal', 'LiteralString',
                                                 'AnyStr', 'NoReturn', 'Never', 'Self',
                                                 'TypeAlias', 'ClassVar', 'Final'):
                                    reftype = 'data'
                                else:
                                    reftype = 'class'
                            else:
                                reftype = 'class'

                            xref = pending_xref(
                                '',
                                refdomain='py',
                                reftype=reftype,
                                reftarget=name,
                                refexplicit=False,
                            )
                            xref += nodes.literal(text=name)
                            result.append(xref)
                            break
                    else:
                        # Not an external type
                        result.append(nodes.Text(name))
                # Check for bare builtins (not in intersphinx, render as text)
                elif name in ('int', 'str', 'float', 'bool', 'bytes', 'list', 'dict',
                              'tuple', 'set', 'frozenset', 'type', 'object', 'complex'):
                    # Bare builtins are not in Python's intersphinx inventory
                    result.append(nodes.Text(name))
                # Check for bare typing types (mixed py:class and py:data)
                elif name in ('Any', 'Optional', 'Literal', 'LiteralString', 'AnyStr',
                              'NoReturn', 'Never', 'Self', 'TypeAlias', 'ClassVar', 'Final'):
                    # These are py:data in Python's intersphinx
                    xref = pending_xref(
                        '',
                        refdomain='py',
                        reftype='data',
                        reftarget=f'typing.{name}',
                        refexplicit=False,
                    )
                    xref += nodes.Text(name)
                    result.append(xref)
                elif name in ('Union', 'TypeVar', 'Generic', 'Protocol'):
                    # These are py:class in Python's intersphinx
                    xref = pending_xref(
                        '',
                        refdomain='py',
                        reftype='class',
                        reftarget=f'typing.{name}',
                        refexplicit=False,
                    )
                    xref += nodes.Text(name)
                    result.append(xref)
                # Check for bare collections.abc types
                elif name in ('Sequence', 'Mapping', 'Callable', 'Iterable', 'Iterator',
                              'Collection', 'Container', 'MutableSequence', 'MutableMapping'):
                    xref = pending_xref(
                        '',
                        refdomain='py',
                        reftype='class',
                        reftarget=f'collections.abc.{name}',
                        refexplicit=False,
                    )
                    xref += nodes.Text(name)
                    result.append(xref)
                else:
                    # Unknown type, don't link
                    result.append(nodes.Text(name))
            else:
                # Unrecognized character
                result.append(nodes.Text(s[i]))
                i += 1

        return result

    # Parse the type string
    nodes_list = parse_recursive(type_str)

    # Wrap in a container
    container = nodes.inline()
    for node in nodes_list:
        container += node
    return container

def _build_type_expr(type_expr):
    """Build type expression with intersphinx linking for external types

    Recursively handles nested types from the children field.
    """
    display = type_expr['display']
    link_target = type_expr.get('link_target')
    children = type_expr.get('children', [])

    # Case 1: Type with link target and children (e.g., Generic[T] where Generic has a link)
    if link_target and children:
        # Build the base type with link
        base_name = display.split('[')[0] if '[' in display else display
        xref = pending_xref(
            '',
            refdomain='py',
            reftype=_kind_to_reftype(link_target['kind']),
            reftarget=link_target['fqn'],
            refexplicit=True,
        )
        xref += nodes.Text(base_name)

        # Build the generic part with children
        return _build_generic_with_children(xref, children)

    # Case 2: Type with link target but no children (simple type)
    elif link_target:
        # Create pending_xref for our own types
        xref = pending_xref(
            '',
            refdomain='py',
            reftype=_kind_to_reftype(link_target['kind']),
            reftarget=link_target['fqn'],
            refexplicit=True,
        )
        xref += nodes.Text(display)
        return xref

    # Case 3: Union type (has children but no link_target)
    elif children:
        # Check if this is a union type by looking for '|' in display
        if '|' in display:
            return _build_union_type(children)
        # Otherwise it's a generic type with no base link (e.g., typing.Optional)
        else:
            # Extract base name
            base_name = display.split('[')[0] if '[' in display else display
            # Parse base to potentially link it via intersphinx
            base_node = _parse_and_link_type(base_name)
            return _build_generic_with_children(base_node, children)

    # Case 4: External type or simple builtin (no link, no children)
    else:
        # Parse the type expression and create intersphinx links for external types
        return _parse_and_link_type(display)


def _build_generic_with_children(base_node, children):
    """Build a generic type expression like Base[T1, T2] with linked children"""
    container = nodes.inline()
    container += base_node
    container += nodes.Text('[')

    for i, child in enumerate(children):
        if i > 0:
            container += nodes.Text(', ')
        # Recursively render child
        child_node = _build_type_expr(child)
        container += child_node

    container += nodes.Text(']')
    return container


def _build_union_type(children):
    """Build a union type expression like A | B | C with linked children"""
    container = nodes.inline()

    for i, child in enumerate(children):
        if i > 0:
            container += nodes.Text(' | ')
        # Recursively render child
        child_node = _build_type_expr(child)
        container += child_node

    return container

def _build_link_from_target(text, link_target):
    """Build cross-reference node from link target"""
    # Handle attribute-level links (e.g., enum variants)
    if link_target.get('attribute'):
        # Link to class attribute (e.g., C.C1 variant)
        # Use py:attribute reftype with full FQN
        xref = pending_xref(
            '',
            refdomain='py',
            reftype='attribute',
            reftarget=link_target['fqn'],
            refexplicit=True,
        )
        xref += nodes.Text(text)
        return xref

    # Regular class/function/etc link
    xref = pending_xref(
        '',
        refdomain='py',
        reftype=_kind_to_reftype(link_target['kind']),
        reftarget=link_target['fqn'],
        refexplicit=True,
    )
    xref += nodes.Text(text)
    return xref

def _build_default_value(default_value):
    """Build nodes for default value with type links"""
    if default_value.get('kind') == 'Simple':
        return nodes.Text(default_value['value'])

    elif default_value.get('kind') == 'Expression':
        # Build expression with embedded type links
        expr = default_value['display']
        type_refs = default_value.get('type_refs', [])

        # Sort by offset descending to insert links from right to left
        nodes_list = []
        last_pos = len(expr)

        for ref in sorted(type_refs, key=lambda r: r['offset'], reverse=True):
            # Add text after this reference
            if ref['offset'] + len(ref['text']) < last_pos:
                nodes_list.insert(0, nodes.Text(expr[ref['offset'] + len(ref['text']):last_pos]))

            # Add linked reference (entire C.C1, not just C)
            if ref.get('link_target'):
                xref = _build_link_from_target(ref['text'], ref['link_target'])
                nodes_list.insert(0, xref)
            else:
                nodes_list.insert(0, nodes.Text(ref['text']))

            last_pos = ref['offset']

        # Add remaining text before first reference
        if last_pos > 0:
            nodes_list.insert(0, nodes.Text(expr[:last_pos]))

        # Return a container with all nodes
        container = nodes.inline(classes=['default_value'])
        for node in nodes_list:
            container += node
        return container

    # Fallback for unknown kinds
    return nodes.Text(str(default_value))

def _parse_myst(markdown_text, env=None):
    """Parse MyST markdown to docutils nodes using myst-parser

    Returns a list of docutils nodes (not a container) so they can be added directly
    to parent nodes.

    Args:
        markdown_text: The MyST markdown text to parse
        env: Optional Sphinx environment (required for MyST features to work correctly)
    """
    from docutils.core import publish_doctree
    import textwrap

    try:
        # Dedent the text to avoid markdown treating it as a code block
        # (indented text in markdown is interpreted as preformatted code)
        dedented_text = textwrap.dedent(markdown_text).strip()

        # Base settings
        settings_overrides = {
            'report_level': 5,  # Suppress warnings
            'halt_level': 5,
        }

        # Add env to settings if provided
        if env is not None:
            settings_overrides['env'] = env

            # Extract MyST configuration from Sphinx app config
            # This enables all MyST extensions configured in conf.py
            if hasattr(env, 'app') and hasattr(env.app, 'config'):
                config = env.app.config

                # Dynamically get valid MyST settings from the parser
                # This avoids hard-coding a setting list that will become stale
                parser_instance = MystParser()
                valid_myst_settings = set()

                # Extract setting names from parser's settings_spec
                # settings_spec is a tuple: (title, description, option_spec_tuple)
                if hasattr(parser_instance, 'settings_spec') and parser_instance.settings_spec:
                    # settings_spec[2] contains the tuple of setting definitions
                    for setting_def in parser_instance.settings_spec[2]:
                        # Each setting_def is (description, options, kwargs)
                        # kwargs contains 'dest' which is the setting name
                        if len(setting_def) >= 3 and isinstance(setting_def[2], dict):
                            dest = setting_def[2].get('dest')
                            if dest:
                                valid_myst_settings.add(dest)

                # Copy only valid MyST settings from Sphinx config to parser settings
                for setting_name in valid_myst_settings:
                    if hasattr(config, setting_name):
                        value = getattr(config, setting_name)
                        # Only set non-default values
                        # Note: MyST uses UNSET as a sentinel, but we check for None here
                        # as that's what Sphinx config will have for unset values
                        if value is not None:
                            settings_overrides[setting_name] = value

        # Parse markdown using docutils core API with MyST parser
        doctree = publish_doctree(
            dedented_text,
            parser=MystParser(),
            settings_overrides=settings_overrides
        )

        # Return the list of child nodes directly (not wrapped in a container)
        # This allows the caller to add them to any parent node
        return list(doctree.children)
    except Exception:
        # Fallback to simple paragraph if parsing fails
        return [nodes.paragraph(text=markdown_text.strip())]

def _extract_first_line_doc(doc_string, max_length=100):
    """Extract first line from docstring for summary

    Returns plain text (strips basic markdown), truncated if needed.
    """
    if not doc_string:
        return ""

    # Split by newlines, take first non-empty line
    lines = doc_string.strip().split('\n')
    first_line = next((line.strip() for line in lines if line.strip()), "")

    # Strip basic markdown: **bold**, *italic*, `code`
    first_line = re.sub(r'\*\*([^*]+)\*\*', r'\1', first_line)
    first_line = re.sub(r'\*([^*]+)\*', r'\1', first_line)
    first_line = re.sub(r'`([^`]+)`', r'\1', first_line)

    # Truncate if too long
    if len(first_line) > max_length:
        first_line = first_line[:max_length].rstrip() + "..."

    return first_line

def _create_index_node(fullname, objtype, display_name=None):
    """Create an index node for Sphinx's IndexBuilder

    Args:
        fullname: Fully qualified name (e.g., "pure.MyClass")
        objtype: Object type ('function', 'class', 'data', 'method', 'attribute', 'module')
        display_name: Optional display name for the index entry

    Returns:
        sphinx.addnodes.index node with appropriate entries
    """
    if display_name is None:
        display_name = fullname

    # Create index node with entries list
    # Format: ('entrytype', 'entryname', 'target', 'main', 'key')
    idx = index()
    entries = []

    if objtype == 'module':
        # Module entries: "module; modulename"
        entries.append(('single', f'{fullname} (module)', fullname, '', None))
    elif objtype in ('class', 'function', 'data'):
        # Top-level items: add both bare name and module-scoped entry
        entries.append(('single', f'{display_name}', fullname, '', None))
        if '.' in fullname:
            module_name = fullname.rsplit('.', 1)[0]
            # Match Python domain convention: "func() (in module mod)"
            if objtype == 'function':
                entries.append(('single', f'{display_name}() (in module {module_name})', fullname, '', None))
            else:
                entries.append(('single', f'{display_name} (in module {module_name})', fullname, '', None))
    elif objtype in ('method', 'attribute', 'property'):
        # Nested items: add class-scoped entry
        parts = fullname.rsplit('.', 2)
        if len(parts) >= 3:
            class_name = parts[-2]
            member_name = parts[-1]
            # "method() (ClassName method)"
            if objtype == 'method':
                entries.append(('single', f'{member_name}() ({class_name} method)', fullname, '', None))
            elif objtype == 'property':
                entries.append(('single', f'{member_name} ({class_name} property)', fullname, '', None))
            else:
                entries.append(('single', f'{member_name} ({class_name} attribute)', fullname, '', None))
        else:
            entries.append(('single', f'{display_name}', fullname, '', None))

    idx['entries'] = entries
    idx['inline'] = False
    return idx

# Additional helper functions for refactoring

def _append_myst_doc(parent_node, doc, env):
    """Helper: Parse MyST markdown and append to parent node

    Replaces repeated pattern:
        if doc:
            for node in _parse_myst(doc, env):
                parent.append(node)

    Usage: _append_myst_doc(content, func.get('doc'), env)
    """
    if doc:
        for node in _parse_myst(doc, env):
            parent_node.append(node)

def _register_py_object(env, fullname, objtype, node_id):
    """Helper: Register object with Python domain for cross-references

    Replaces repeated pattern:
        if hasattr(env, 'domaindata'):
            py_domain = env.get_domain('py')
            py_domain.note_object(fullname, objtype, node_id, location=env.docname)

    Usage: _register_py_object(env, fullname, 'function', sig_id)
    """
    if hasattr(env, 'domaindata'):
        py_domain = env.get_domain('py')
        py_domain.note_object(fullname, objtype, node_id, location=env.docname)

def _kind_to_reftype(kind):
    """Helper: Map ItemKind to Sphinx reftype

    Eliminates repeated dict literal:
        kind_to_reftype = {
            'Class': 'class',
            'Function': 'func',
            ...
        }

    Usage: reftype = _kind_to_reftype(link_target['kind'])
    """
    return {
        'Class': 'class',
        'Function': 'func',
        'TypeAlias': 'data',
        'Variable': 'data',
        'Module': 'mod',
    }.get(kind, 'obj')

def _build_callable_signatures(signatures, name, module_name, fullname, env):
    """Build signature nodes for functions/methods with all overloads

    Handles:
    - Multiple overload signatures
    - ID only on first signature (avoids duplicates)
    - Parameter rendering with types and defaults
    - Return type annotation

    Returns: list of desc_signature nodes
    """
    sig_nodes = []

    for idx, sig in enumerate(signatures):
        sig_node = desc_signature(module=module_name, fullname=fullname)
        sig_node['module'] = module_name
        sig_node['fullname'] = fullname

        # Only first signature gets the ID
        if idx == 0:
            sig_node['ids'].append(fullname)
        sig_node['first'] = (idx == 0)

        # Function/method name
        sig_node += desc_name(text=name)

        # Parameters
        param_list = desc_parameterlist()
        for param in sig['parameters']:
            param_node = desc_parameter()
            param_node += nodes.Text(param['name'] + ': ')
            param_node += _build_type_expr(param['type_'])

            if param.get('default'):
                param_node += nodes.Text(' = ')
                param_node += _build_default_value(param['default'])

            param_list += param_node
        sig_node += param_list

        # Return type
        if sig.get('return_type'):
            returns = desc_returns()
            returns += _build_type_expr(sig['return_type'])
            sig_node += returns

        sig_nodes.append(sig_node)

    return sig_nodes

@lru_cache(maxsize=512)
def _parse_and_link_type_cached(type_str):
    """Cached version of _parse_and_link_type

    Many types repeat across signatures (int, str, Optional[...])
    Cache avoids re-parsing regex and building node trees.

    Note: Returns a node object which is mutable, but since we don't
    modify these after creation, caching is safe.
    """
    return _parse_and_link_type(type_str)

@lru_cache(maxsize=256)
def _extract_first_line_doc_cached(doc_string, max_length=100):
    """Cached version of _extract_first_line_doc

    Module contents tables extract first line for every item.
    Cache avoids redundant markdown stripping.
    """
    return _extract_first_line_doc(doc_string, max_length)

def _group_items_by_kind(items):
    """Group module items by kind for contents table

    Returns: dict with keys 'functions', 'classes', 'type_aliases', 'variables'
    Each value is a list of items of that kind.
    """
    groups = {
        'functions': [],
        'classes': [],
        'type_aliases': [],
        'variables': []
    }

    for item in items:
        kind = item['kind']
        if kind == 'Function':
            groups['functions'].append(item)
        elif kind == 'Class':
            groups['classes'].append(item)
        elif kind == 'TypeAlias':
            groups['type_aliases'].append(item)
        elif kind == 'Variable':
            groups['variables'].append(item)

    return groups

def _get_reftype_for_item(item):
    """Get Sphinx reftype for cross-references"""
    kind = item['kind']
    if kind == 'Function':
        return 'func'
    elif kind == 'Class':
        return 'class'
    elif kind == 'TypeAlias':
        return 'data'
    elif kind == 'Variable':
        return 'data'
    return 'obj'

def _build_contents_table(env, category_title, items, module_name):
    """Build a single category table in module contents

    Returns list of nodes (rubric + table).
    """
    from sphinx.addnodes import pending_xref

    # Use rubric for category heading
    rubric = nodes.rubric(text=category_title)

    # Create table structure
    table = nodes.table()
    tgroup = nodes.tgroup(cols=2)
    table += tgroup

    # Column specifications
    tgroup += nodes.colspec(colwidth=30)
    tgroup += nodes.colspec(colwidth=70)

    # Table head
    thead = nodes.thead()
    tgroup += thead
    row = nodes.row()
    thead += row

    # Header cells
    entry = nodes.entry()
    entry += nodes.paragraph(text='Name')
    row += entry

    entry = nodes.entry()
    entry += nodes.paragraph(text='Description')
    row += entry

    # Table body
    tbody = nodes.tbody()
    tgroup += tbody

    for item in items:
        item_name = item['name']
        item_doc = item.get('doc', '')
        item_fqn = f"{module_name}.{item_name}"

        # Extract first line description (using cached version)
        description = _extract_first_line_doc_cached(item_doc)

        # Create table row
        row = nodes.row()
        tbody += row

        # Name cell with cross-reference
        entry = nodes.entry()
        para = nodes.paragraph()
        xref = pending_xref(
            '',
            refdomain='py',
            reftype=_get_reftype_for_item(item),
            reftarget=item_fqn,
            refexplicit=False,
        )
        xref += nodes.literal(text=item_name, classes=['xref', 'py'])
        para += xref
        entry += para
        row += entry

        # Description cell
        entry = nodes.entry()
        if description:
            entry += nodes.paragraph(text=description)
        row += entry

    return [rubric, table]

def _build_module_contents_table(env, doc_module, module_name):
    """Build module contents table with categorized items

    Returns list of nodes (section with rubrics and tables).
    """
    # Group items by kind (using helper)
    groups = _group_items_by_kind(doc_module['items'])

    # Skip if module has no items to show
    if not any(groups.values()):
        return []

    # Create section
    section = nodes.section(ids=[f'{module_name}-contents'])
    section += nodes.title(text='Module Contents')

    # Build each category (in this specific order)
    if groups['classes']:
        section.extend(_build_contents_table(
            env, 'Classes', groups['classes'], module_name
        ))

    if groups['functions']:
        section.extend(_build_contents_table(
            env, 'Functions', groups['functions'], module_name
        ))

    if groups['type_aliases']:
        section.extend(_build_contents_table(
            env, 'Type Aliases', groups['type_aliases'], module_name
        ))

    if groups['variables']:
        section.extend(_build_contents_table(
            env, 'Variables', groups['variables'], module_name
        ))

    return [section]

def _build_deprecated_note(deprecated):
    """Build a deprecation notice paragraph if deprecated info is present."""
    if deprecated is None:
        return None
    since = deprecated.get('since')
    note = deprecated.get('note')
    container = nodes.admonition(classes=['deprecated'])
    title = nodes.title(text='Deprecated')
    container += title
    content_para = nodes.paragraph()
    text_parts = []
    if since:
        text_parts.append(f'since {since}')
    if note:
        if text_parts:
            text_parts.append(f' \u2014 {note}')
        else:
            text_parts.append(note)
    if text_parts:
        content_para += nodes.Text(''.join(text_parts))
    else:
        content_para += nodes.Text('This item is deprecated.')
    container += content_para
    return container

def _build_function(env, func, module_name):
    """Build function with all overload signatures"""
    fullname = f"{module_name}.{func['name']}"

    # CREATE INDEX NODE - targets the first signature
    index_node = _create_index_node(fullname, 'function', func['name'])

    desc_node = desc(domain='py', objtype='function', noindex=False)
    desc_node['classes'].extend(['py', 'function'])

    # Add signature for each overload (using consolidated helper)
    for sig_node in _build_callable_signatures(
        func['signatures'], func['name'], module_name, fullname, env
    ):
        desc_node += sig_node

    # Docstring and deprecation (using helper)
    content = desc_content()
    dep_note = _build_deprecated_note(func.get('deprecated'))
    if dep_note is not None:
        content += dep_note
    if func.get('doc'):
        _append_myst_doc(content, func['doc'], env)
    if len(content.children) > 0:
        desc_node += content

    # Register (using helper)
    _register_py_object(env, fullname, 'function', fullname)

    return [index_node, desc_node]

def _build_type_alias(env, alias, module_name):
    """Build type alias documentation"""
    fullname = f"{module_name}.{alias['name']}"

    # CREATE INDEX NODE
    index_node = _create_index_node(fullname, 'data', alias['name'])

    desc_node = desc(domain='py', objtype='data', noindex=False)

    sig_node = desc_signature(module=module_name, fullname=fullname)
    sig_node['module'] = module_name
    sig_node['fullname'] = fullname
    sig_node['ids'].append(fullname)

    # Use Python 3.12+ type syntax
    sig_node += desc_annotation(text='type ')
    sig_node += desc_name(text=alias['name'])
    sig_node += nodes.Text(' = ')
    sig_node += _build_type_expr(alias['definition'])
    desc_node += sig_node

    # Docstring (using helper)
    if alias.get('doc'):
        content = desc_content()
        _append_myst_doc(content, alias['doc'], env)
        desc_node += content

    # Register (using helper)
    _register_py_object(env, fullname, 'data', fullname)

    return [index_node, desc_node]

def _build_class(env, cls, module_name):
    """Build class documentation"""
    fullname = f"{module_name}.{cls['name']}"
    sig_id = fullname

    # CREATE INDEX NODE for the class
    index_node = _create_index_node(fullname, 'class', cls['name'])

    desc_node = desc(domain='py', objtype='class', noindex=False)
    desc_node['classes'].extend(['py', 'class'])

    sig_node = desc_signature(module=module_name, fullname=fullname)
    sig_node['module'] = module_name
    sig_node['fullname'] = fullname
    sig_node['ids'].append(sig_id)

    # Add "class" prefix annotation with syntax highlighting
    annotation = desc_annotation()
    # Keyword span (class keyword)
    keyword_span = nodes.inline(classes=['k'])
    keyword_span += nodes.Text('class')
    annotation += keyword_span
    # Whitespace span
    ws_span = nodes.inline(classes=['w'])
    ws_span += nodes.Text(' ')
    annotation += ws_span
    sig_node += annotation

    # Add class name
    sig_node += desc_name(text=cls['name'])

    desc_node += sig_node

    # Always create desc_content to hold docstring + methods + attributes
    content = desc_content()

    # Add deprecation note and docstring
    dep_note = _build_deprecated_note(cls.get('deprecated'))
    if dep_note is not None:
        content += dep_note
    _append_myst_doc(content, cls.get('doc'), env)

    # Register with Python domain (using helper)
    _register_py_object(env, fullname, 'class', sig_id)

    # Render class methods
    methods = cls.get('methods', [])
    for method in methods:
        method_fullname = f"{fullname}.{method['name']}"

        # ADD INDEX NODE for method
        method_index = _create_index_node(method_fullname, 'method', method['name'])
        content += method_index

        method_desc = desc(domain='py', objtype='method', noindex=False)
        method_desc['classes'].extend(['py', 'method'])

        # Add signature for each overload (using consolidated helper)
        for sig_node in _build_callable_signatures(
            method['signatures'], method['name'], module_name, method_fullname, env
        ):
            method_desc += sig_node

        # Method deprecation and docstring (using helper)
        method_content = desc_content()
        dep_note = _build_deprecated_note(method.get('deprecated'))
        if dep_note is not None:
            method_content += dep_note
        if method.get('doc'):
            _append_myst_doc(method_content, method['doc'], env)
        if len(method_content.children) > 0:
            method_desc += method_content

        # Register method (using helper)
        _register_py_object(env, method_fullname, 'method', method_fullname)

        content += method_desc

    # Render class attributes and properties
    attributes = cls.get('attributes', [])
    for attr in attributes:
        is_property = attr.get('is_property', False)
        is_readonly = attr.get('is_readonly', False)
        objtype = 'property' if is_property else 'attribute'
        attr_fullname = f"{fullname}.{attr['name']}"

        # ADD INDEX NODE for attribute/property
        attr_index = _create_index_node(attr_fullname, objtype, attr['name'])
        content += attr_index

        attr_desc = desc(domain='py', objtype=objtype, noindex=False)

        sig_node = desc_signature(module=module_name, fullname=attr_fullname)
        sig_node['module'] = module_name
        sig_node['fullname'] = attr_fullname
        sig_node['ids'].append(attr_fullname)

        if is_property:
            sig_node += desc_annotation(text='property ')

        sig_node += desc_name(text=attr['name'])
        if attr.get('type_'):
            sig_node += nodes.Text(': ')
            sig_node += _build_type_expr(attr['type_'])

        attr_desc += sig_node

        # Attribute/property deprecation and docstring (using helper)
        attr_content = desc_content()
        dep_note = _build_deprecated_note(attr.get('deprecated'))
        if dep_note is not None:
            attr_content += dep_note
        if is_readonly:
            attr_content += nodes.paragraph(text='Read-only property.')
        if attr.get('doc'):
            _append_myst_doc(attr_content, attr['doc'], env)
        if len(attr_content.children) > 0:
            attr_desc += attr_content

        # Register attribute/property (using helper)
        _register_py_object(env, attr_fullname, objtype, attr_fullname)

        content += attr_desc

    # Add the complete content block to desc_node
    desc_node += content

    return [index_node, desc_node]

def _build_variable(env, var, module_name):
    """Build variable documentation"""
    fullname = f"{module_name}.{var['name']}"

    # CREATE INDEX NODE
    index_node = _create_index_node(fullname, 'data', var['name'])

    desc_node = desc(domain='py', objtype='data', noindex=False)

    sig_node = desc_signature(module=module_name, fullname=fullname)
    sig_node['module'] = module_name
    sig_node['fullname'] = fullname
    sig_node['ids'].append(fullname)

    sig_node += desc_name(text=var['name'])
    if var.get('type_'):
        sig_node += nodes.Text(': ')
        sig_node += _build_type_expr(var['type_'])
    desc_node += sig_node

    # Docstring (using helper)
    if var.get('doc'):
        content = desc_content()
        _append_myst_doc(content, var['doc'], env)
        desc_node += content

    # Register (using helper)
    _register_py_object(env, fullname, 'data', fullname)

    return [index_node, desc_node]

def _build_submodule(env, submod, parent_module_name):
    """Build documentation for a submodule reference"""
    submod_name = submod['name']
    submod_fqn = submod['fqn']
    submod_doc = submod.get('doc', '')

    # Create a list item with a reference
    list_item = nodes.list_item()
    para = nodes.paragraph()

    # Add the module annotation as strong text
    para += nodes.strong(text='module ')

    # Create a reference to the submodule's documentation page
    # The documentation pages are named after the FQN (e.g., mixed.main_mod.html)
    ref = nodes.reference('', '')
    ref['refuri'] = f'{submod_fqn}.html'
    ref['reftitle'] = f'Link to {submod_fqn} module'
    ref += nodes.literal(text=submod_name, classes=['xref', 'py', 'py-mod'])
    para += ref

    list_item += para

    # Add docstring if present
    if submod_doc:
        for node in _parse_myst(submod_doc, env):
            list_item.append(node)

    # Return just the list item (caller will add to bullet list)
    return [list_item]

class Pyo3APIDirective(SphinxDirective):
    """Render API from pyo3-stub-gen JSON IR"""

    required_arguments = 1  # Module name

    def run(self):
        module_name = self.arguments[0]

        # Load JSON IR - check both api/ subdirectory and srcdir
        json_path = Path(self.env.srcdir) / "api" / "api_reference.json"
        if not json_path.exists():
            json_path = Path(self.env.srcdir) / "api_reference.json"

        with open(json_path) as f:
            doc_package = json.load(f)

        # Find module
        if module_name not in doc_package['modules']:
            return [nodes.error('', nodes.paragraph(
                text=f"Module not found: {module_name}"))]

        doc_module = doc_package['modules'][module_name]

        result = []

        # REGISTER MODULE with Python domain for py-modindex
        if hasattr(self.env, 'domaindata'):
            py_domain = self.env.get_domain('py')
            synopsis = _extract_first_line_doc(doc_module.get('doc', ''))
            # Sphinx 9.x signature: note_module(name, node_id, synopsis, platform, deprecated)
            py_domain.note_module(
                module_name,
                self.env.docname,
                synopsis,
                '',
                False
            )

        # OPTIONALLY: Add module index entry to genindex
        module_index = _create_index_node(module_name, 'module')
        result.append(module_index)

        # Render module docstring if present
        if doc_module.get('doc'):
            result.extend(_parse_myst(doc_module['doc'], self.env))

        # Add module contents table if enabled in config
        if doc_package.get('config', {}).get('contents-table', False):
            result.extend(_build_module_contents_table(self.env, doc_module, module_name))

        # Group items by kind
        functions = [item for item in doc_module['items'] if item['kind'] == 'Function']
        classes = [item for item in doc_module['items'] if item['kind'] == 'Class']
        type_aliases = [item for item in doc_module['items'] if item['kind'] == 'TypeAlias']
        variables = [item for item in doc_module['items'] if item['kind'] == 'Variable']
        modules = [item for item in doc_module['items'] if item['kind'] == 'Module']

        # Submodules section (add FIRST for prominence)
        if modules:
            mod_section = nodes.section(ids=[f'{module_name}-submodules'])
            mod_section += nodes.title(text='Submodules')
            # Create a single bullet list for all submodules
            bullet_list = nodes.bullet_list()
            for submod in modules:
                bullet_list.extend(self._build_submodule(submod, module_name))
            mod_section += bullet_list
            result.append(mod_section)

        # Functions section
        if functions:
            func_section = nodes.section(ids=[f'{module_name}-functions'])
            func_section += nodes.title(text='Functions')
            for func in functions:
                func_section.extend(self._build_function(func, module_name))
            result.append(func_section)

        # Classes section
        if classes:
            class_section = nodes.section(ids=[f'{module_name}-classes'])
            class_section += nodes.title(text='Classes')
            for cls in classes:
                class_section.extend(self._build_class(cls, module_name))
            result.append(class_section)

        # Type Aliases section
        if type_aliases:
            alias_section = nodes.section(ids=[f'{module_name}-type-aliases'])
            alias_section += nodes.title(text='Type Aliases')
            for alias in type_aliases:
                alias_section.extend(self._build_type_alias(alias, module_name))
            result.append(alias_section)

        # Variables section
        if variables:
            var_section = nodes.section(ids=[f'{module_name}-variables'])
            var_section += nodes.title(text='Variables')
            for var in variables:
                var_section.extend(self._build_variable(var, module_name))
            result.append(var_section)

        return result

    def _build_item(self, item, module_name):
        kind = item['kind']
        if kind == 'Function':
            return self._build_function(item, module_name)
        elif kind == 'Class':
            return self._build_class(item, module_name)
        elif kind == 'TypeAlias':
            return self._build_type_alias(item, module_name)
        elif kind == 'Variable':
            return self._build_variable(item, module_name)
        return []

    def _build_function(self, func, module_name):
        return _build_function(self.env, func, module_name)

    def _build_type_alias(self, alias, module_name):
        return _build_type_alias(self.env, alias, module_name)

    def _build_class(self, cls, module_name):
        return _build_class(self.env, cls, module_name)

    def _build_variable(self, var, module_name):
        return _build_variable(self.env, var, module_name)

    def _build_submodule(self, submod, module_name):
        return _build_submodule(self.env, submod, module_name)

class Pyo3APIPackageDirective(SphinxDirective):
    """Render API for all modules in a package from pyo3-stub-gen JSON IR"""

    required_arguments = 1  # Package name

    def run(self):
        package_name = self.arguments[0]

        # Load JSON IR - check both api/ subdirectory and srcdir
        json_path = Path(self.env.srcdir) / "api" / "api_reference.json"
        if not json_path.exists():
            json_path = Path(self.env.srcdir) / "api_reference.json"

        with open(json_path) as f:
            doc_package = json.load(f)

        # Find all modules matching the package
        result = []
        for module_name in sorted(doc_package['modules'].keys()):
            # Include the package itself and all submodules
            if module_name == package_name or module_name.startswith(package_name + '.'):
                doc_module = doc_package['modules'][module_name]

                # REGISTER EACH MODULE
                if hasattr(self.env, 'domaindata'):
                    py_domain = self.env.get_domain('py')
                    synopsis = _extract_first_line_doc(doc_module.get('doc', ''))
                    # Sphinx 9.x signature: note_module(name, node_id, synopsis, platform, deprecated)
                    py_domain.note_module(
                        module_name,
                        self.env.docname,
                        synopsis,
                        '',
                        False
                    )

                # Add section header for each module
                section = nodes.section(ids=[f'module-{module_name}'])
                title_text = f"{module_name} Module" if module_name != package_name else f"{module_name} Package"
                title = nodes.title(text=title_text)
                section += title

                # OPTIONALLY: Add module index entry
                module_index = _create_index_node(module_name, 'module')
                section += module_index

                # Render module docstring if present
                if doc_module.get('doc'):
                    for node in _parse_myst(doc_module['doc'], self.env):
                        section.append(node)

                # Add module contents table if enabled in config
                if doc_package.get('config', {}).get('contents-table', False):
                    for node in _build_module_contents_table(self.env, doc_module, module_name):
                        section.append(node)

                # Group items by kind
                functions = [item for item in doc_module['items'] if item['kind'] == 'Function']
                classes = [item for item in doc_module['items'] if item['kind'] == 'Class']
                type_aliases = [item for item in doc_module['items'] if item['kind'] == 'TypeAlias']
                variables = [item for item in doc_module['items'] if item['kind'] == 'Variable']
                modules = [item for item in doc_module['items'] if item['kind'] == 'Module']

                # Submodules subsection (add FIRST for prominence)
                if modules:
                    mod_section = nodes.section(ids=[f'{module_name}-submodules'])
                    mod_section += nodes.title(text='Submodules')
                    # Create a single bullet list for all submodules
                    bullet_list = nodes.bullet_list()
                    for submod in modules:
                        bullet_list.extend(self._build_submodule(submod, module_name))
                    mod_section += bullet_list
                    section.append(mod_section)

                # Functions subsection
                if functions:
                    func_section = nodes.section(ids=[f'{module_name}-functions'])
                    func_section += nodes.title(text='Functions')
                    for func in functions:
                        func_section.extend(self._build_function(func, module_name))
                    section.append(func_section)

                # Classes subsection
                if classes:
                    class_section = nodes.section(ids=[f'{module_name}-classes'])
                    class_section += nodes.title(text='Classes')
                    for cls in classes:
                        class_section.extend(self._build_class(cls, module_name))
                    section.append(class_section)

                # Type Aliases subsection
                if type_aliases:
                    alias_section = nodes.section(ids=[f'{module_name}-type-aliases'])
                    alias_section += nodes.title(text='Type Aliases')
                    for alias in type_aliases:
                        alias_section.extend(self._build_type_alias(alias, module_name))
                    section.append(alias_section)

                # Variables subsection
                if variables:
                    var_section = nodes.section(ids=[f'{module_name}-variables'])
                    var_section += nodes.title(text='Variables')
                    for var in variables:
                        var_section.extend(self._build_variable(var, module_name))
                    section.append(var_section)

                result.append(section)

        return result

    def _build_item(self, item, module_name):
        kind = item['kind']
        if kind == 'Function':
            return self._build_function(item, module_name)
        elif kind == 'Class':
            return self._build_class(item, module_name)
        elif kind == 'TypeAlias':
            return self._build_type_alias(item, module_name)
        elif kind == 'Variable':
            return self._build_variable(item, module_name)
        return []

    def _build_function(self, func, module_name):
        return _build_function(self.env, func, module_name)

    def _build_type_alias(self, alias, module_name):
        return _build_type_alias(self.env, alias, module_name)

    def _build_class(self, cls, module_name):
        return _build_class(self.env, cls, module_name)

    def _build_variable(self, var, module_name):
        return _build_variable(self.env, var, module_name)

    def _build_submodule(self, submod, module_name):
        return _build_submodule(self.env, submod, module_name)

def setup(app):
    app.add_directive('pyo3-api', Pyo3APIDirective)
    app.add_directive('pyo3-api-package', Pyo3APIPackageDirective)
    return {'version': '0.1', 'parallel_read_safe': True}
