#!/usr/bin/env python3

import argparse
import re
import sys
from pathlib import Path

from cython_parser import ClassInfo as CythonClassInfo
from cython_parser import FunctionInfo as CythonFunctionInfo
from cython_parser import GlobalVariable as CythonGlobalVariable
from cython_parser import MemberVariable as CythonMemberVariable
from cython_parser import MethodInfo as CythonMethodInfo
from cython_parser import analyze_cython_code
from stub_parser import PyiClassInfo
from stub_parser import PyiFunction
from stub_parser import PyiGlobalVariable
from stub_parser import PyiMember
from stub_parser import PyiParser


class TypeNormalizer:
    CYTHON_TO_PYTHON_TYPE_MAP = {
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

    COLLECTIONS = ["list", "tuple", "set", "dict"]

    @classmethod
    def normalize_cython_type(cls, cython_type: str) -> str:
        if not cython_type:
            return cython_type

        # Clean the type string (remove whitespace, make lowercase)
        cleaned_type = cython_type.strip().lower()

        # Apply type mappings
        for cython_type_name, python_type in cls.CYTHON_TO_PYTHON_TYPE_MAP.items():
            if cython_type_name in cleaned_type:
                cleaned_type = cleaned_type.replace(cython_type_name, python_type)

        return cleaned_type

    def _parse_union_types(cls, type_str: str) -> set[str]:
        type_str = type_str.strip()

        if type_str.startswith("union[") and type_str.endswith("]"):
            # Handle Union[type1, type2]
            content = type_str[len("union["):-1]
            return {t.strip().lower() for t in content.split(",")}
        elif "|" in type_str:
            # Handle type1 | type2
            return {t.strip().lower() for t in type_str.split("|")}

        return {type_str.lower()}

    def is_pyi_type_more_specific(self, pyx_type: str, pyi_type: str) -> bool:
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

    @staticmethod
    def normalize_parameter(param: str) -> tuple[str, str]:
        param = param.strip()

        # Remove default value (after =)
        if "=" in param:
            param = param.split("=")[0].strip()

        # Python style: name: type
        if ":" in param:
            name, type_hint = param.split(":", 1)
            return name.strip(), type_hint.strip()

        # For cases where Cython normalized form is already Python style
        tokens = param.split()
        if len(tokens) == 1:
            # Only name
            return tokens[0], ""
        else:
            # Unexpected form, treat entire string as name
            return param, ""


class ValidationReporter:
    def __init__(self, pyx_file: Path, pyi_file: Path):
        self.pyx_file = pyx_file
        self.pyi_file = pyi_file
        self.errors: list[str] = []
        self.warnings: list[str] = []

    def add_error(self, message: str, pyx_line: int | None = None, pyi_line: int | None = None):
        line_info = self._format_line_info(pyx_line, pyi_line)
        self.errors.append(f"{message} {line_info}")

    def add_warning(self, message: str, pyx_line: int | None = None, pyi_line: int | None = None):
        line_info = self._format_line_info(pyx_line, pyi_line)
        self.warnings.append(f"{message} {line_info}")

    def _format_line_info(self, pyx_line: int | None, pyi_line: int | None) -> str:
        parts = []
        if pyx_line is not None:
            parts.append(f"({self.pyx_file.name}:{pyx_line})")
        if pyi_line is not None:
            parts.append(f"({self.pyi_file.name}:{pyi_line})")
        return " ".join(parts) if parts else ""

    def has_errors(self) -> bool:
        return len(self.errors) > 0

    def has_warnings(self) -> bool:
        return len(self.warnings) > 0

    def print_results(self, pass_warning: bool = False):
        if not self.errors and not self.warnings:
            print("✅ All validations passed!")
            return

        if self.errors:
            print(f"\n❌ ERRORS ({len(self.errors)}):")
            for error in self.errors:
                print(f"  • {error}")

        if self.warnings and not pass_warning:
            print(f"\n⚠️  WARNINGS ({len(self.warnings)}):")
            for warning in self.warnings:
                print(f"  • {warning}")

        if pass_warning:
            print(f"\n* {len(self.errors)} errors, {len(self.warnings)} warnings (warnings suppressed)")
        else:
            print(f"\n* {len(self.errors)} errors, {len(self.warnings)} warnings")

    def results(self, pass_warning: bool = False) -> str:
        output = []
        if not self.errors and not self.warnings:
            output.append("✅ All validations passed!")
            return "\n".join(output)

        if self.errors:
            output.append(f"\n❌ ERRORS ({len(self.errors)}):")
            for error in self.errors:
                output.append(f"  • {error}")

        if self.warnings and not pass_warning:
            output.append(f"\n⚠️  WARNINGS ({len(self.warnings)}):")
            for warning in self.warnings:
                output.append(f"  • {warning}")

        if pass_warning:
            output.append(f"\n* {len(self.errors)} errors, {len(self.warnings)} warnings (warnings suppressed)")
        else:
            output.append(f"\n* {len(self.errors)} errors, {len(self.warnings)} warnings")

        return "\n".join(output)


class PyxPyiValidator:
    def __init__(self, pyx_file: Path, pyi_file: Path, pass_warning: bool = False):
        self.pyx_file = pyx_file
        self.pyi_file = pyi_file
        self.pass_warning = pass_warning
        self.pyx_classes: dict[str, CythonClassInfo] = {}
        self.pyx_functions: dict[str, CythonFunctionInfo] = {}
        self.pyx_global_variables: dict[str, CythonGlobalVariable] = {}
        self.pyi_classes: dict[str, PyiClassInfo] = {}
        self.pyi_functions: dict[str, PyiFunction] = {}
        self.pyi_global_variables: dict[str, PyiGlobalVariable] = {}
        self.reporter = ValidationReporter(pyx_file, pyi_file)
        self.type_normalizer = TypeNormalizer()

    def validate(self) -> bool:
        print(f"Validating {self.pyx_file} -> {self.pyi_file}")

        if not self.pyx_file.exists():
            self.reporter.add_error(f"PYX file not found: {self.pyx_file}")
            return False
        if not self.pyi_file.exists():
            self.reporter.add_error(f"PYI file not found: {self.pyi_file}")
            return False

        if not self._parse_files():
            return False

        self._validate_classes()
        self._validate_functions()
        self._validate_global_variables()

        return not self.reporter.has_errors() and (self.pass_warning or not self.reporter.has_warnings())

    def _parse_files(self) -> bool:
        try:
            pyx_analyzer = analyze_cython_code(
                name=str(self.pyx_file),
                code_content=self.pyx_file.read_text(encoding="utf-8")
            )
            self.pyx_classes = {cls.name: cls for cls in pyx_analyzer.classes}
            self.pyx_functions = {func.name: func for func in pyx_analyzer.functions}
            self.pyx_global_variables = {var.name: var for var in pyx_analyzer.global_variables}
        except Exception as e:
            self.reporter.add_error(f"Error parsing PYX file: {e}")
            return False

        try:
            pyi_parser = PyiParser(self.pyi_file)
            self.pyi_classes, self.pyi_functions, self.pyi_global_variables = pyi_parser.parse()
        except Exception as e:
            self.reporter.add_error(f"Error parsing PYI file: {e}")
            return False

        return True

    def _validate_classes(self):
        pyx_class_names = set(self.pyx_classes.keys())
        pyi_class_names = set(self.pyi_classes.keys())

        # Missing classes
        missing_classes = pyx_class_names - pyi_class_names
        for class_name in missing_classes:
            pyx_line = self.pyx_classes[class_name].line_number
            self.reporter.add_error(f"Class '{class_name}' missing in PYI file", pyx_line)

        # Extra classes
        extra_classes = pyi_class_names - pyx_class_names
        for class_name in extra_classes:
            pyi_class = self.pyi_classes[class_name]
            if not pyi_class.ignore_validation:
                self.reporter.add_error(f"Class '{class_name}' in PYI but not in PYX", pyi_line=pyi_class.line_number)

        # Validate common classes
        common_classes = pyx_class_names & pyi_class_names
        for class_name in common_classes:
            self._validate_class(
                self.pyx_classes[class_name],
                self.pyi_classes[class_name]
            )

    def _validate_functions(self):
        pyx_function_names = set(self.pyx_functions.keys())
        pyi_function_names = set(self.pyi_functions.keys())

        # Missing functions
        missing_functions = pyx_function_names - pyi_function_names
        for function_name in missing_functions:
            function = self.pyx_functions[function_name]
            pyx_line = function.line_number
            if not function.is_cdef:
                self.reporter.add_error(f"Function '{function_name}' missing in PYI", pyx_line)

        # Extra functions
        extra_functions = pyi_function_names - pyx_function_names
        for function_name in extra_functions:
            pyi_function = self.pyi_functions[function_name]
            if not pyi_function.ignore_validation:
                self.reporter.add_error(f"Function '{function_name}' in PYI but not in PYX", pyi_line=pyi_function.line_number)

        # Validate common functions
        common_functions = pyx_function_names & pyi_function_names
        for function_name in common_functions:
            if self.pyi_functions[function_name].ignore_validation:
                continue

            pyx_function = self.pyx_functions[function_name]
            pyi_function = self.pyi_functions[function_name]

            if pyx_function.is_cdef:
                # Skip comparison for cdef functions
                continue

            self._validate_function(function_name, pyx_function, pyi_function)

    def _validate_global_variables(self):
        pyx_variable_names = set(self.pyx_global_variables.keys())
        pyi_variable_names = set(self.pyi_global_variables.keys())

        # Missing variables
        missing_variables = pyx_variable_names - pyi_variable_names
        for variable_name in missing_variables:
            variable = self.pyx_global_variables[variable_name]
            pyx_line = variable.line_number
            self.reporter.add_error(f"Global variable '{variable_name}' missing in PYI", pyx_line)

        # Extra variables
        extra_variables = pyi_variable_names - pyx_variable_names
        for variable_name in extra_variables:
            pyi_variable = self.pyi_global_variables[variable_name]
            if not pyi_variable.ignore_validation:
                self.reporter.add_error(f"Global variable '{variable_name}' in PYI but not in PYX", pyi_line=pyi_variable.line_number)

        # Validate common variables
        common_variables = pyx_variable_names & pyi_variable_names
        for variable_name in common_variables:
            if self.pyi_global_variables[variable_name].ignore_validation:
                continue

            pyx_variable = self.pyx_global_variables[variable_name]
            pyi_variable = self.pyi_global_variables[variable_name]

            self._validate_global_variable(variable_name, pyx_variable, pyi_variable)

    def _validate_class(self, pyx_class: CythonClassInfo, pyi_class: PyiClassInfo):
        class_name = pyx_class.name

        # Validate base classes
        if set(pyx_class.base_classes) != set(pyi_class.base_classes):
            pyx_line = pyx_class.line_number
            pyi_line = pyi_class.line_number
            self.reporter.add_error(
                f"Class '{class_name}': base classes mismatch. "
                f".pyx: {pyx_class.base_classes}, .pyi: {pyi_class.base_classes}",
                pyx_line, pyi_line
            )

        # Validate members
        self._validate_members(class_name, pyx_class.methods, pyx_class.member_variables, pyi_class.members)

    def _validate_function(self, function_name: str, pyx_function: CythonFunctionInfo, pyi_function: PyiFunction):
        pyx_line = pyx_function.line_number
        pyi_line = pyi_function.line_number

        # Validate parameters
        self._validate_function_parameters(function_name, pyx_function, pyi_function)

        # Validate return type
        pyx_return = pyx_function.return_type.strip() if pyx_function.return_type else ""
        pyi_return = pyi_function.return_type.strip() if pyi_function.return_type else ""

        pyx_return_normalized = self.type_normalizer.normalize_cython_type(pyx_return)
        pyi_return_normalized = self.type_normalizer.normalize_cython_type(pyi_return)

        if pyx_return and not pyi_return:
            self.reporter.add_error(
                f"Function '{function_name}' return type missing in PYI",
                pyx_line, pyi_line
            )
        elif (pyx_return and pyi_return and
              pyx_return_normalized != pyi_return_normalized and
              not self.type_normalizer.is_pyi_type_more_specific(pyx_return_normalized, pyi_return_normalized)):
            self.reporter.add_error(
                f"Function '{function_name}' return type mismatch "
                f"(.pyx: '{pyx_return}', .pyi: '{pyi_return}')",
                pyx_line, pyi_line
            )

    def _validate_global_variable(self, variable_name: str, pyx_variable: CythonGlobalVariable, pyi_variable: PyiGlobalVariable):
        pyx_line = pyx_variable.line_number
        pyi_line = pyi_variable.line_number

        pyx_type_normalized = self.type_normalizer.normalize_cython_type(pyx_variable.type_hint) if pyx_variable.type_hint else ""
        pyi_type_normalized = self.type_normalizer.normalize_cython_type(pyi_variable.type_hint) if pyi_variable.type_hint else ""

        if (pyx_type_normalized != pyi_type_normalized and
            not self.type_normalizer.is_pyi_type_more_specific(pyx_type_normalized, pyi_type_normalized)):
            self.reporter.add_error(
                f"Global variable '{variable_name}' type mismatch "
                f"(.pyx: {pyx_variable.type_hint}, .pyi: {pyi_variable.type_hint})",
                pyx_line, pyi_line
            )

    def _validate_members(self, class_name: str, pyx_methods: list[CythonMethodInfo], # noqa: C901
                         pyx_member_variables: list[CythonMemberVariable], pyi_members: dict[str, PyiMember]):
        pyx_combined_members = {}

        for method in pyx_methods:
            pyx_combined_members[method.name.replace("self.", "")] = method

        for var in pyx_member_variables:
            pyx_combined_members[var.name.replace("self.", "")] = var

        pyx_member_names = set(pyx_combined_members.keys())
        pyi_member_names = set(pyi_members.keys())

        # Missing members
        missing_members = pyx_member_names - pyi_member_names
        for member_name in missing_members:
            member = pyx_combined_members[member_name]
            pyx_line = member.line_number
            if isinstance(member, CythonMethodInfo) and not member.is_cdef:
                self.reporter.add_error(
                    f"Class '{class_name}': member '{member_name}' missing in PYI",
                    pyx_line
                )

        # Extra members
        extra_members = pyi_member_names - pyx_member_names
        for member_name in extra_members:
            pyi_member = pyi_members[member_name]
            if not pyi_member.ignore_validation:
                self.reporter.add_error(
                    f"Class '{class_name}': member '{member_name}' in PYI but not in PYX",
                    pyi_line=pyi_member.line_number
                )

        # Validate common members
        common_members = pyx_member_names & pyi_member_names
        for member_name in common_members:
            if pyi_members[member_name].ignore_validation:
                continue

            pyx_member = pyx_combined_members[member_name]
            pyi_member = pyi_members[member_name]

            if isinstance(pyx_member, CythonMethodInfo):
                if pyx_member.is_cdef:
                    # Skip comparison for cdef functions
                    continue

                if pyi_member.is_method:
                    self._validate_method(class_name, member_name, pyx_member, pyi_member)
                else:
                    self.reporter.add_error(
                        f"Class '{class_name}': member '{member_name}' type mismatch (method/variable) "
                        f"(.pyx: {type(pyx_member).__name__}, .pyi: {type(pyi_member).__name__})",
                        pyx_member.line_number, pyi_member.line_number
                    )
            elif isinstance(pyx_member, CythonMemberVariable) and not pyi_member.is_method:
                self._validate_member_variable(class_name, member_name, pyx_member, pyi_member)
            else:
                self.reporter.add_error(
                    f"Class '{class_name}': member '{member_name}' type mismatch (method/variable) "
                    f"(.pyx: {type(pyx_member).__name__}, .pyi: {type(pyi_member).__name__})",
                    getattr(pyx_member, "line_number", None), pyi_member.line_number
                )

    def _validate_method(self, class_name: str, method_name: str, pyx_method: CythonMethodInfo, pyi_member: PyiMember):
        pyx_line = pyx_method.line_number
        pyi_line = pyi_member.line_number

        # Validate decorators
        if pyx_method.is_property != pyi_member.is_property:
            self.reporter.add_error(
                f"Class '{class_name}': method '{method_name}' @property decorator mismatch "
                f"(.pyx: {pyx_method.is_property}, .pyi: {pyi_member.is_property})",
                pyx_line, pyi_line
            )

        if pyx_method.is_static != pyi_member.is_staticmethod:
            self.reporter.add_error(
                f"Class '{class_name}': method '{method_name}' @staticmethod decorator mismatch "
                f"(.pyx: {pyx_method.is_static}, .pyi: {pyi_member.is_staticmethod})",
                pyx_line, pyi_line
            )

        if pyx_method.is_classmethod != pyi_member.is_classmethod:
            self.reporter.add_error(
                f"Class '{class_name}': method '{method_name}' @classmethod decorator mismatch "
                f"(.pyx: {pyx_method.is_classmethod}, .pyi: {pyi_member.is_classmethod})",
                pyx_line, pyi_line
            )

        # Validate parameters
        self._validate_method_parameters(class_name, method_name, pyx_method, pyi_member)

        # Validate return type
        pyx_return = pyx_method.return_type.strip() if pyx_method.return_type else ""
        pyi_return = pyi_member.return_type.strip() if pyi_member.return_type else ""

        pyx_return_normalized = self.type_normalizer.normalize_cython_type(pyx_return)
        pyi_return_normalized = self.type_normalizer.normalize_cython_type(pyi_return)

        if pyx_return and not pyi_return:
            self.reporter.add_error(
                f"Class '{class_name}': method '{method_name}' return type missing in PYI",
                pyx_line, pyi_line
            )
        elif (pyx_return and pyi_return and
              pyx_return_normalized != pyi_return_normalized and
              not self.type_normalizer.is_pyi_type_more_specific(pyx_return_normalized, pyi_return_normalized)):
            self.reporter.add_error(
                f"Class '{class_name}': method '{method_name}' return type mismatch "
                f"(.pyx: '{pyx_return}', .pyi: '{pyi_return}')",
                pyx_line, pyi_line
            )

    def _validate_member_variable(self, class_name: str, member_name: str,
                                 pyx_member: CythonMemberVariable, pyi_member: PyiMember):
        pyx_line = pyx_member.line_number
        pyi_line = pyi_member.line_number

        pyx_type_normalized = self.type_normalizer.normalize_cython_type(pyx_member.type_hint) if pyx_member.type_hint else ""
        pyi_type_normalized = self.type_normalizer.normalize_cython_type(pyi_member.type_hint) if pyi_member.type_hint else ""

        if (pyx_type_normalized != pyi_type_normalized and
            not self.type_normalizer.is_pyi_type_more_specific(pyx_type_normalized, pyi_type_normalized)):
            self.reporter.add_error(
                f"Class '{class_name}': member '{member_name}' type mismatch "
                f"(.pyx: {pyx_member.type_hint}, .pyi: {pyi_member.type_hint})",
                pyx_line, pyi_line
            )

    def _validate_method_parameters(self, class_name: str, method_name: str,
                                   pyx_method: CythonMethodInfo, pyi_member: PyiMember):
        pyx_params = pyx_method.args or []
        pyi_params = pyi_member.parameters or []
        pyx_line = pyx_method.line_number
        pyi_line = pyi_member.line_number

        # Validate parameter count
        if len(pyx_params) != len(pyi_params):
            self.reporter.add_error(
                f"Class '{class_name}': method '{method_name}' parameter count mismatch "
                f"(.pyx: {len(pyx_params)}, .pyi: {len(pyi_params)})",
                pyx_line, pyi_line
            )
            return

        # Validate each parameter
        for i, (pyx_param_str, pyi_param_str) in enumerate(zip(pyx_params, pyi_params)):
            pyx_name, pyx_type = self.type_normalizer.normalize_parameter(pyx_param_str)
            pyi_name, pyi_type = self.type_normalizer.normalize_parameter(pyi_param_str)

            if pyi_name in pyi_member.ignored_params:
                continue

            # Validate parameter name
            if pyx_name != pyi_name:
                self.reporter.add_error(
                    f"Class '{class_name}': method '{method_name}' parameter {i+1} name mismatch "
                    f"(.pyx: '{pyx_name}', .pyi: '{pyi_name}')",
                    pyx_line, pyi_line
                )

            # Validate parameter type
            pyx_type_normalized = self.type_normalizer.normalize_cython_type(pyx_type) if pyx_type else ""
            pyi_type_normalized = self.type_normalizer.normalize_cython_type(pyi_type) if pyi_type else ""

            if pyx_type and not pyi_type:
                self.reporter.add_error(
                    f"Class '{class_name}': method '{method_name}' parameter '{pyx_name}' type hint missing in PYI",
                    pyx_line, pyi_line
                )
            elif (pyx_type and pyi_type and
                  pyx_type_normalized != pyi_type_normalized and
                  not self.type_normalizer.is_pyi_type_more_specific(pyx_type_normalized, pyi_type_normalized)):
                self.reporter.add_error(
                    f"Class '{class_name}': method '{method_name}' parameter '{pyx_name}' type mismatch "
                    f"(.pyx: '{pyx_type}', .pyi: '{pyi_type}')",
                    pyx_line, pyi_line
                )

    def _validate_function_parameters(self, function_name: str, pyx_function: CythonFunctionInfo, pyi_function: PyiFunction):
        pyx_params = pyx_function.args or []
        pyi_params = pyi_function.parameters or []
        pyx_line = pyx_function.line_number
        pyi_line = pyi_function.line_number

        # Validate parameter count
        if len(pyx_params) != len(pyi_params):
            self.reporter.add_error(
                f"Function '{function_name}' parameter count mismatch "
                f"(.pyx: {len(pyx_params)}, .pyi: {len(pyi_params)})",
                pyx_line, pyi_line
            )
            return

        # Validate each parameter
        for i, (pyx_param_str, pyi_param_str) in enumerate(zip(pyx_params, pyi_params)):
            pyx_name, pyx_type = self.type_normalizer.normalize_parameter(pyx_param_str)
            pyi_name, pyi_type = self.type_normalizer.normalize_parameter(pyi_param_str)

            if pyi_name in pyi_function.ignored_params:
                continue

            # Validate parameter name
            if pyx_name != pyi_name:
                self.reporter.add_error(
                    f"Function '{function_name}' parameter {i+1} name mismatch "
                    f"(.pyx: '{pyx_name}', .pyi: '{pyi_name}')",
                    pyx_line, pyi_line
                )

            # Validate parameter type
            pyx_type_normalized = self.type_normalizer.normalize_cython_type(pyx_type) if pyx_type else ""
            pyi_type_normalized = self.type_normalizer.normalize_cython_type(pyi_type) if pyi_type else ""

            if pyx_type and not pyi_type:
                self.reporter.add_error(
                    f"Function '{function_name}' parameter '{pyx_name}' type hint missing in PYI",
                    pyx_line, pyi_line
                )
            elif (pyx_type and pyi_type and
                  pyx_type_normalized != pyi_type_normalized and
                  not self.type_normalizer.is_pyi_type_more_specific(pyx_type_normalized, pyi_type_normalized)):
                self.reporter.add_error(
                    f"Function '{function_name}' parameter '{pyx_name}' type mismatch "
                    f"(.pyx: '{pyx_type}', .pyi: '{pyi_type}')",
                    pyx_line, pyi_line
                )

    def print_results(self):
        self.reporter.print_results(self.pass_warning)

    def results(self) -> str:
        return self.reporter.results(self.pass_warning)


def main():
    parser = argparse.ArgumentParser(description="Validate Cython PYX files against PYI stub files.")
    parser.add_argument("pyx_file", type=Path, help="Path to the .pyx file")
    parser.add_argument("pyi_file", type=Path, help="Path to the .pyi stub file")
    parser.add_argument(
        "-w", "--pass-warning",
        action="store_true",
        help="Do not print warnings and exit with success even if warnings exist"
    )
    args = parser.parse_args()

    validator = PyxPyiValidator(args.pyx_file, args.pyi_file, args.pass_warning)
    success = validator.validate()
    validator.print_results()

    sys.exit(0 if success and (args.pass_warning or not validator.reporter.has_warnings()) else 1)


if __name__ == "__main__":
    main()
