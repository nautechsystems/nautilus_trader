#!/usr/bin/env python3

import traceback
from dataclasses import dataclass
from dataclasses import field
from pathlib import Path

from Cython.Compiler import PyrexTypes
from Cython.Compiler.Main import CompilationOptions
from Cython.Compiler.Main import Context
from Cython.Compiler.Main import default_options
from Cython.Compiler.TreeFragment import parse_from_strings
from Cython.Compiler.Visitor import ScopeTrackingTransform


@dataclass
class MemberVariable:
    """Represents a member variable of a class."""

    name: str
    type_hint: str | None = None
    is_private: bool = False
    default_value: str | None = None
    is_public: bool = False
    is_readonly: bool = False
    is_class: bool = False
    is_instance: bool = False
    line_number: int | None = None


@dataclass
class MethodInfo:
    """Represents a method of a class."""

    name: str
    args: list[str] = field(default_factory=list)
    return_type: str | None = None
    docstring: str | None = None
    is_private: bool = False
    is_static: bool = False
    is_classmethod: bool = False
    is_cdef: bool = False
    is_cpdef: bool = False
    is_property: bool = False
    line_number: int | None = None


@dataclass
class FunctionInfo:
    """Represents a function."""

    name: str
    args: list[str] = field(default_factory=list)
    return_type: str | None = None
    docstring: str | None = None
    is_cdef: bool = False
    is_cpdef: bool = False
    line_number: int | None = None


@dataclass
class GlobalVariable:
    """Represents a global variable."""

    name: str
    type_hint: str | None = None
    value: str | None = None
    is_constant: bool = False
    line_number: int | None = None


@dataclass
class ClassInfo:
    """Represents a class."""

    name: str
    base_classes: list[str] = field(default_factory=list)
    docstring: str | None = None
    member_variables: list[MemberVariable] = field(default_factory=list)
    methods: list[MethodInfo] = field(default_factory=list)
    is_cdef_class: bool = False
    is_extension_type: bool = False
    line_number: int | None = None

class CythonCodeAnalyzer(ScopeTrackingTransform):
    """
    Analyzes Cython code and extracts information about classes, functions, and variables.
    """

    def __init__(self, context):
        super().__init__(context=context)
        self.classes: list[ClassInfo] = []
        self.functions: list[FunctionInfo] = []
        self.global_variables: list[GlobalVariable] = []
        self.current_class: ClassInfo | None = None
        self.current_function: FunctionInfo | MethodInfo | None = None
        self.class_stack: list[ClassInfo] = []

    def visit_ModuleNode(self, node):
        """Visit the root module node."""
        self.visitchildren(node)
        return node

    def visit_CClassDefNode(self, node):
        """Visit a Cython extension type class definition node (cdef class)."""
        return self._visit_class_node(node, is_cdef_class=True, is_extension_type=True)

    def visit_PyClassDefNode(self, node):
        """Visit a Python class definition node (class)."""
        return self._visit_class_node(node, is_cdef_class=False, is_extension_type=False)

    def _visit_class_node(self, node, is_cdef_class: bool = False, is_extension_type: bool = False):
        """Visit common processing for class nodes."""
        base_classes = []
        if hasattr(node, "bases") and node.bases:
            base_classes = [
                name
                for arg in node.bases.args
                if (name := self._extract_name_from_node(arg))
            ]

        docstring = self._extract_doc_from_node(node)
        class_name = getattr(node, "class_name", None) or getattr(node, "name", "Unknown")

        class_info = ClassInfo(
            name=class_name,
            base_classes=base_classes,
            docstring=docstring,
            is_cdef_class=is_cdef_class,
            is_extension_type=is_extension_type,
            line_number=node.pos[1],
        )

        self.class_stack.append(class_info)
        self.current_class = class_info

        self.visitchildren(node)

        self.class_stack.pop()
        self.current_class = self.class_stack[-1] if self.class_stack else None

        self.classes.append(class_info)
        return node

    def visit_CFuncDefNode(self, node):
        """Visit a Cython C function definition node (cdef)."""
        return self._visit_function_node(node, is_cdef=not node.overridable, is_cpdef=node.overridable)

    def visit_DefNode(self, node):
        """Visit a generic function definition node."""
        return self._visit_function_node(node)

    def _visit_function_node(self, node, is_cdef: bool = False, is_cpdef: bool = False):
        """Visit common processing for function/method nodes."""
        if self.current_function: # If we are already in a function, we should not process this node
            self.visitchildren(node)
            self.current_function = None
            return node

        is_cfunc = is_cdef or is_cpdef
        function_name = node.name if not is_cfunc else node.declarator.base.name

        args = self._extract_function_args(node, is_cfunc)
        return_type = self._extract_return_type(node, is_cfunc)
        decorators = self._analyze_decorators(node)
        docstring = self._extract_doc_from_node(node)

        is_private = (function_name.startswith("_") and not function_name.startswith("__")) or function_name.endswith("__")

        if self.current_class:
            method_info = MethodInfo(
                name=function_name,
                args=args,
                return_type=return_type,
                docstring=docstring,
                is_private=is_private,
                is_static=decorators.get("staticmethod", False),
                is_classmethod=decorators.get("classmethod", False),
                is_property=decorators.get("property", False),
                is_cdef=is_cdef,
                is_cpdef=is_cpdef,
                line_number=node.pos[1],
            )
            self.current_class.methods.append(method_info)
            self.current_function = method_info
        else:
            function_info = FunctionInfo(
                name=function_name,
                args=args,
                return_type=return_type,
                docstring=docstring,
                is_cdef=is_cdef,
                is_cpdef=is_cpdef,
                line_number=node.pos[1],
            )
            self.functions.append(function_info)
            self.current_function = function_info

        self.visitchildren(node)
        self.current_function = None
        return node

    def visit_SingleAssignmentNode(self, node):
        """Visit a single assignment node (Python-style variable assignment)."""
        if hasattr(node, "lhs") and hasattr(node, "rhs") and not hasattr(node.rhs, "module_name"):
            var_name = self._extract_name_from_node(node.lhs)
            if var_name:
                type_hint = None
                if hasattr(node.lhs, "annotation") and node.lhs.annotation:
                    type_hint = self._extract_type_from_node(node.lhs.annotation)

                value = self._extract_value_from_node(node.rhs)

                self._add_variable(
                    var_name,
                    type_hint,
                    value,
                    is_class=self.current_class is not None,
                    is_instance=self.current_function is not None,
                    line_number=node.pos[1],
                )

        self.visitchildren(node)
        return node

    def _add_variable(
        self,
        name: str,
        type_hint: str | None,
        value: str | None,
        is_public: bool = False,
        is_readonly: bool = False,
        is_class: bool = False,
        is_instance: bool = False,
        line_number: int | None = None,
    ):
        """Add a variable to the appropriate scope (global, class, or instance)."""
        is_private = name.replace("self.", "").startswith("_") and not name.startswith("__")
        is_constant = name.isupper()

        if is_class and is_instance and name.startswith("self.") and self.current_function and self.current_function.name == "__init__":
            member_var = MemberVariable(
                name=name,
                type_hint=type_hint,
                is_private=is_private,
                default_value=value,
                is_public=is_public,
                is_readonly=is_readonly,
                is_instance=True,
                line_number=line_number,
            )
            if self.current_class:
                self.current_class.member_variables.append(member_var)
        elif is_class and not is_instance:
            member_var = MemberVariable(
                name=name,
                type_hint=type_hint,
                is_private=is_private,
                default_value=value,
                is_public=is_public,
                is_readonly=is_readonly,
                is_class=True,
                line_number=line_number,
            )
            if self.current_class:
                self.current_class.member_variables.append(member_var)
        elif not is_class and not is_instance:
            global_var = GlobalVariable(
                name=name,
                type_hint=type_hint,
                value=value,
                is_constant=is_constant,
                line_number=line_number,
            )
            self.global_variables.append(global_var)

    def _extract_function_args(self, node, is_cfunc: bool) -> list[str]:
        """Extract function parameters."""
        args = []
        node_args = node.declarator.args if is_cfunc else node.args

        for arg in node_args:
            got_name_from_type = False
            if hasattr(arg, "declarator"):
                arg_name = self._extract_name_from_node(arg.declarator)
                if not arg_name and hasattr(arg, "base_type"):
                    arg_name = self._extract_name_from_node(arg.base_type)
                    got_name_from_type = True

            arg_type = None
            if hasattr(arg, "annotation") and arg.annotation:
                arg_type = self._extract_type_from_node(arg.annotation)
            elif hasattr(arg, "type") and arg.type:
                arg_type = self._extract_type_from_node(arg.type)
            elif hasattr(arg, "base_type") and arg.base_type:
                arg_type = self._extract_type_from_node(arg.base_type)
                if got_name_from_type:
                    arg_type = None

            arg_type = self.map_cython_type(arg_type) if arg_type else None

            default_val = None
            if hasattr(arg, "default") and arg.default:
                default_val = self._extract_value_from_node(arg.default)

            if arg_type == "self":
                arg_str = "self"
            else:
                if arg_name == "" and arg_type: # when there is no name and is type
                    arg_name, arg_type = arg_type, None

                arg_str = arg_name
                if arg_type and arg_type != "self":
                    arg_str += f": {arg_type}"
                if default_val:
                    arg_str += f" = {default_val}"

            args.append(arg_str)

        return args

    def _extract_return_type(self, node, is_cfunc: bool) -> str | None:
        """Extract the return type."""
        if is_cfunc:
            return self.map_cython_type(self._extract_name_from_node(node.base_type))
        if hasattr(node, "return_type_annotation") and node.return_type_annotation:
            if hasattr(node.return_type_annotation, "string") and node.return_type_annotation.string:
                return self._extract_type_from_node(node.return_type_annotation)
            return self._extract_type_from_node(node.return_type_annotation.expr)
        return None

    def _analyze_decorators(self, node) -> dict[str, bool]:
        """Analyzes decorators."""
        decorators = {}
        if not hasattr(node, "decorators") or not node.decorators:
            return decorators

        for decorator in node.decorators:
            if hasattr(decorator, "decorator"):
                dec_name = self._extract_name_from_node(decorator.decorator)
                if dec_name in ["staticmethod", "classmethod", "property"]:
                    decorators[dec_name] = True

        return decorators

    def _extract_name_from_node(self, node) -> str | None:
        """Extract a name from a node."""
        if node is None:
            return None
        if hasattr(node, "name"):
            return node.name
        if hasattr(node, "attribute") and hasattr(node, "obj"):
            obj_name = self._extract_name_from_node(node.obj)
            return f"{obj_name}.{node.attribute}" if obj_name else node.attribute
        return str(node) if node else None

    def _extract_type_from_node(self, type_node) -> str | None:
        """Extract a type string from a type node."""
        if type_node is None:
            return None
        if hasattr(type_node, "string") and hasattr(type_node.string, "constant_result"):
            return type_node.string.constant_result
        if hasattr(type_node, "name"):
            return type_node.name
        if isinstance(type_node, PyrexTypes.BaseType):
            return str(type_node)
        if hasattr(type_node, "__str__"):
            return str(type_node)
        return None

    def _extract_value_from_node(self, node) -> str | None:
        """Extract a value as a string from a node."""
        if node is None:
            return None
        if hasattr(node, "value"):
            value = str(node.value)
            return "None" if value == "Py_None" else value
        if hasattr(node, "compile_time_value"):
            return "expr"
        return str(node)

    def _extract_doc_from_node(self, node) -> str | None:
        """Extract the docstring from a node."""
        if node is None or not hasattr(node, "doc"):
            return None
        doc = str(node.doc)
        return doc if doc != "None" else None

    def map_cython_type(self, type_hint: str) -> str:
        """Map Cython types to Python types."""
        type_map = {
            "object": "Any",
            "bint": "bool",
            "double": "float",
            "uint64_t": "int",
            "int64_t": "int",
            "uint32_t": "int",
            "int32_t": "int",
            "uint16_t": "int",
            "int16_t": "int",
            "uint8_t": "int",
            "int8_t": "int",
            "long": "int",
            "void": "None",
        }
        return type_map.get(type_hint, type_hint)

def analyze_cython_code(name: str, code_content: str) -> CythonCodeAnalyzer:
    """Analyzes Cython code and extracts information."""
    options = CompilationOptions(default_options)
    context = Context(include_directories=["./"], compiler_directives={}, options=options)

    try:
        tree = parse_from_strings(name, code_content)
        if tree:
            analyzer = CythonCodeAnalyzer(context)
            analyzer.visit(tree)
            return analyzer
        else:
            raise ValueError("Failed to parse code.")
    except Exception as e:
        print(f"An error occurred during analysis: {e}")
        traceback.print_exc()
        return CythonCodeAnalyzer(context)


def print_results(analyzer: CythonCodeAnalyzer):  # noqa: C901
    """Print the analysis results."""
    if analyzer.classes:
        print("\n Classes:")
        for cls in analyzer.classes:
            class_type = "cdef class" if cls.is_cdef_class else "class"
            extension_info = " (extension type)" if cls.is_extension_type else ""
            print(f"\n{class_type}: {cls.name}{extension_info}")

            if cls.base_classes:
                print(f"  Inherits: {', '.join(cls.base_classes)}")

            if cls.docstring:
                print(f'  """{cls.docstring}"""')

            if cls.member_variables:
                print("  Member Variables:")
                for var in cls.member_variables:
                    visibility = "private" if var.is_private else "public"
                    type_info = f": {var.type_hint}" if var.type_hint else ""
                    default_info = f" = {var.default_value}" if var.default_value else ""

                    modifiers = []
                    if var.is_public:
                        modifiers.append("public")
                    if var.is_readonly:
                        modifiers.append("readonly")
                    modifier_str = f" ({', '.join(modifiers)})" if modifiers else ""
                    scope = "instance" if var.is_instance else "class"

                    print(f"    - {var.name}{type_info}{default_info} ({visibility}){modifier_str} {scope}")

            if cls.methods:
                print("  Methods:")
                for method in cls.methods:
                    visibility = "private" if method.is_private else "public"

                    func_type = "def "
                    if method.is_cdef:
                        func_type = "cdef "
                    elif method.is_cpdef:
                        func_type = "cpdef "

                    decorators = []
                    if method.is_static:
                        decorators.append("@staticmethod")
                    if method.is_classmethod:
                        decorators.append("@classmethod")
                    if method.is_property:
                        decorators.append("@property")

                    decorator_str = " ".join(decorators) + " " if decorators else ""
                    args_str = ", ".join(method.args) if method.args else ""
                    return_str = f" -> {method.return_type}" if method.return_type else ""

                    print(f"    - {decorator_str}{func_type}{method.name}({args_str}){return_str} ({visibility})")
                    if method.docstring:
                        print(f'    """{method.docstring}"""')

    if analyzer.functions:
        print("\n  Functions:")
        for func in analyzer.functions:
            func_type = "def "
            if func.is_cdef:
                func_type = "cdef "
            elif func.is_cpdef:
                func_type = "cpdef "

            args_str = ", ".join(func.args) if func.args else ""
            return_str = f" -> {func.return_type}" if func.return_type else ""
            print(f"  - {func_type}{func.name}({args_str}){return_str}")
            if func.docstring:
                print(f'  """{func.docstring}"""')

    if analyzer.global_variables:
        print("\n  Global Variables:")
        for var in analyzer.global_variables:
            type_info = f": {var.type_hint}" if var.type_hint else ""
            value_info = f" = {var.value}" if var.value else ""
            classification = "Constant" if var.is_constant else "Variable"
            print(f"  - {var.name}{type_info}{value_info} ({classification})")

if __name__ == "__main__":
    file_path = Path("/Users/sam/Documents/Development/woung717/nautilus_trader/nautilus_trader/data/messages.pyx")
    code = file_path.read_text(encoding="utf-8")

    analyzer_result = analyze_cython_code(name=str(file_path), code_content=code)
    print_results(analyzer_result)
