#!/usr/bin/env python3
"""
A script validates that classes, methods, member variables, functions, global variables,
docstrings, and type annotations from Cython .pyx files are correctly extracted
to their corresponding .pyi stub files.
"""

import argparse
import ast
import re
import sys
from dataclasses import dataclass
from dataclasses import field
from pathlib import Path

from cython_parser import ClassInfo as CythonClassInfo
from cython_parser import FunctionInfo as CythonFunctionInfo
from cython_parser import GlobalVariable as CythonGlobalVariable
from cython_parser import MemberVariable as CythonMemberVariable
from cython_parser import MethodInfo as CythonMethodInfo
from cython_parser import analyze_cython_code


@dataclass
class PyiMember:
    """Class member information"""

    name: str
    type_hint: str | None = None
    is_method: bool = False
    is_property: bool = False
    is_staticmethod: bool = False
    is_classmethod: bool = False
    is_overload: bool = False
    docstring: str | None = None
    is_private: bool = False
    parameters: list[str] = None
    return_type: str | None = None
    line_number: int | None = None
    ignore_validation: bool = False
    ignored_params: set[str] = field(default_factory=set)

    def __post_init__(self):
        if self.parameters is None:
            self.parameters = []
        self.is_private = self.name.startswith("_")


@dataclass
class PyiFunction:
    """Top-level function information"""

    name: str
    parameters: list[str] = field(default_factory=list)
    return_type: str | None = None
    docstring: str | None = None
    is_private: bool = False
    is_overload: bool = False
    line_number: int | None = None
    ignore_validation: bool = False
    ignored_params: set[str] = field(default_factory=set)

    def __post_init__(self):
        self.is_private = self.name.startswith("_")


@dataclass
class PyiGlobalVariable:
    """Global variable information"""

    name: str
    type_hint: str | None = None
    value: str | None = None
    is_private: bool = False
    line_number: int | None = None
    ignore_validation: bool = False

    def __post_init__(self):
        self.is_private = self.name.startswith("_")


@dataclass
class PyiClassInfo:
    """Class information"""

    name: str
    docstring: str | None = None
    members: dict[str, PyiMember] = None
    base_classes: list[str] = None
    line_number: int | None = None

    def __post_init__(self):
        if self.members is None:
            self.members = {}
        if self.base_classes is None:
            self.base_classes = []


class PyiParser:
    """Python .pyi stub file parser"""

    def __init__(self, file_path: Path):
        self.file_path = file_path
        self.file_content = self.file_path.read_text(encoding="utf-8")
        self.file_lines = self.file_content.splitlines()
        self.classes: dict[str, PyiClassInfo] = {}
        self.functions: dict[str, PyiFunction] = {}
        self.global_variables: dict[str, PyiGlobalVariable] = {}

    def parse(self) -> tuple[dict[str, PyiClassInfo], dict[str, PyiFunction], dict[str, PyiGlobalVariable]]:
        """Parse Pyi file to extract class, function, and global variable information"""
        try:
            tree = ast.parse(self.file_content)
        except Exception as e:
            print(f"Error parsing {self.file_path}: {e}")
            return {}, {}, {}

        for node in tree.body:
            if isinstance(node, ast.ClassDef):
                class_info = self._parse_class(node)
                self.classes[class_info.name] = class_info
            elif isinstance(node, ast.FunctionDef):
                function_info = self._parse_function(node)
                self.functions[function_info.name] = function_info
            elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
                # Global variable with type annotation
                var_info = self._parse_global_variable_annotated(node)
                self.global_variables[var_info.name] = var_info
            elif isinstance(node, ast.Assign):
                # Regular global variable assignment
                for var_info in self._parse_global_variable_assign(node):
                    self.global_variables[var_info.name] = var_info

        return self.classes, self.functions, self.global_variables

    def _is_ignored(self, node: ast.AST) -> bool:
        """Check if the source line ends with # skip-validate."""
        if not hasattr(node, "lineno"):
            return False
        line_index = node.lineno - 1
        if 0 <= line_index < len(self.file_lines):
            line = self.file_lines[line_index].rstrip()
            return line.endswith("# skip-validate")
        return False

    def _parse_class(self, node: ast.ClassDef) -> PyiClassInfo:
        """Parse class node"""
        # Base classes
        base_classes = []
        for base in node.bases:
            if isinstance(base, ast.Name):
                base_classes.append(base.id)
            elif isinstance(base, ast.Attribute):
                base_classes.append(ast.unparse(base))

        class_info = PyiClassInfo(
            name=node.name,
            docstring=ast.get_docstring(node),
            base_classes=base_classes,
            line_number=node.lineno
        )

        # Parse members
        for item in node.body:
            if isinstance(item, ast.FunctionDef) or isinstance(item, ast.AsyncFunctionDef):
                # Extract parameter information in more detail
                parameters = []
                ignored_params = set()
                for arg in item.args.args:
                    if self._is_ignored(arg):
                        ignored_params.add(arg.arg)
                    param_str = arg.arg
                    if arg.annotation:
                        param_str += f": {ast.unparse(arg.annotation)}"
                    parameters.append(param_str)

                # Handle parameters with default values
                defaults = item.args.defaults
                if defaults:
                    # Start index of parameters with default values
                    defaults_start = len(parameters) - len(defaults)
                    for i, default in enumerate(defaults):
                        param_idx = defaults_start + i
                        if param_idx < len(parameters):
                            parameters[param_idx] += f" = {ast.unparse(default)}"

                # Analyze decorators
                is_property = False
                is_staticmethod = False
                is_classmethod = False
                is_overload = False

                for decorator in item.decorator_list:
                    if isinstance(decorator, ast.Name):
                        if decorator.id == "property":
                            is_property = True
                        elif decorator.id == "staticmethod":
                            is_staticmethod = True
                        elif decorator.id == "classmethod":
                            is_classmethod = True
                        elif decorator.id == "overload":
                            is_overload = True
                    elif isinstance(decorator, ast.Attribute):
                        # For cases like typing.overload
                        decorator_name = ast.unparse(decorator)
                        if "overload" in decorator_name:
                            is_overload = True

                member = PyiMember(
                    name=item.name,
                    is_method=True,
                    is_property=is_property,
                    is_staticmethod=is_staticmethod,
                    is_classmethod=is_classmethod,
                    is_overload=is_overload,
                    parameters=parameters,
                    return_type=ast.unparse(item.returns) if item.returns else None,
                    docstring=ast.get_docstring(item),
                    line_number=item.lineno,
                    ignore_validation=self._is_ignored(item),
                )
                member.ignored_params = ignored_params
                class_info.members[item.name] = member

            elif isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
                # Variable with type annotation
                member = PyiMember(
                    name=item.target.id,
                    type_hint=ast.unparse(item.annotation),
                    line_number=item.lineno,
                    ignore_validation=self._is_ignored(item),
                )
                class_info.members[item.target.id] = member

            elif isinstance(item, ast.Assign):
                # Regular variable assignment
                for target in item.targets:
                    if isinstance(target, ast.Name):
                        member = PyiMember(
                            name=target.id,
                            line_number=item.lineno,
                            ignore_validation=self._is_ignored(item),
                        )
                        class_info.members[target.id] = member

        return class_info

    def _parse_function(self, node: ast.FunctionDef) -> PyiFunction:
        """Parse top-level function"""
        # Extract parameter information
        parameters = []
        ignored_params = set()
        for arg in node.args.args:
            if self._is_ignored(arg):
                ignored_params.add(arg.arg)
            param_str = arg.arg
            if arg.annotation:
                param_str += f": {ast.unparse(arg.annotation)}"
            parameters.append(param_str)

        # Handle parameters with default values
        defaults = node.args.defaults
        if defaults:
            defaults_start = len(parameters) - len(defaults)
            for i, default in enumerate(defaults):
                param_idx = defaults_start + i
                if param_idx < len(parameters):
                    parameters[param_idx] += f" = {ast.unparse(default)}"

        # Analyze decorators
        is_overload = False
        for decorator in node.decorator_list:
            if isinstance(decorator, ast.Name):
                if decorator.id == "overload":
                    is_overload = True
            elif isinstance(decorator, ast.Attribute):
                decorator_name = ast.unparse(decorator)
                if "overload" in decorator_name:
                    is_overload = True

        function_info = PyiFunction(
            name=node.name,
            parameters=parameters,
            return_type=ast.unparse(node.returns) if node.returns else None,
            docstring=ast.get_docstring(node),
            is_overload=is_overload,
            line_number=node.lineno,
            ignore_validation=self._is_ignored(node),
        )
        function_info.ignored_params = ignored_params
        return function_info

    def _parse_global_variable_annotated(self, node: ast.AnnAssign) -> PyiGlobalVariable:
        """Parse global variable with type annotation"""
        value = None
        if node.value:
            try:
                value = ast.unparse(node.value)
            except Exception:
                value = str(node.value)

        return PyiGlobalVariable(
            name=node.target.id,
            type_hint=ast.unparse(node.annotation),
            value=value,
            line_number=node.lineno,
            ignore_validation=self._is_ignored(node),
        )

    def _parse_global_variable_assign(self, node: ast.Assign) -> list[PyiGlobalVariable]:
        """Parse regular global variable assignment"""
        variables = []
        value = None
        if node.value:
            try:
                value = ast.unparse(node.value)
            except Exception:
                value = str(node.value)

        for target in node.targets:
            if isinstance(target, ast.Name):
                var_info = PyiGlobalVariable(
                    name=target.id,
                    value=value,
                    line_number=node.lineno,
                    ignore_validation=self._is_ignored(node),
                )
                variables.append(var_info)

        return variables


class PyxPyiValidator:
    """PYX and PYI file validator"""

    def __init__(self, pyx_file: Path, pyi_file: Path, include_private: bool = False, pass_warning: bool = False):
        self.pyx_file = pyx_file
        self.pyi_file = pyi_file
        self.include_private = include_private
        self.pass_warning = pass_warning
        self.pyx_classes: dict[str, CythonClassInfo] = {}
        self.pyx_functions: dict[str, CythonFunctionInfo] = {}
        self.pyx_global_variables: dict[str, CythonGlobalVariable] = {}
        self.pyi_classes: dict[str, PyiClassInfo] = {}
        self.pyi_functions: dict[str, PyiFunction] = {}
        self.pyi_global_variables: dict[str, PyiGlobalVariable] = {}
        self.errors = []
        self.warnings = []
        self.COLLECTIONS = ["list", "tuple", "set", "dict"]

    def validate(self) -> bool:
        """Perform validation"""
        print(f"Validating {self.pyx_file} -> {self.pyi_file}")

        # Check file existence
        if not self.pyx_file.exists():
            self.errors.append(f"PYX file not found: {self.pyx_file}")
            return False

        if not self.pyi_file.exists():
            self.errors.append(f"PYI file not found: {self.pyi_file}")
            return False

        # Parse files
        try:
            pyx_analyzer = analyze_cython_code(name=str(self.pyx_file), code_content=self.pyx_file.read_text(encoding="utf-8"))
            self.pyx_classes = {cls.name: cls for cls in pyx_analyzer.classes}
            self.pyx_functions = {func.name: func for func in pyx_analyzer.functions}
            self.pyx_global_variables = {var.name: var for var in pyx_analyzer.global_variables}
        except Exception as e:
            self.errors.append(f"Error parsing PYX file: {e}")
            return False

        try:
            pyi_parser = PyiParser(self.pyi_file)
            self.pyi_classes, self.pyi_functions, self.pyi_global_variables = pyi_parser.parse()
        except Exception as e:
            self.errors.append(f"Error parsing PYI file: {e}")
            return False

        # Perform validation
        self._validate_classes()
        self._validate_functions()
        self._validate_global_variables()

        return len(self.errors) == 0 and (True if self.pass_warning else len(self.warnings) == 0)

    def _validate_classes(self):
        """Validate classes"""
        pyx_class_names = set(self.pyx_classes.keys())
        pyi_class_names = set(self.pyi_classes.keys())

        # Missing classes
        missing_classes = pyx_class_names - pyi_class_names
        for class_name in missing_classes:
            pyx_line = self.pyx_classes[class_name].line_number
            self.errors.append(f"Class '{class_name}' missing in PYI file ({self.pyx_file.name}:{pyx_line})")

        # Extra classes
        extra_classes = pyi_class_names - pyx_class_names
        for class_name in extra_classes:
            pyi_line = self.pyi_classes[class_name].line_number
            self.errors.append(f"Class '{class_name}' in PYI but not in PYX ({self.pyi_file.name}:{pyi_line})")

        # Validate common classes
        common_classes = pyx_class_names & pyi_class_names
        for class_name in common_classes:
            self._validate_class(
                self.pyx_classes[class_name],
                self.pyi_classes[class_name]
            )

    def _validate_functions(self):
        """Validate top-level functions"""
        # Determine validation targets based on whether to include private functions
        if self.include_private:
            pyx_functions_to_validate = self.pyx_functions
        else:
            pyx_functions_to_validate = {name: func for name, func in self.pyx_functions.items()
                                       if not func.name.startswith("_")}

        pyx_function_names = set(pyx_functions_to_validate.keys())
        pyi_function_names = set(self.pyi_functions.keys())

        # Missing functions
        missing_functions = pyx_function_names - pyi_function_names
        for function_name in missing_functions:
            function = pyx_functions_to_validate[function_name]
            pyx_line = function.line_number
            if not function.is_cdef:
                self.errors.append(f"Function '{function_name}' missing in PYI ({self.pyx_file.name}:{pyx_line})")

        # Extra functions
        extra_functions = pyi_function_names - pyx_function_names
        for function_name in extra_functions:
            pyi_function = self.pyi_functions[function_name]
            if not pyi_function.ignore_validation:
                self.errors.append(f"Function '{function_name}' in PYI but not in PYX ({self.pyi_file.name}:{pyi_function.line_number})")

        # Validate common functions
        common_functions = pyx_function_names & pyi_function_names
        for function_name in common_functions:
            if self.pyi_functions[function_name].ignore_validation:
                continue
            pyx_function = pyx_functions_to_validate[function_name]
            pyi_function = self.pyi_functions[function_name]

            if pyx_function.is_cdef:
                # Skip comparison for cdef functions
                continue

            self._validate_function(function_name, pyx_function, pyi_function)

    def _validate_global_variables(self):
        """Validate global variables"""
        # Determine validation targets based on whether to include private variables
        if self.include_private:
            pyx_variables_to_validate = self.pyx_global_variables
        else:
            pyx_variables_to_validate = {name: var for name, var in self.pyx_global_variables.items()
                                       if not var.name.startswith("_")}

        pyx_variable_names = set(pyx_variables_to_validate.keys())
        pyi_variable_names = set(self.pyi_global_variables.keys())

        # Missing variables
        missing_variables = pyx_variable_names - pyi_variable_names
        for variable_name in missing_variables:
            variable = pyx_variables_to_validate[variable_name]
            pyx_line = variable.line_number
            self.errors.append(f"Global variable '{variable_name}' missing in PYI ({self.pyx_file.name}:{pyx_line})")

        # Extra variables
        extra_variables = pyi_variable_names - pyx_variable_names
        for variable_name in extra_variables:
            pyi_variable = self.pyi_global_variables[variable_name]
            if not pyi_variable.ignore_validation:
                self.errors.append(f"Global variable '{variable_name}' in PYI but not in PYX ({self.pyi_file.name}:{pyi_variable.line_number})")

        # Validate common variables
        common_variables = pyx_variable_names & pyi_variable_names
        for variable_name in common_variables:
            if self.pyi_global_variables[variable_name].ignore_validation:
                continue
            pyx_variable = pyx_variables_to_validate[variable_name]
            pyi_variable = self.pyi_global_variables[variable_name]

            self._validate_global_variable(variable_name, pyx_variable, pyi_variable)

    def _validate_class(self, pyx_class: CythonClassInfo, pyi_class: PyiClassInfo):
        """Validate individual class"""
        class_name = pyx_class.name

        # Validate base classes
        if set(pyx_class.base_classes) != set(pyi_class.base_classes):
            pyx_line = pyx_class.line_number
            pyi_line = pyi_class.line_number
            self.errors.append(
                f"Class '{class_name}': base classes mismatch. "
                f".pyx: {pyx_class.base_classes} ({self.pyx_file.name}:{pyx_line}), "
                f".pyi: {pyi_class.base_classes} ({self.pyi_file.name}:{pyi_line})"
            )

        # Validate docstring
        if pyx_class.docstring and not pyi_class.docstring:
            pyx_line = pyx_class.line_number
            pyi_line = pyi_class.line_number
            self.errors.append(f"Class '{class_name}': docstring missing in PYI ({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})")
        elif pyx_class.docstring and pyi_class.docstring:
            # Compare after removing whitespace
            pyx_doc = pyx_class.docstring.replace(" ", "").strip("\n")
            pyi_doc = pyi_class.docstring.replace(" ", "").strip("\n")
            if pyx_doc != pyi_doc:
                pyx_line = pyx_class.line_number
                pyi_line = pyi_class.line_number
                self.errors.append(f"Class '{class_name}': docstring differs ({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})")

        # Validate members
        self._validate_members(class_name, pyx_class.methods, pyx_class.member_variables, pyi_class.members)

    def _validate_function(self, function_name: str, pyx_function: CythonFunctionInfo, pyi_function: PyiFunction):
        """Validate individual function"""
        pyx_line = pyx_function.line_number
        pyi_line = pyi_function.line_number

        # Validate parameters
        self._validate_function_parameters(function_name, pyx_function, pyi_function)

        # Validate return type
        pyx_return = pyx_function.return_type.strip() if pyx_function.return_type else ""
        pyi_return = pyi_function.return_type.strip() if pyi_function.return_type else ""
        pyx_return_normalized = self._normalize_cython_type(pyx_return)
        pyi_return_normalized = self._normalize_cython_type(pyi_return)

        if pyx_return and not pyi_return:
            self.errors.append(
                f"Function '{function_name}' return type missing in PYI "
                f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
            )
        elif pyx_return and pyi_return and \
             pyx_return_normalized != pyi_return_normalized and \
             not self._is_pyi_type_more_specific(pyx_return_normalized, pyi_return_normalized):
            self.errors.append(
                f"Function '{function_name}' return type mismatch "
                f"(.pyx: '{pyx_return}' {self.pyx_file.name}:{pyx_line}, .pyi: '{pyi_return}' {self.pyi_file.name}:{pyi_line})"
            )

        # Validate docstring
        if pyx_function.docstring and not pyi_function.docstring:
            self.errors.append(
                f"Function '{function_name}' docstring missing in PYI "
                f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
            )

    def _validate_global_variable(self, variable_name: str, pyx_variable: CythonGlobalVariable, pyi_variable: PyiGlobalVariable):
        """Validate individual global variable"""
        pyx_line = pyx_variable.line_number
        pyi_line = pyi_variable.line_number

        pyx_type_normalized = self._normalize_cython_type(pyx_variable.type_hint) if pyx_variable.type_hint else ""
        pyi_type_normalized = self._normalize_cython_type(pyi_variable.type_hint) if pyi_variable.type_hint else ""

        if pyx_type_normalized != pyi_type_normalized and \
           not self._is_pyi_type_more_specific(pyx_type_normalized, pyi_type_normalized):
            self.errors.append(
                f"Global variable '{variable_name}' type mismatch "
                f"(.pyx: {pyx_variable.type_hint} {self.pyx_file.name}:{pyx_line}, "
                f".pyi: {pyi_variable.type_hint} {self.pyi_file.name}:{pyi_line})"
            )

    def _validate_members(self, class_name: str, pyx_methods: list[CythonMethodInfo], pyx_member_variables: list[CythonMemberVariable], pyi_members: dict[str, PyiMember]):
        """Validate class members"""
        pyx_combined_members = {}
        for method in pyx_methods:
            pyx_combined_members[method.name.replace("self.", "")] = method
        for var in pyx_member_variables:
            pyx_combined_members[var.name.replace("self.", "")] = var

        # Determine validation targets based on whether to include private members
        if self.include_private:
            pyx_members_to_validate = pyx_combined_members
        else:
            pyx_members_to_validate = {name: member for name, member in pyx_combined_members.items()
                                       if not member.is_private}

        pyx_member_names = set(pyx_members_to_validate.keys())
        pyi_member_names = set(pyi_members.keys())

        # Missing members
        missing_members = pyx_member_names - pyi_member_names
        for member_name in missing_members:
            member = pyx_members_to_validate[member_name]
            pyx_line = member.line_number
            if isinstance(member, CythonMethodInfo) and not member.is_cdef:
                self.errors.append(f"Class '{class_name}': member '{member_name}' missing in PYI ({self.pyx_file.name}:{pyx_line})")

        # Extra members
        extra_members = pyi_member_names - pyx_member_names
        for member_name in extra_members:
            pyi_member = pyi_members[member_name]
            if not pyi_member.ignore_validation:
                self.errors.append(f"Class '{class_name}': member '{member_name}' in PYI but not in PYX ({self.pyi_file.name}:{pyi_member.line_number})")

        # Validate common members
        common_members = pyx_member_names & pyi_member_names
        for member_name in common_members:
            if pyi_members[member_name].ignore_validation:
                continue
            pyx_member = pyx_members_to_validate[member_name]
            pyi_member = pyi_members[member_name]

            if isinstance(pyx_member, CythonMethodInfo):
                if pyx_member.is_cdef:
                    # Skip comparison for cdef functions
                    continue
                if pyi_member.is_method:
                    self._validate_method(class_name, member_name, pyx_member, pyi_member)
                else:
                    self.errors.append(
                        f"Class '{class_name}': member '{member_name}' type mismatch (method/variable) "
                        f"(.pyx: {type(pyx_member).__name__}, .pyi: {type(pyi_member).__name__})"
                    )
            elif isinstance(pyx_member, CythonMemberVariable) and not pyi_member.is_method:
                self._validate_member_variable(class_name, member_name, pyx_member, pyi_member)
            else:
                self.errors.append(
                    f"Class '{class_name}': member '{member_name}' type mismatch (method/variable) "
                    f"(.pyx: {type(pyx_member).__name__}, .pyi: {type(pyi_member).__name__})"
                )

    def _normalize_parameter(self, param: str) -> tuple[str, str]:
        """Normalize parameter to separate name and type (supports both Cython and Python)"""
        param = param.strip()

        # Remove default value (after =)
        if "=" in param:
            param = param.split("=")[0].strip()

        # Python style: name: type
        if ":" in param:
            name, type_hint = param.split(":", 1)
            return name.strip(), type_hint.strip()

        # For cases where Cython normalized form is already Python style
        # This should have been processed by _normalize_cython_parameter
        tokens = param.split()
        if len(tokens) == 1:
            # Only name
            return tokens[0], ""
        else:
            # Unexpected form, treat entire string as name
            return param, ""

    def _normalize_cython_type(self, cython_type: str) -> str:
        """Normalize Cython types to their Python equivalents for comparison"""
        if not cython_type:
            return cython_type

        # Create a mapping of Cython types to Python types
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
            # Add more mappings as needed
        }

        # Clean the type string (remove whitespace, make lowercase)
        cleaned_type = cython_type.strip().lower()

        for cython_type, python_type in type_map.items():
            if cython_type in cleaned_type:
                cleaned_type = cleaned_type.replace(cython_type, python_type)

        cleaned_type = cleaned_type.lower()

        return cleaned_type

    def _parse_union_types(self, type_str: str) -> set[str]:
        """Parse a union type string (e.g., "Union[int, None]" or "int | None") into a set of individual types."""
        type_str = type_str.strip()
        if type_str.startswith("union[") and type_str.endswith("]"):
            # Handle Union[type1, type2]
            content = type_str[len("union["):-1]
            return {t.strip().lower() for t in content.split(",")}
        elif "|" in type_str:
            # Handle type1 | type2
            return {t.strip().lower() for t in type_str.split("|")}
        return {type_str.lower()}

    def is_specific_generic(self, pyx_type: str, pyi_type: str) -> bool:
        # Handle generic collections
        if pyx_type in self.COLLECTIONS:
            # Check if pyi_type starts with pyx_type followed by '['
            if pyi_type.startswith(pyx_type + "[") and pyi_type.endswith("]"):
                return True
        return False

    def _is_pyi_type_more_specific(self, pyx_type: str, pyi_type: str) -> bool:
        """
        Check if the PYI type is a more specific version of the PYX type.
        e.g., pyx_type="list", pyi_type="list[int]" -> True
        e.g., pyx_type="None", pyi_type="int | None" -> True
        """
        if not pyx_type or pyx_type == "any":
            return True

        if not pyi_type:
            return False

        pyx_type_lower = pyx_type.strip().lower()
        pyi_type_lower = pyi_type.strip().lower()

        pyx_type_lower = re.sub(r"\[.+\]", "", pyx_type_lower)
        pyi_type_lower = re.sub(r"\[.+\]", "", pyi_type_lower)

        if pyx_type_lower == pyi_type_lower:
            return True

        # Handle Union types where PYX might be a single type and PYI is Union[PYX_TYPE, None] or PYX_TYPE | None
        pyi_union_types = self._parse_union_types(pyi_type_lower)
        if (pyx_type_lower in [pyi_union.split(".")[-1] for pyi_union in pyi_union_types] or \
                any(pyx_type_lower == pyi_union for pyi_union in pyi_union_types)) and \
            "none" in pyi_union_types:
            return True

        return False

    def _validate_method(self, class_name: str, method_name: str, pyx_method: CythonMethodInfo, pyi_member: PyiMember):
        """Validate individual method (with decorator validation)"""
        pyx_line = pyx_method.line_number
        pyi_line = pyi_member.line_number

        # Validate decorators
        if pyx_method.is_property != pyi_member.is_property:
            self.errors.append(
                f"Class '{class_name}': method '{method_name}' @property decorator mismatch "
                f"(.pyx: {pyx_method.is_property} {self.pyx_file.name}:{pyx_line}, "
                f".pyi: {pyi_member.is_property} {self.pyi_file.name}:{pyi_line})"
            )

        if pyx_method.is_static != pyi_member.is_staticmethod:
            self.errors.append(
                f"Class '{class_name}': method '{method_name}' @staticmethod decorator mismatch "
                f"(.pyx: {pyx_method.is_static} {self.pyx_file.name}:{pyx_line}, "
                f".pyi: {pyi_member.is_staticmethod} {self.pyi_file.name}:{pyi_line})"
            )

        if pyx_method.is_classmethod != pyi_member.is_classmethod:
            self.errors.append(
                f"Class '{class_name}': method '{method_name}' @classmethod decorator mismatch "
                f"(.pyx: {pyx_method.is_classmethod} {self.pyx_file.name}:{pyx_line}, "
                f".pyi: {pyi_member.is_classmethod} {self.pyi_file.name}:{pyi_line})"
            )

        # CythonCodeAnalyzer does not directly provide @overload info, so we skip this check for now
        # if pyx_method.is_overload != pyi_member.is_overload:
        #     self.warnings.append(
        #         f"Class '{class_name}': method '{method_name}' @overload decorator mismatch "
        #         f"(.pyx: {pyx_method.is_overload} {self.pyx_file.name}, "
        #         f".pyi: {pyi_member.is_overload} {self.pyi_file.name}:{pyi_line})"
        #     )

        # Validate parameters
        self._validate_method_parameters(class_name, method_name, pyx_method, pyi_member)

        # Validate return type
        pyx_return = pyx_method.return_type.strip() if pyx_method.return_type else ""
        pyi_return = pyi_member.return_type.strip() if pyi_member.return_type else ""
        pyx_return_normalized = self._normalize_cython_type(pyx_return)
        pyi_return_normalized = self._normalize_cython_type(pyi_return)

        if pyx_return and not pyi_return:
            self.errors.append(
                f"Class '{class_name}': method '{method_name}' return type missing in PYI "
                f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
            )
        elif pyx_return and pyi_return and \
             pyx_return_normalized != pyi_return_normalized and \
             not self._is_pyi_type_more_specific(pyx_return_normalized, pyi_return_normalized):
            self.errors.append(
                f"Class '{class_name}': method '{method_name}' return type mismatch "
                f"(.pyx: '{pyx_return}' {self.pyx_file.name}:{pyx_line}, .pyi: '{pyi_return}' {self.pyi_file.name}:{pyi_line})"
            )

        # Validate docstring
        if pyx_method.docstring and not pyi_member.docstring:
            self.errors.append(
                f"Class '{class_name}': method '{method_name}' docstring missing in PYI "
                f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
            )

    def _validate_member_variable(self, class_name: str, member_name: str, pyx_member: CythonMemberVariable, pyi_member: PyiMember):
        """Validate individual member variable"""
        pyx_line = pyx_member.line_number
        pyi_line = pyi_member.line_number

        pyx_type_normalized = self._normalize_cython_type(pyx_member.type_hint) if pyx_member.type_hint else ""
        pyi_type_normalized = self._normalize_cython_type(pyi_member.type_hint) if pyi_member.type_hint else ""

        if pyx_type_normalized != pyi_type_normalized and \
           not self._is_pyi_type_more_specific(pyx_type_normalized, pyi_type_normalized):
            self.errors.append(
                f"Class '{class_name}': member '{member_name}' type mismatch "
                f"(.pyx: {pyx_member.type_hint} {self.pyx_file.name}:{pyx_line}, "
                f".pyi: {pyi_member.type_hint} {self.pyi_file.name}:{pyi_line})"
            )

    def _validate_method_parameters(self, class_name: str, method_name: str, pyx_method: CythonMethodInfo, pyi_member: PyiMember):
        """Validate method parameters"""
        pyx_params = pyx_method.args or []
        pyi_params = pyi_member.parameters or []
        pyx_line = pyx_method.line_number
        pyi_line = pyi_member.line_number

        # Validate parameter count
        if len(pyx_params) != len(pyi_params):
            self.errors.append(
                f"Class '{class_name}': method '{method_name}' parameter count mismatch "
                f"(.pyx: {len(pyx_params)} {self.pyx_file.name}:{pyx_line}, .pyi: {len(pyi_params)} {self.pyi_file.name}:{pyi_line})"
            )
            return

        # Validate each parameter
        for i, (pyx_param_str, pyi_param_str) in enumerate(zip(pyx_params, pyi_params)):
            pyx_name, pyx_type = self._normalize_parameter(pyx_param_str)
            pyi_name, pyi_type = self._normalize_parameter(pyi_param_str)

            if pyi_name in pyi_member.ignored_params:
                continue

            # Validate parameter name
            if pyx_name != pyi_name:
                self.errors.append(
                    f"Class '{class_name}': method '{method_name}' parameter {i+1} name mismatch "
                    f"(.pyx: '{pyx_name}' {self.pyx_file.name}:{pyx_line}, .pyi: '{pyi_name}' {self.pyi_file.name}:{pyi_line})"
                )

            # Validate parameter type
            pyx_type_normalized = self._normalize_cython_type(pyx_type) if pyx_type else ""
            pyi_type_normalized = self._normalize_cython_type(pyi_type) if pyi_type else ""

            if pyx_type and not pyi_type:
                self.errors.append(
                    f"Class '{class_name}': method '{method_name}' parameter '{pyx_name}' type hint missing in PYI "
                    f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
                )
            elif pyx_type and pyi_type and \
                 pyx_type_normalized != pyi_type_normalized and \
                 not self._is_pyi_type_more_specific(pyx_type_normalized, pyi_type_normalized):
                self.errors.append(
                    f"Class '{class_name}': method '{method_name}' parameter '{pyx_name}' type mismatch "
                    f"(.pyx: '{pyx_type}' {self.pyx_file.name}:{pyx_line}, .pyi: '{pyi_type}' {self.pyi_file.name}:{pyi_line})"
                )

    def _validate_function_parameters(self, function_name: str, pyx_function: CythonFunctionInfo, pyi_function: PyiFunction):
        """Validate function parameters"""
        pyx_params = pyx_function.args or []
        pyi_params = pyi_function.parameters or []
        pyx_line = pyx_function.line_number
        pyi_line = pyi_function.line_number

        # Validate parameter count
        if len(pyx_params) != len(pyi_params):
            self.errors.append(
                f"Function '{function_name}' parameter count mismatch "
                f"(.pyx: {len(pyx_params)} {self.pyx_file.name}:{pyx_line}, .pyi: {len(pyi_params)} {self.pyi_file.name}:{pyi_line})"
            )
            return

        # Validate each parameter
        for i, (pyx_param_str, pyi_param_str) in enumerate(zip(pyx_params, pyi_params)):
            pyx_name, pyx_type = self._normalize_parameter(pyx_param_str)
            pyi_name, pyi_type = self._normalize_parameter(pyi_param_str)

            if pyi_name in pyi_function.ignored_params:
                continue

            # Validate parameter name
            if pyx_name != pyi_name:
                self.errors.append(
                    f"Function '{function_name}' parameter {i+1} name mismatch "
                    f"(.pyx: '{pyx_name}' {self.pyx_file.name}:{pyx_line}, .pyi: '{pyi_name}' {self.pyi_file.name}:{pyi_line})"
                )

            # Validate parameter type
            pyx_type_normalized = self._normalize_cython_type(pyx_type) if pyx_type else ""
            pyi_type_normalized = self._normalize_cython_type(pyi_type) if pyi_type else ""

            if pyx_type and not pyi_type:
                self.errors.append(
                    f"Function '{function_name}' parameter '{pyx_name}' type hint missing in PYI "
                    f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
                )
            elif pyx_type and pyi_type and \
                 pyx_type_normalized != pyi_type_normalized and \
                 not self._is_pyi_type_more_specific(pyx_type_normalized, pyi_type_normalized):
                self.errors.append(
                    f"Function '{function_name}' parameter '{pyx_name}' type mismatch "
                    f"(.pyx: '{pyx_type}' {self.pyx_file.name}:{pyx_line}, .pyi: '{pyi_type}' {self.pyi_file.name}:{pyi_line})"
                )

    def print_results(self):
        """Print validation results"""
        if not self.errors and not self.warnings:
            print("✅ All validations passed!")
            return

        if self.errors:
            print(f"\n❌ ERRORS ({len(self.errors)}):")
            for error in self.errors:
                print(f"  • {error}")

        if self.warnings and not self.pass_warning:
            print(f"\n⚠️  WARNINGS ({len(self.warnings)}):")
            for warning in self.warnings:
                print(f"  • {warning}")

        if self.pass_warning:
            print(f"\n* {len(self.errors)} errors, {len(self.warnings)} warnings (warnings suppressed)")
        else:
            print(f"\n* {len(self.errors)} errors, {len(self.warnings)} warnings")

    def results(self) -> str:
        """Return validation results as string"""
        output = []

        if not self.errors and not self.warnings:
            output.append("✅ All validations passed!")
            return "\n".join(output)

        if self.errors:
            output.append(f"\n❌ ERRORS ({len(self.errors)}):")
            for error in self.errors:
                output.append(f"  • {error}")

        if self.warnings and not self.pass_warning:
            output.append(f"\n⚠️  WARNINGS ({len(self.warnings)}):")
            for warning in self.warnings:
                output.append(f"  • {warning}")

        if self.pass_warning:
            output.append(f"\n* {len(self.errors)} errors, {len(self.warnings)} warnings (warnings suppressed)")
        else:
            output.append(f"\n* {len(self.errors)} errors, {len(self.warnings)} warnings")

        return "\n".join(output)


def main():
    parser = argparse.ArgumentParser(description="Validate Cython PYX files against PYI stub files.")
    parser.add_argument("pyx_file", type=Path, help="Path to the .pyx file")
    parser.add_argument("pyi_file", type=Path, help="Path to the .pyi stub file")
    parser.add_argument(
        "-p", "--include-private",
        action="store_true",
        help="Include private members (starting with '_') in validation"
    )
    parser.add_argument(
        "-w", "--pass-warning",
        action="store_true",
        help="Do not print warnings and exit with success even if warnings exist"
    )
    args = parser.parse_args()

    validator = PyxPyiValidator(args.pyx_file, args.pyi_file, args.include_private, args.pass_warning)
    success = validator.validate()
    validator.print_results()

    sys.exit(0 if success and (args.pass_warning or not validator.warnings) else 1)


if __name__ == "__main__":
    main()
