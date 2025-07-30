#!/usr/bin/env python3
"""
PYX to PYI Validation Script

이 스크립트는 Cython .pyx 파일의 클래스, 메서드, 멤버변수, docstring, 타입 어노테이션이
해당하는 .pyi 스텁 파일에 올바르게 추출되었는지 검증합니다.
"""

import ast
import keyword  # Added for Python keyword checking
import re
import sys
from dataclasses import dataclass
from dataclasses import field
from pathlib import Path


@dataclass
class Member:
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
class ClassInfo:
    """클래스 정보"""

    name: str
    docstring: str | None = None
    members: dict[str, Member] = None
    base_classes: list[str] = None
    line_number: int | None = None

    def __post_init__(self):
        if self.members is None:
            self.members = {}
        if self.base_classes is None:
            self.base_classes = []


class PyxParser:
    """Cython .pyx 파일 파서"""

    def __init__(self, file_path: Path):
        self.file_path = file_path
        self.content = file_path.read_text(encoding="utf-8")
        self.classes: dict[str, ClassInfo] = {}
        self.PYTHON_KEYWORDS = set(keyword.kwlist) # Initialize Python keywords

    def parse(self) -> dict[str, ClassInfo]:
        """Pyx 파일을 파싱하여 클래스 정보 추출"""
        comment_line_pattern = re.compile(r"^\s*#.*$")
        lines = [re.sub(comment_line_pattern, "", l) for l in self.content.split("\n")]
        current_class = None
        current_indent = 0
        in_docstring = False
        docstring_quote = None
        i = 0


        while i < len(lines):
            line = lines[i].rstrip()
            if not line:
                i += 1
                continue

            indent = len(line) - len(line.lstrip())
            stripped_line = line.strip()

            # docstring 상태 추적
            if not in_docstring:
                if stripped_line.startswith(('"""', "'''")):
                    docstring_quote = '"""' if stripped_line.startswith('"""') else "'''"
                    # 한 줄 docstring인지 확인
                    if stripped_line.count(docstring_quote) >= 2 and len(stripped_line) > 6:
                        # 한 줄 docstring - 다음 라인으로
                        i += 1
                        continue
                    else:
                        # 여러 줄 docstring 시작
                        in_docstring = True
                        i += 1
                        continue
            else:
                # docstring 내부
                if docstring_quote in stripped_line:
                    # docstring 끝
                    in_docstring = False
                    docstring_quote = None
                i += 1
                continue

            # docstring 내부라면 파싱하지 않음
            if in_docstring:
                i += 1
                continue

            # 클래스 정의 찾기
            class_match = re.match(r"^(\s*)(?:cdef\s+)?class\s+(\w+)(?:\((.*?)\))?:", line)
            if class_match:
                class_indent, class_name, bases = class_match.groups()
                current_indent = len(class_indent)

                base_classes = []
                if bases:
                    base_classes = [b.strip() for b in bases.split(",") if b.strip()]

                current_class = ClassInfo(name=class_name, base_classes=base_classes, line_number=i+1)

                # docstring 추출 - _extract_docstring 메서드 사용
                i += 1
                if i < len(lines):
                    docstring, extra_lines = self._extract_docstring(lines, i)
                    if docstring:
                        current_class.docstring = docstring
                        i += extra_lines

                self.classes[class_name] = current_class
                continue

            # 클래스 내부의 멤버들 파싱
            if current_class and indent > current_indent:
                member_info = self._parse_member(line, lines, i)
                if member_info:
                    member, lines_consumed = member_info
                    member.line_number = i + 1  # 라인 넘버 추가
                    current_class.members[member.name] = member
                    i += lines_consumed
                    continue

            # 클래스 끝
            if current_class and indent <= current_indent and line.strip():
                current_class = None
                current_indent = 0

            i += 1

        return self.classes

    def _parse_member(self, line: str, lines: list[str], start_idx: int) -> tuple[Member, int] | None:
        """멤버 변수/메서드 파싱 (데코레이터 지원 추가, cpdef 메서드 포함)"""
        line = line.strip()

        # cdef 멤버 변수는 검증에서 제외 (Python에서 접근 불가)
        # 단, cpdef는 포함 (Python에서도 접근 가능)
        if re.match(r"^\s*cdef\s+(?!.*\bdef\b)", line):
            return None

        # 데코레이터 수집
        decorators = []
        current_line_idx = start_idx

        # 현재 라인이 데코레이터인지 확인하고 수집
        while current_line_idx < len(lines):
            current_line = lines[current_line_idx].strip()
            if current_line.startswith("@"):
                decorators.append(current_line)
                current_line_idx += 1
            else:
                break

        # 데코레이터 다음의 실제 정의 라인
        if current_line_idx < len(lines):
            definition_line = lines[current_line_idx].strip()
        else:
            return None

        lines_consumed = current_line_idx - start_idx

        # 메서드 정의 확인 (def, cpdef 모두 포함)
        method_start = re.match(r"(?:cpdef\s+|cdef\s+)?def\s+(\w+)\s*\(", definition_line)
        if method_start:
            method_name = method_start.group(1)
            if method_name in self.PYTHON_KEYWORDS: # Filter out Python keywords
                return None

            # 전체 메서드 시그니처 수집 (여러 줄 가능)
            full_signature = definition_line
            paren_count = definition_line.count("(") - definition_line.count(")")

            # 괄호가 닫힐 때까지 다음 줄들 수집
            while paren_count > 0 and current_line_idx + 1 < len(lines):
                current_line_idx += 1
                next_line = lines[current_line_idx].rstrip()
                full_signature += " " + next_line.strip()
                paren_count += next_line.count("(") - next_line.count(")")

            lines_consumed = current_line_idx - start_idx + 1

            # 완전한 시그니처에서 파라미터와 반환 타입 추출
            signature_match = re.match(r"(?:cpdef\s+|cdef\s+)?def\s+\w+\s*\((.*?)\)(?:\s*->\s*(.+?))?:", full_signature)
            if signature_match:
                params, return_type = signature_match.groups()

                # 파라미터 파싱
                param_list = self._parse_parameters(params)

                # 데코레이터 분석
                is_property = any("@property" in dec for dec in decorators)
                is_staticmethod = any("@staticmethod" in dec for dec in decorators)
                is_classmethod = any("@classmethod" in dec for dec in decorators)
                is_overload = any("@overload" in dec for dec in decorators)

                member = Member(
                    name=method_name,
                    is_method=True,
                    is_property=is_property,
                    is_staticmethod=is_staticmethod,
                    is_classmethod=is_classmethod,
                    is_overload=is_overload,
                    parameters=param_list,
                    return_type=return_type.strip() if return_type else None
                )

                # docstring 추출 - _extract_docstring 메서드 사용
                if current_line_idx + 1 < len(lines):
                    docstring, extra_lines = self._extract_docstring(lines, current_line_idx + 1)
                    if docstring:
                        member.docstring = docstring
                        lines_consumed += extra_lines

                return member, lines_consumed

        # 프로퍼티 (레거시 지원 - 데코레이터가 이미 처리됨)
        if line.startswith("@property") and start_idx + 1 < len(lines):
            next_line = lines[start_idx + 1].strip()
            prop_method = re.match(r"def\s+(\w+)\s*\(.*?\)(?:\s*->\s*(.+?))?:", next_line)
            if prop_method:
                prop_name, return_type = prop_method.groups()
                member = Member(
                    name=prop_name,
                    is_property=True,
                    return_type=return_type.strip() if return_type else None
                )
                return member, 2

        # 멤버 변수 (Python 접근 가능한 것만)
        # cdef가 포함된 변수 정의는 제외 (단, cpdef는 허용)
        if re.match(r"^\s*cdef\s+(?!.*\bdef\b)", line):
            return None

        # Python 타입 힌트 형식만 허용
        # Member variables must be explicitly prefixed with 'self.'
        # This regex specifically looks for 'self.variable_name: type_hint' patterns.
        var_match = re.match(r"self\.(\w+)\s*:\s*(.+?)(?:\s*=.*)?$", line)

        if var_match:
            var_name, type_hint = var_match.groups()
            if var_name in self.PYTHON_KEYWORDS: # Filter out Python keywords
                return None
            member = Member(
                name=var_name,
                type_hint=type_hint.strip()
            )
            return member, 1

        return None

    def _parse_parameters(self, params_str: str) -> list[str]:
        """파라미터 문자열을 파싱하여 리스트로 반환 (Cython 문법 지원)"""
        if not params_str.strip():
            return []

        param_list = []
        current_param = ""
        paren_depth = 0
        bracket_depth = 0
        in_string = False
        string_char = None

        for char in params_str:
            if not in_string:
                if char in ['"', "'"]:
                    in_string = True
                    string_char = char
                elif char == "(":
                    paren_depth += 1
                elif char == ")":
                    paren_depth -= 1
                elif char == "[":
                    bracket_depth += 1
                elif char == "]":
                    bracket_depth -= 1
                elif char == "," and paren_depth == 0 and bracket_depth == 0:
                    # 최상위 레벨의 콤마만 파라미터 구분자로 사용
                    param = current_param.strip()
                    if param and param != "self" and param not in ["**kwargs"]:
                        # Cython 파라미터 정규화
                        normalized_param = self._normalize_cython_parameter(param)
                        param_name = normalized_param.split(":")[0].strip().split("=")[0].strip() # Extract name
                        if param_name not in self.PYTHON_KEYWORDS: # Filter out Python keywords
                            param_list.append(normalized_param)
                    current_param = ""
                    continue
            else:
                if char == string_char:
                    in_string = False
                    string_char = None

            current_param += char

        # 마지막 파라미터 처리
        param = current_param.strip()
        if param and param != "self":
            normalized_param = self._normalize_cython_parameter(param)
            param_name = normalized_param.split(":")[0].strip().split("=")[0].strip() # Extract name
            if param_name not in self.PYTHON_KEYWORDS: # Filter out Python keywords
                param_list.append(normalized_param)

        return param_list

    def _normalize_cython_parameter(self, param: str) -> str:
        """
        Cython 파라미터를 Python 스타일로 정규화

        예시:
        - "UUID4 event_id not None" -> "event_id: UUID4"
        - "str name" -> "name: str"
        - "int count = 0" -> "count: int = 0"
        - "event_id: UUID4" -> "event_id: UUID4" (이미 Python 형식인 경우 그대로)
        """
        # 일반적인 Cython 한정자들: not None, or None, nogil, etc.
        cython_qualifiers = [
            "not None",
            "or None",
            "nogil",
            "except *",
            "except +",
            "except?"
        ]

        param = param.strip()

        # 이미 Python 형식인지 확인 (param: type 패턴)
        if ":" in param:
            for qualifier in cython_qualifiers:
                if qualifier in param:
                    param = param.replace(qualifier, "").strip()
            return param

        # 기본값 분리 (= 이후)
        default_value = ""
        if "=" in param:
            param, default_value = param.split("=", 1)
            param = param.strip()
            default_value = f" = {default_value.strip()}"

        for qualifier in cython_qualifiers:
            if qualifier in param:
                param = param.replace(qualifier, "").strip()

        # 토큰들로 분할
        tokens = param.split()
        if len(tokens) == 0:
            return param
        elif len(tokens) == 1:
            # 타입이나 이름만 있는 경우
            return tokens[0]
        elif len(tokens) == 2:
            # "타입 이름" 형태 (Cython 스타일)
            type_hint, param_name = tokens
            return f"{param_name}: {type_hint}{default_value}"
        else:
            # 더 복잡한 경우는 마지막 토큰을 이름으로, 나머지를 타입으로 처리
            param_name = tokens[-1]
            type_hint = " ".join(tokens[:-1])
            return f"{param_name}: {type_hint}{default_value}"

    def _is_in_docstring(self, lines: list[str], line_idx: int) -> bool:
        # 현재 라인부터 역순으로 검사하여 docstring 시작을 찾음
        docstring_start = None
        docstring_quote = None

        for i in range(line_idx, -1, -1):
            line = lines[i].strip()

            # docstring 끝 (현재 위치에서 역순 검사)
            if line.endswith('"""') and not line.startswith('"""'):
                # 이미 docstring 내부라면 시작점 확인 계속
                continue
            elif line.endswith("'''") and not line.startswith("'''"):
                continue

            # docstring 시작 찾기
            if '"""' in line:
                # 한 줄 docstring인지 확인
                if line.count('"""') >= 2:
                    # 한 줄 docstring - 현재 라인이 이 라인이면 docstring 내부
                    return i == line_idx
                else:
                    # 여러 줄 docstring 시작
                    docstring_start = i
                    docstring_quote = '"""'
                    break
            elif "'''" in line:
                if line.count("'''") >= 2:
                    return i == line_idx
                else:
                    docstring_start = i
                    docstring_quote = "'''"
                    break

        # docstring 시작을 찾았다면, 현재 라인이 그 범위 내인지 확인
        if docstring_start is not None:
            # docstring 끝 찾기
            for i in range(docstring_start + 1, len(lines)):
                if docstring_quote in lines[i]:
                    docstring_end = i
                    return docstring_start < line_idx <= docstring_end

        return False

    def _extract_docstring(self, lines: list[str], start_idx: int) -> tuple[str | None, int]:
        """Docstring 추출 - 통일된 메서드"""
        if start_idx >= len(lines):
            return None, 0

        line = lines[start_idx].strip()

        # docstring 시작 확인
        quote = None
        if line.startswith('"""'):
            quote = '"""'
        elif line.startswith("'''"):
            quote = "'''"
        else:
            return None, 0

        # 한 줄 docstring 확인
        if line.endswith(quote) and len(line) > 6:
            # 한 줄 docstring
            docstring = line[3:-3].strip()
            return docstring if docstring else None, 1

        # 여러 줄 docstring
        docstring_lines = [line[3:]]  # 첫 번째 줄에서 """ 제거
        lines_consumed = 1

        for i in range(start_idx + 1, len(lines)):
            line_content = lines[i].rstrip()
            if line_content.strip().endswith(quote):
                # docstring 끝 발견
                remaining_content = line_content.rstrip()[:-3]
                if remaining_content:
                    docstring_lines.append(remaining_content)
                lines_consumed += 1
                break
            docstring_lines.append(line_content)
            lines_consumed += 1

        docstring = "\n".join(docstring_lines).strip()
        return docstring if docstring else None, lines_consumed


class PyiParser:
    """Python .pyi 스텁 파일 파서"""

    def __init__(self, file_path: Path):
        self.file_path = file_path
        self.file_content = self.file_path.read_text(encoding="utf-8")
        self.file_lines = self.file_content.splitlines()
        self.classes: dict[str, ClassInfo] = {}

    def parse(self) -> dict[str, ClassInfo]:
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

    def _parse_class(self, node: ast.ClassDef) -> ClassInfo:
        """클래스 노드 파싱"""
        # 베이스 클래스
        base_classes = []
        for base in node.bases:
            if isinstance(base, ast.Name):
                base_classes.append(base.id)
            elif isinstance(base, ast.Attribute):
                base_classes.append(ast.unparse(base))

        class_info = ClassInfo(
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
                    if arg.arg != "self":
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

                member = Member(
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
                member = Member(
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
                        member = Member(
                            name=target.id,
                            line_number=item.lineno,
                            ignore_validation=self._is_ignored(item),
                        )
                        class_info.members[target.id] = member

        return class_info


class PyxPyiValidator:
    """PYX와 PYI 파일 검증기"""

    def __init__(self, pyx_file: Path, pyi_file: Path):
        self.pyx_file = pyx_file
        self.pyi_file = pyi_file
        self.pyx_classes = {}
        self.pyi_classes = {}
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
            pyx_parser = PyxParser(self.pyx_file)
            self.pyx_classes = pyx_parser.parse()
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

    def _validate_class(self, pyx_class: ClassInfo, pyi_class: ClassInfo):
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
        self._validate_members(class_name, pyx_class.members, pyi_class.members)

    def _validate_members(self, class_name: str, pyx_members: dict[str, Member], pyi_members: dict[str, Member]):
        """클래스 멤버들 검증"""
        # private 멤버 제외한 public 멤버들만 검증
        pyx_public = {name: member for name, member in pyx_members.items()
                     if not name.startswith("__") or name.endswith("__")}

        pyx_member_names = set(pyx_public.keys())
        pyi_member_names = set(pyi_members.keys())

        # 누락된 멤버들
        missing_members = pyx_member_names - pyi_member_names
        for member_name in missing_members:
            member = pyx_public[member_name]
            pyx_line = member.line_number
            if not member.is_private:  # public 멤버만 에러로 처리
                self.errors.append(f"Class '{class_name}': member '{member_name}' missing in PYI ({self.pyx_file.name}:{pyx_line})")
            else:
                self.warnings.append(f"Class '{class_name}': private member '{member_name}' missing in PYI ({self.pyx_file.name}:{pyx_line})")

        # 공통 멤버들 검증
        common_members = pyx_member_names & pyi_member_names
        for member_name in common_members:
            if pyi_members[member_name].ignore_validation:
                continue
            self._validate_member(
                class_name, member_name,
                pyx_public[member_name],
                pyi_members[member_name]
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
            # Add more mappings as needed
        }

        # Clean the type string (remove whitespace, make lowercase)
        cleaned_type = cython_type.strip().lower()

        for cython_type, python_type in type_map.items():
            if cython_type in cleaned_type:
                cleaned_type = cleaned_type.replace(cython_type, python_type)

        return cleaned_type

    def _validate_member(self, class_name: str, member_name: str, pyx_member: Member, pyi_member: Member):
        """개별 멤버 검증 (데코레이터 검증 추가)"""
        pyx_line = pyx_member.line_number
        pyi_line = pyi_member.line_number

        # 메서드/프로퍼티 타입 검증
        if not pyx_member.is_method and not pyi_member.is_method:
            pyx_member_normalized_type = self._normalize_cython_type(pyx_member.type_hint) if pyx_member.type_hint else ""
            pyi_member_normalized_type = self._normalize_cython_type(pyi_member.type_hint) if pyi_member.type_hint else ""

            if pyx_member_normalized_type != pyi_member_normalized_type:
                if not (pyx_member_normalized_type in self.COLLECTIONS and pyi_member_normalized_type.startswith(pyx_member_normalized_type)):
                    self.errors.append(
                        f"Class '{class_name}': member '{member_name}' type mismatch "
                        f"(.pyx: {pyx_member.type_hint} {self.pyx_file.name}:{pyx_line}, "
                        f".pyi: {pyi_member.type_hint} {self.pyi_file.name}:{pyi_line})"
                    )

        # 데코레이터 검증
        if pyx_member.is_property != pyi_member.is_property:
            self.errors.append(
                f"Class '{class_name}': member '{member_name}' @property decorator mismatch "
                f"(.pyx: {pyx_member.is_property} {self.pyx_file.name}:{pyx_line}, "
                f".pyi: {pyi_member.is_property} {self.pyi_file.name}:{pyi_line})"
            )

        if pyx_member.is_staticmethod != pyi_member.is_staticmethod:
            self.errors.append(
                f"Class '{class_name}': member '{member_name}' @staticmethod decorator mismatch "
                f"(.pyx: {pyx_member.is_staticmethod} {self.pyx_file.name}:{pyx_line}, "
                f".pyi: {pyi_member.is_staticmethod} {self.pyi_file.name}:{pyi_line})"
            )

        if pyx_member.is_classmethod != pyi_member.is_classmethod:
            self.errors.append(
                f"Class '{class_name}': member '{member_name}' @classmethod decorator mismatch "
                f"(.pyx: {pyx_member.is_classmethod} {self.pyx_file.name}:{pyx_line}, "
                f".pyi: {pyi_member.is_classmethod} {self.pyi_file.name}:{pyi_line})"
            )

        if pyx_member.is_overload != pyi_member.is_overload:
            self.warnings.append(
                f"Class '{class_name}': member '{member_name}' @overload decorator mismatch "
                f"(.pyx: {pyx_member.is_overload} {self.pyx_file.name}:{pyx_line}, "
                f".pyi: {pyi_member.is_overload} {self.pyi_file.name}:{pyi_line})"
            )

        # 메서드의 경우 파라미터 검증
        if pyx_member.is_method and pyi_member.is_method:
            self._validate_method_parameters(class_name, member_name, pyx_member, pyi_member)

        # 타입 힌트 검증
        if pyx_member.type_hint and not pyi_member.type_hint:
            self.warnings.append(
                f"Class '{class_name}': member '{member_name}' type hint missing in PYI "
                f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
            )

        # 반환 타입 검증 (메서드의 경우)
        if pyx_member.is_method:
            pyx_return = pyx_member.return_type.strip() if pyx_member.return_type else ""
            pyi_return = pyi_member.return_type.strip() if pyi_member.return_type else ""
            pyx_return_normalized = self._normalize_cython_type(pyx_return)
            pyi_return_normalized = self._normalize_cython_type(pyi_return)

            if pyx_return and not pyi_return:
                self.warnings.append(
                    f"Class '{class_name}': method '{member_name}' return type missing in PYI "
                    f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
                )
            elif pyx_return and pyi_return and pyx_return_normalized != pyi_return_normalized:
                self.warnings.append(
                    f"Class '{class_name}': method '{member_name}' return type mismatch "
                    f"(.pyx: '{pyx_return}' {self.pyx_file.name}:{pyx_line}, .pyi: '{pyi_return}' {self.pyi_file.name}:{pyi_line})"
                )

        # docstring 검증
        if pyx_member.docstring and not pyi_member.docstring:
            self.warnings.append(
                f"Class '{class_name}': member '{member_name}' docstring missing in PYI "
                f"({self.pyx_file.name}:{pyx_line}, {self.pyi_file.name}:{pyi_line})"
            )

    def _validate_method_parameters(self, class_name: str, method_name: str, pyx_member: Member, pyi_member: Member):
        """메서드 파라미터 검증"""
        pyx_params = pyx_member.parameters or []
        pyi_params = pyi_member.parameters or []
        pyx_line = pyx_member.line_number
        pyi_line = pyi_member.line_number

        # 파라미터 개수 검증
        if len(pyx_params) != len(pyi_params):
            self.errors.append(
                f"Class '{class_name}': method '{method_name}' parameter count mismatch "
                f"(.pyx: {len(pyx_params)} {self.pyx_file.name}:{pyx_line}, .pyi: {len(pyi_params)} {self.pyi_file.name}:{pyi_line})"
            )
            return

        # 각 파라미터 검증
        for i, (pyx_param, pyi_param) in enumerate(zip(pyx_params, pyi_params)):
            pyx_name, pyx_type = self._normalize_parameter(pyx_param)
            pyi_name, pyi_type = self._normalize_parameter(pyi_param)

            if pyi_name in pyi_member.ignored_params:
                continue

            # When Cython type is attached in pyx_name, dettach it
            pyx_name_split = pyx_name.split()
            if len(pyx_name_split) > 1:
                pyx_name = pyx_name_split[-1]

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

        if self.warnings:
            print(f"\n⚠️  WARNINGS ({len(self.warnings)}):")
            for warning in self.warnings:
                print(f"  • {warning}")

        print(f"\n* {len(self.errors)} errors, {len(self.warnings)} warnings")


def main():
    """메인 함수"""
    if len(sys.argv) != 3:
        print(f"Usage: python {sys.argv[0]} <pyx_file> <pyi_file>")
        sys.exit(1)

    pyx_file = Path(sys.argv[1])
    pyi_file = Path(sys.argv[2])

    validator = PyxPyiValidator(pyx_file, pyi_file)
    success = validator.validate()
    validator.print_results()

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
