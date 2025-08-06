#!/usr/bin/env python3

import ast
from dataclasses import dataclass
from dataclasses import field
from pathlib import Path


SKIPPING_COMMENT = "# skip-validate"

@dataclass
class Decorators:
    is_property: bool = False
    is_staticmethod: bool = False
    is_classmethod: bool = False
    is_overload: bool = False


@dataclass
class PyiMember:
    name: str
    type_hint: str | None = None
    is_method: bool = False
    is_property: bool = False
    is_staticmethod: bool = False
    is_classmethod: bool = False
    is_overload: bool = False
    docstring: str | None = None
    is_private: bool = False
    parameters: list[str] = field(default_factory=list)
    return_type: str | None = None
    line_number: int | None = None
    ignore_validation: bool = False
    ignored_params: set[str] = field(default_factory=set)

    def __post_init__(self):
        self.is_private = self.name.startswith("_")


@dataclass
class PyiFunction:
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
    name: str
    docstring: str | None = None
    members: dict[str, PyiMember] = field(default_factory=dict)
    base_classes: list[str] = field(default_factory=list)
    line_number: int | None = None
    ignore_validation: bool = False


class PyiParser:


    def __init__(self, file_path: Path):
        self.file_path = file_path
        self.file_content = self.file_path.read_text(encoding="utf-8")
        self.file_lines = self.file_content.splitlines()
        self.classes: dict[str, PyiClassInfo] = {}
        self.functions: dict[str, PyiFunction] = {}
        self.global_variables: dict[str, PyiGlobalVariable] = {}

    def parse(self) -> tuple[dict[str, PyiClassInfo], dict[str, PyiFunction], dict[str, PyiGlobalVariable]]:
        try:
            tree = ast.parse(self.file_content)
        except SyntaxError as e:
            print(f"Syntax error parsing {self.file_path}: {e}")
            return {}, {}, {}
        except Exception as e:
            print(f"Error parsing {self.file_path}: {e}")
            return {}, {}, {}

        for node in tree.body:
            try:
                if isinstance(node, ast.ClassDef):
                    class_info = self._parse_class(node)
                    self.classes[class_info.name] = class_info
                elif isinstance(node, ast.FunctionDef | ast.AsyncFunctionDef):
                    function_info = self._parse_function(node)
                    self.functions[function_info.name] = function_info
                elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
                    var_info = self._parse_global_variable_annotated(node)
                    self.global_variables[var_info.name] = var_info
                elif isinstance(node, ast.Assign):
                    for var_info in self._parse_global_variable_assign(node):
                        self.global_variables[var_info.name] = var_info
            except Exception as e:
                print(f"Error processing node in {self.file_path}: {e}")
                continue

        return self.classes, self.functions, self.global_variables

    def _is_ignored(self, node: ast.AST) -> bool:
        if not hasattr(node, "lineno"):
            return False
        line_index = node.lineno - 1
        if 0 <= line_index < len(self.file_lines):
            line = self.file_lines[line_index].rstrip()
            return line.endswith(SKIPPING_COMMENT)
        return False

    def _parse_class(self, node: ast.ClassDef) -> PyiClassInfo:
        base_classes = self._parse_base_classes(node.bases)

        class_info = PyiClassInfo(
            name=node.name,
            docstring=ast.get_docstring(node),
            base_classes=base_classes,
            line_number=node.lineno,
            ignore_validation=self._is_ignored(node),
        )

        for item in node.body:
            if isinstance(item, ast.FunctionDef | ast.AsyncFunctionDef):
                member = self._parse_class_method(item)
                class_info.members[item.name] = member
            elif isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
                member = self._parse_class_variable_annotated(item)
                class_info.members[item.target.id] = member
            elif isinstance(item, ast.Assign):
                for target in item.targets:
                    if isinstance(target, ast.Name):
                        member = self._parse_class_variable_assign(item, target.id)
                        class_info.members[target.id] = member

        return class_info

    def _parse_base_classes(self, bases: list[ast.expr]) -> list[str]:
        base_classes = []
        for base in bases:
            if isinstance(base, ast.Name):
                base_classes.append(base.id)
            elif isinstance(base, ast.Attribute):
                base_classes.append(ast.unparse(base))
        return base_classes

    def _parse_class_method(self, item: ast.FunctionDef | ast.AsyncFunctionDef) -> PyiMember:
        parameters, ignored_params = self._parse_parameters(item)
        decorators = self._analyze_decorators(item.decorator_list)

        return PyiMember(
            name=item.name,
            is_method=True,
            is_property=decorators.is_property,
            is_staticmethod=decorators.is_staticmethod,
            is_classmethod=decorators.is_classmethod,
            is_overload=decorators.is_overload,
            parameters=parameters,
            return_type=ast.unparse(item.returns) if item.returns else None,
            docstring=ast.get_docstring(item),
            line_number=item.lineno,
            ignore_validation=self._is_ignored(item),
            ignored_params=ignored_params
        )

    def _parse_class_variable_annotated(self, item: ast.AnnAssign) -> PyiMember:
        return PyiMember(
            name=item.target.id,
            type_hint=ast.unparse(item.annotation),
            line_number=item.lineno,
            ignore_validation=self._is_ignored(item),
        )

    def _parse_class_variable_assign(self, item: ast.Assign, name: str) -> PyiMember:
        return PyiMember(
            name=name,
            line_number=item.lineno,
            ignore_validation=self._is_ignored(item),
        )

    def _parse_function(self, node: ast.FunctionDef | ast.AsyncFunctionDef) -> PyiFunction:
        parameters, ignored_params = self._parse_parameters(node)
        is_overload = self._has_overload_decorator(node.decorator_list)

        return PyiFunction(
            name=node.name,
            parameters=parameters,
            return_type=ast.unparse(node.returns) if node.returns else None,
            docstring=ast.get_docstring(node),
            is_overload=is_overload,
            line_number=node.lineno,
            ignore_validation=self._is_ignored(node),
            ignored_params=ignored_params
        )

    def _parse_parameters(self, node: ast.FunctionDef | ast.AsyncFunctionDef) -> tuple[list[str], set[str]]:
        parameters = []
        ignored_params = set()

        for arg in node.args.args:
            if self._is_ignored(arg):
                ignored_params.add(arg.arg)

            param_str = arg.arg
            if arg.annotation:
                param_str += f": {ast.unparse(arg.annotation)}"
            parameters.append(param_str)

        defaults = node.args.defaults
        if defaults:
            defaults_start = len(parameters) - len(defaults)
            for i, default in enumerate(defaults):
                param_idx = defaults_start + i
                if param_idx < len(parameters):
                    parameters[param_idx] += f" = {ast.unparse(default)}"

        return parameters, ignored_params

    def _analyze_decorators(self, decorator_list: list[ast.expr]) -> "Decorators":
        decorators = Decorators()

        for decorator in decorator_list:
            if isinstance(decorator, ast.Name):
                if decorator.id == "property":
                    decorators.is_property = True
                elif decorator.id == "staticmethod":
                    decorators.is_staticmethod = True
                elif decorator.id == "classmethod":
                    decorators.is_classmethod = True
                elif decorator.id == "overload":
                    decorators.is_overload = True
            elif isinstance(decorator, ast.Attribute):
                decorator_name = ast.unparse(decorator)
                if "overload" in decorator_name:
                    decorators.is_overload = True

        return decorators

    def _has_overload_decorator(self, decorator_list: list[ast.expr]) -> bool:
        for decorator in decorator_list:
            if isinstance(decorator, ast.Name) and decorator.id == "overload":
                return True
            elif isinstance(decorator, ast.Attribute):
                decorator_name = ast.unparse(decorator)
                if "overload" in decorator_name:
                    return True
        return False

    def _parse_global_variable_annotated(self, node: ast.AnnAssign) -> PyiGlobalVariable:
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

    def print_results(self):  # noqa: C901
        if self.classes:
            print("\nClasses:")
            for cls in self.classes.values():
                print(f"\nclass: {cls.name}")
                if cls.ignore_validation:
                    print("  (Validation Ignored)")

                if cls.base_classes:
                    print(f"  Inherits: {', '.join(cls.base_classes)}")

                if cls.docstring:
                    print(f'  """{cls.docstring}"""')

                if cls.members:
                    print("  Members:")
                    for member in cls.members.values():
                        if member.is_method:
                            visibility = "private" if member.is_private else "public"
                            decorators = []
                            if member.is_staticmethod:
                                decorators.append("@staticmethod")
                            if member.is_classmethod:
                                decorators.append("@classmethod")
                            if member.is_property:
                                decorators.append("@property")
                            if member.is_overload:
                                decorators.append("@overload")

                            decorator_str = " ".join(decorators) + " " if decorators else ""
                            args_str = ", ".join(member.parameters) if member.parameters else ""
                            return_str = f" -> {member.return_type}" if member.return_type else ""
                            ignore_str = " (Validation Ignored)" if member.ignore_validation else ""

                            print(f"    - {decorator_str}def {member.name}({args_str}){return_str} ({visibility}){ignore_str}")
                            if member.docstring:
                                print(f'      """{member.docstring}"""')
                        else:  # It's a variable
                            visibility = "private" if member.is_private else "public"
                            type_info = f": {member.type_hint}" if member.type_hint else ""
                            ignore_str = " (Validation Ignored)" if member.ignore_validation else ""
                            print(f"    - {member.name}{type_info} ({visibility}){ignore_str}")

        if self.functions:
            print("\nFunctions:")
            for func in self.functions.values():
                decorators = []
                if func.is_overload:
                    decorators.append("@overload")
                decorator_str = " ".join(decorators) + " " if decorators else ""
                args_str = ", ".join(func.parameters) if func.parameters else ""
                return_str = f" -> {func.return_type}" if func.return_type else ""
                ignore_str = " (Validation Ignored)" if func.ignore_validation else ""
                print(f"  - {decorator_str}def {func.name}({args_str}){return_str}{ignore_str}")
                if func.docstring:
                    print(f'    """{func.docstring}"""')

        if self.global_variables:
            print("\nGlobal Variables:")
            for var in self.global_variables.values():
                type_info = f": {var.type_hint}" if var.type_hint else ""
                value_info = f" = {var.value}" if var.value else ""
                classification = "Constant" if var.name.isupper() else "Variable"
                ignore_str = " (Validation Ignored)" if var.ignore_validation else ""
                print(f"  - {var.name}{type_info}{value_info} ({classification}){ignore_str}")
