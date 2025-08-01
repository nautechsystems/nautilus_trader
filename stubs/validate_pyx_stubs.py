#!/usr/bin/env python3
"""
PYX to PYI Validation Script

이 스크립트는 Cython .pyx 파일의 클래스, 메서드, 멤버변수, docstring, 타입 어노테이션이
해당하는 .pyi 스텁 파일에 올바르게 추출되었는지 검증합니다.
"""

import argparse
import ast
import sys
from dataclasses import dataclass
from dataclasses import field
from pathlib import Path

from cython_parser import ClassInfo as CythonClassInfo
from cython_parser import MemberVariable as CythonMemberVariable
from cython_parser import MethodInfo as CythonMethodInfo
from cython_parser import analyze_cython_code


@dataclass
class PyiMember:
    """클래스 멤버 정보"""

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
class PyiClassInfo:
    """클래스 정보"""

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
    """Python .pyi 스텁 파일 파서"""

    def __init__(self, file_path: Path):
        self.file_path = file_path
        self.file_content = self.file_path.read_text(encoding="utf-8")
        self.file_lines = self.file_content.splitlines()
        self.classes: dict[str, PyiClassInfo] = {}

    def parse(self) -> dict[str, PyiClassInfo]:
        """Pyi 파일을 파싱하여 클래스 정보 추출"""
        try:
            tree = ast.parse(self.file_content)
        except Exception as e:
            print(f"Error parsing {self.file_path}: {e}")
            return {}

        for node in ast.walk(tree):
            if isinstance(node, ast.ClassDef):
                class_info = self._parse_class(node)
                self.classes[class_info.name] = class_info

        return self.classes

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
        """클래스 노드 파싱"""
        # 베이스 클래스
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

        # 멤버들 파싱
        for item in node.body:
            if isinstance(item, ast.FunctionDef):
                # 파라미터 정보를 더 상세하게 추출
                parameters = []
                ignored_params = set()
                for arg in item.args.args:
                    if self._is_ignored(arg):
                        ignored_params.add(arg.arg)
                    param_str = arg.arg
                    if arg.annotation:
                        param_str += f": {ast.unparse(arg.annotation)}"
                    parameters.append(param_str)

                # 기본값이 있는 파라미터 처리
                defaults = item.args.defaults
                if defaults:
                    # 기본값이 있는 파라미터들의 시작 인덱스
                    defaults_start = len(parameters) - len(defaults)
                    for i, default in enumerate(defaults):
                        param_idx = defaults_start + i
                        if param_idx < len(parameters):
                            parameters[param_idx] += f" = {ast.unparse(default)}"

                # 데코레이터 분석
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
                        # typing.overload 등의 경우
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
                # 타입 어노테이션이 있는 변수
                member = PyiMember(
                    name=item.target.id,
                    type_hint=ast.unparse(item.annotation),
                    line_number=item.lineno,
                    ignore_validation=self._is_ignored(item),
                )
                class_info.members[item.target.id] = member

            elif isinstance(item, ast.Assign):
                # 일반 변수 할당
                for target in item.targets:
                    if isinstance(target, ast.Name):
                        member = PyiMember(
                            name=target.id,
                            line_number=item.lineno,
                            ignore_validation=self._is_ignored(item),
                        )
                        class_info.members[target.id] = member

        return class_info


class PyxPyiValidator:
    """PYX와 PYI 파일 검증기"""

    def __init__(self, pyx_file: Path, pyi_file: Path, include_private: bool = False, pass_warning: bool = False):
        self.pyx_file = pyx_file
        self.pyi_file = pyi_file
        self.include_private = include_private
        self.pass_warning = pass_warning
        self.pyx_classes: dict[str, CythonClassInfo] = {}
        self.pyi_classes: dict[str, PyiClassInfo] = {}
        self.errors = []
        self.warnings = []
        self.COLLECTIONS = ["list", "tuple", "set", "dict"]

    def validate(self) -> bool:
        """검증 수행"""
        print(f"Validating {self.pyx_file} -> {self.pyi_file}")

        # 파일 존재 확인
        if not self.pyx_file.exists():
            self.errors.append(f"PYX file not found: {self.pyx_file}")
            return False

        if not self.pyi_file.exists():
            self.errors.append(f"PYI file not found: {self.pyi_file}")
            return False

        # 파싱
        try:
            pyx_analyzer = analyze_cython_code(name=str(self.pyx_file), code_content=self.pyx_file.read_text(encoding="utf-8"))
            self.pyx_classes = {cls.name: cls for cls in pyx_analyzer.classes}
        except Exception as e:
            self.errors.append(f"Error parsing PYX file: {e}")
            return False

        try:
            pyi_parser = PyiParser(self.pyi_file)
            self.pyi_classes = pyi_parser.parse()
        except Exception as e:
            self.errors.append(f"Error parsing PYI file: {e}")
            return False

        # 검증 수행
        self._validate_classes()

        return len(self.errors) == 0

    def _validate_classes(self):
        """클래스들 검증"""
        pyx_class_names = set(self.pyx_classes.keys())
        pyi_class_names = set(self.pyi_classes.keys())

        # 누락된 클래스
        missing_classes = pyx_class_names - pyi_class_names
        for class_name in missing_classes:
            pyx_line = self.pyx_classes[class_name].line_number
            self.errors.append(f"Class '{class_name}' missing in PYI file ({self.pyx_file.name}:{pyx_line})")

        # 추가된 클래스 (경고)
        extra_classes = pyi_class_names - pyx_class_names
        for class_name in extra_classes:
            pyi_line = self.pyi_classes[class_name].line_number
            self.warnings.append(f"Class '{class_name}' in PYI but not in PYX ({self.pyi_file.name}:{pyi_line})")

        # 공통 클래스들 검증
        common_classes = pyx_class_names & pyi_class_names
        for class_name in common_classes:
            self._validate_class(
                self.pyx_classes[class_name],
                self.pyi_classes[class_name]
            )

    def _validate_class(self, pyx_class: CythonClassInfo, pyi_class: PyiClassInfo):
        """개별 클래스 검증"""
        class_name = pyx_class.name

        # 베이스 클래스 검증
        if set(pyx_class.base_classes) != set(pyi_class.base_classes):
            pyx_line = pyx_class.line_number
            pyi_line = pyi_class.line_number
            self.errors.append(
                f"Class '{class_name}': base classes mismatch. "
                f".pyx: {pyx_class.base_classes} ({self.pyx_file.name}:{pyx_line}), "
                f".pyi: {pyi_class.base_classes} ({self.pyi_file.name}:{pyi_line})"
            )

        # docstring 검증
        if pyx_class.docstring and not pyi_class.docstring:
            pyx_line = pyx_class.line_number
            pyi_line = pyi_class.line_number
            self.warnings.append(f"Class '{class_name}': docstring missing in PYI ({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})")
        elif pyx_class.docstring and pyi_class.docstring:
            # 공백을 제거하고 비교
            if "".join(pyx_class.docstring.split()) != "".join(pyi_class.docstring.split()):
                pyx_line = pyx_class.line_number
                pyi_line = pyi_class.line_number
                self.warnings.append(f"Class '{class_name}': docstring differs ({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})")

        # 멤버들 검증
        self._validate_members(class_name, pyx_class.methods, pyx_class.member_variables, pyi_class.members)

    def _validate_members(self, class_name: str, pyx_methods: list[CythonMethodInfo], pyx_member_variables: list[CythonMemberVariable], pyi_members: dict[str, PyiMember]):
        """클래스 멤버들 검증"""
        pyx_combined_members = {}
        for method in pyx_methods:
            pyx_combined_members[method.name] = method
        for var in pyx_member_variables:
            pyx_combined_members[var.name] = var

        # private 멤버 포함 여부에 따라 검증 대상 결정
        if self.include_private:
            pyx_members_to_validate = pyx_combined_members
        else:
            pyx_members_to_validate = {name: member for name, member in pyx_combined_members.items()
                                       if not member.is_private}

        pyx_member_names = set(pyx_members_to_validate.keys())
        pyi_member_names = set(pyi_members.keys())

        # 누락된 멤버들
        missing_members = pyx_member_names - pyi_member_names
        for member_name in missing_members:
            member = pyx_members_to_validate[member_name]
            pyx_line = member.line_number
            if isinstance(member, CythonMethodInfo) and member.is_cdef:
                self.warnings.append(f"Class '{class_name}': cdef method '{member_name}' not expected in PYI ({self.pyx_file.name}:{pyx_line})")
            else:
                self.errors.append(f"Class '{class_name}': member '{member_name}' missing in PYI ({self.pyx_file.name}:{pyx_line})")

        # 추가된 멤버들 (경고)
        extra_members = pyi_member_names - pyx_member_names
        for member_name in extra_members:
            pyi_member = pyi_members[member_name]
            if not pyi_member.ignore_validation:
                self.warnings.append(f"Class '{class_name}': member '{member_name}' in PYI but not in PYX ({self.pyi_file.name}:{pyi_member.line_number})")

        # 공통 멤버들 검증
        common_members = pyx_member_names & pyi_member_names
        for member_name in common_members:
            if pyi_members[member_name].ignore_validation:
                continue
            pyx_member = pyx_members_to_validate[member_name]
            pyi_member = pyi_members[member_name]

            if isinstance(pyx_member, CythonMethodInfo):
                if pyx_member.is_cdef:
                    # cdef 함수는 비교에서 제외
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
        """파라미터를 정규화하여 이름과 타입을 분리 (Cython과 Python 모두 지원)"""
        param = param.strip()

        # 기본값 제거 (= 이후)
        if "=" in param:
            param = param.split("=")[0].strip()

        # Python 스타일: name: type
        if ":" in param:
            name, type_hint = param.split(":", 1)
            return name.strip(), type_hint.strip()

        # Cython에서 정규화된 형태가 이미 Python 스타일인 경우 처리
        # 이미 _normalize_cython_parameter에서 처리되었을 것임
        tokens = param.split()
        if len(tokens) == 1:
            # 이름만 있는 경우
            return tokens[0], ""
        else:
            # 예상치 못한 형태는 전체를 이름으로 처리
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

        return cleaned_type

    def _validate_method(self, class_name: str, method_name: str, pyx_method: CythonMethodInfo, pyi_member: PyiMember):
        """개별 메서드 검증 (데코레이터 검증 추가)"""
        pyx_line = pyx_method.line_number
        pyi_line = pyi_member.line_number

        # 데코레이터 검증
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

        # 파라미터 검증
        self._validate_method_parameters(class_name, method_name, pyx_method, pyi_member)

        # 반환 타입 검증
        pyx_return = pyx_method.return_type.strip() if pyx_method.return_type else ""
        pyi_return = pyi_member.return_type.strip() if pyi_member.return_type else ""
        pyx_return_normalized = self._normalize_cython_type(pyx_return)
        pyi_return_normalized = self._normalize_cython_type(pyi_return)

        if pyx_return and not pyi_return:
            self.warnings.append(
                f"Class '{class_name}': method '{method_name}' return type missing in PYI "
                f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
            )
        elif pyx_return and pyi_return and pyx_return_normalized != pyi_return_normalized:
            self.warnings.append(
                f"Class '{class_name}': method '{method_name}' return type mismatch "
                f"(.pyx: '{pyx_return}' {self.pyx_file.name}:{pyx_line}, .pyi: '{pyi_return}' {self.pyi_file.name}:{pyi_line})"
            )

        # docstring 검증
        if pyx_method.docstring and not pyi_member.docstring:
            self.warnings.append(
                f"Class '{class_name}': method '{method_name}' docstring missing in PYI "
                f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
            )

    def _validate_member_variable(self, class_name: str, member_name: str, pyx_member: CythonMemberVariable, pyi_member: PyiMember):
        """개별 멤버 변수 검증"""
        pyx_line = pyx_member.line_number
        pyi_line = pyi_member.line_number

        pyx_type_normalized = self._normalize_cython_type(pyx_member.type_hint) if pyx_member.type_hint else ""
        pyi_type_normalized = self._normalize_cython_type(pyi_member.type_hint) if pyi_member.type_hint else ""

        if pyx_type_normalized != pyi_type_normalized:
            if not (pyx_type_normalized in self.COLLECTIONS and pyi_type_normalized.startswith(pyx_type_normalized)):
                self.errors.append(
                    f"Class '{class_name}': member '{member_name}' type mismatch "
                    f"(.pyx: {pyx_member.type_hint} {self.pyx_file.name}:{pyx_line}, "
                    f".pyi: {pyi_member.type_hint} {self.pyi_file.name}:{pyi_line})"
                )

    def _validate_method_parameters(self, class_name: str, method_name: str, pyx_method: CythonMethodInfo, pyi_member: PyiMember):
        """메서드 파라미터 검증"""
        pyx_params = pyx_method.args or []
        pyi_params = pyi_member.parameters or []
        pyx_line = pyx_method.line_number
        pyi_line = pyi_member.line_number

        # 파라미터 개수 검증
        if len(pyx_params) != len(pyi_params):
            self.errors.append(
                f"Class '{class_name}': method '{method_name}' parameter count mismatch "
                f"(.pyx: {len(pyx_params)} {self.pyx_file.name}:{pyx_line}, .pyi: {len(pyi_params)} {self.pyi_file.name}:{pyi_line})"
            )
            return

        # 각 파라미터 검증
        for i, (pyx_param_str, pyi_param_str) in enumerate(zip(pyx_params, pyi_params)):
            pyx_name, pyx_type = self._normalize_parameter(pyx_param_str)
            pyi_name, pyi_type = self._normalize_parameter(pyi_param_str)

            if pyi_name in pyi_member.ignored_params:
                continue

            # 파라미터 이름 검증
            if pyx_name != pyi_name:
                self.errors.append(
                    f"Class '{class_name}': method '{method_name}' parameter {i+1} name mismatch "
                    f"(.pyx: '{pyx_name}' {self.pyx_file.name}:{pyx_line}, .pyi: '{pyi_name}' {self.pyi_file.name}:{pyi_line})"
                )

            # 파라미터 타입 검증
            pyx_type_normalized = self._normalize_cython_type(pyx_type) if pyx_type else ""
            pyi_type_normalized = self._normalize_cython_type(pyi_type) if pyi_type else ""

            if pyx_type and not pyi_type:
                self.warnings.append(
                    f"Class '{class_name}': method '{method_name}' parameter '{pyx_name}' type hint missing in PYI "
                    f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
                )
            elif pyx_type and pyi_type and pyx_type_normalized != pyi_type_normalized:
                if not (pyx_type_normalized in self.COLLECTIONS and pyi_type_normalized.startswith(pyx_type_normalized)):
                    self.warnings.append(
                        f"Class '{class_name}': method '{method_name}' parameter '{pyx_name}' type mismatch "
                        f"(.pyx: '{pyx_type}' {self.pyx_file.name}:{pyx_line}, .pyi: '{pyi_type}' {self.pyi_file.name}:{pyi_line})"
                    )

    def print_results(self):
        """검증 결과 출력"""
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


def main():
    """메인 함수"""
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
