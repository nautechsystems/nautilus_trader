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
    name: str
    type_hint: str | None = None
    is_private: bool = False
    default_value: str | None = None
    is_public: bool = False  # cdef public 여부
    is_readonly: bool = False  # cdef readonly 여부


@dataclass
class MethodInfo:
    name: str
    args: list[str] = field(default_factory=list)
    return_type: str | None = None
    is_private: bool = False
    is_static: bool = False
    is_classmethod: bool = False
    is_cdef: bool = False
    is_cpdef: bool = False
    is_property: bool = False


@dataclass
class FunctionInfo:
    name: str
    args: list[str] = field(default_factory=list)
    return_type: str | None = None
    is_cdef: bool = False
    is_cpdef: bool = False


@dataclass
class GlobalVariable:
    name: str
    type_hint: str | None = None
    value: str | None = None
    is_constant: bool = False
    is_cdef: bool = False


@dataclass
class ClassInfo:
    name: str
    base_classes: list[str] = field(default_factory=list)
    member_variables: list[MemberVariable] = field(default_factory=list)
    methods: list[MethodInfo] = field(default_factory=list)
    is_cdef_class: bool = False
    is_extension_type: bool = False


class CythonCodeAnalyzer(ScopeTrackingTransform):
    def __init__(self, context):
        super().__init__(context=context)
        self.classes: list[ClassInfo] = []
        self.functions: list[FunctionInfo] = []
        self.global_variables: list[GlobalVariable] = []
        self.current_class: ClassInfo | None = None
        self.current_function: FunctionInfo | MethodInfo | None = None
        self.class_stack: list[ClassInfo] = []

    def visit_ModuleNode(self, node):
        """모듈 노드 방문"""
        self.visitchildren(node)
        return node

    def visit_CClassDefNode(self, node):
        """Cython 확장 타입 클래스 정의 노드 방문 (cdef class)"""
        return self._visit_class_node(node, is_cdef_class=True, is_extension_type=True)

    def visit_PyClassDefNode(self, node):
        """Python 클래스 정의 노드 방문 (class)"""
        return self._visit_class_node(node, is_cdef_class=False, is_extension_type=False)

    def _visit_class_node(self, node, is_cdef_class=False, is_extension_type=False):
        """클래스 노드 공통 처리"""
        # 기본 클래스 추출
        base_classes = []
        if hasattr(node, "bases") and node.bases:
            for arg in node.bases.args:
                base_name = self._extract_name_from_node(arg)
                if base_name:
                    base_classes.append(base_name)

        # 클래스 이름 추출
        class_name = getattr(node, "class_name", None) or getattr(node, "name", "Unknown")

        # 새 클래스 정보 생성
        class_info = ClassInfo(
            name=class_name,
            base_classes=base_classes,
            is_cdef_class=is_cdef_class,
            is_extension_type=is_extension_type
        )

        # 클래스 스택에 추가
        self.class_stack.append(class_info)
        self.current_class = class_info

        # 자식 노드들 방문
        self.visitchildren(node)

        # 클래스 스택에서 제거
        self.class_stack.pop()
        self.current_class = self.class_stack[-1] if self.class_stack else None

        self.classes.append(class_info)
        return node

    def visit_FuncDefNode(self, node):
        """Python 함수 정의 노드 방문 (def)"""
        return self._visit_function_node(node, is_cdef=False, is_cpdef=False)

    def visit_CFuncDefNode(self, node):
        """Cython C 함수 정의 노드 방문 (cdef)"""
        return self._visit_function_node(node, is_cdef=not node.overridable, is_cpdef=node.overridable)

    def visit_DefNode(self, node):
        """일반적인 함수 정의 노드 방문"""
        return self._visit_function_node(node)

    def _visit_function_node(self, node, is_cdef=False, is_cpdef=False):
        """함수/메소드 노드 공통 처리"""
        is_cfunc = is_cdef or is_cpdef
        function_name = node.name if not is_cdef and not is_cpdef else node.declarator.base.name

        args = self._extract_function_args(node, is_cfunc)
        return_type = self._extract_return_type(node, is_cfunc)
        decorators = self._analyze_decorators(node)

        is_private = (function_name.startswith("_") and not function_name.startswith("__")) or function_name.endswith("__")

        if self.current_class:
            # 클래스 메소드인 경우
            method_info = MethodInfo(
                name=function_name,
                args=args,
                return_type=return_type,
                is_private=is_private,
                is_static=decorators.get("staticmethod", False),
                is_classmethod=decorators.get("classmethod", False),
                is_property=decorators.get("property", False),
                is_cdef=is_cdef,
                is_cpdef=is_cpdef
            )
            self.current_class.methods.append(method_info)
            self.current_function = method_info
        else:
            # 일반 함수인 경우
            function_info = FunctionInfo(
                name=function_name,
                args=args,
                return_type=return_type,
                is_cdef=is_cdef,
                is_cpdef=is_cpdef
            )
            self.functions.append(function_info)
            self.current_function = function_info

        self.visitchildren(node)

        self.current_function = None

        return node

    def visit_CVarDefNode(self, node):
        """C 변수 정의 노드 방문 (cdef)"""
        self._process_variable_declaration(node, is_cdef=True)
        self.visitchildren(node)
        return node

    def visit_SingleAssignmentNode(self, node):
        """단일 할당 노드 방문 (Python 스타일 변수 할당)"""
        if hasattr(node, "lhs") and hasattr(node, "rhs"):
            var_name = self._extract_name_from_node(node.lhs)
            if var_name:
                value = self._extract_value_from_node(node.rhs)
                self._add_variable(var_name, None, value, is_cdef=False)

        self.visitchildren(node)
        return node

    def _process_variable_declaration(self, node, is_cdef=False):
        """변수 선언 처리"""
        if not hasattr(node, "declarators"):
            return

        base_type = self._extract_type_from_node(getattr(node, "base_type", None))

        # visibility 확인
        is_public = getattr(node, "visibility", None) == "public"
        is_readonly = getattr(node, "visibility", None) == "readonly"

        for declarator in node.declarators:
            var_name = getattr(declarator, "name", None)
            if not var_name:
                continue

            default_value = None
            if hasattr(declarator, "default") and declarator.default:
                default_value = self._extract_value_from_node(declarator.default)

            self._add_variable(
                var_name,
                base_type,
                default_value,
                is_cdef=is_cdef,
                is_public=is_public,
                is_readonly=is_readonly
            )

    def _add_variable(self, name, type_hint, value, is_cdef=False, is_public=False, is_readonly=False):
        """변수 추가"""
        is_private = name.startswith("_") and not name.startswith("__")
        is_constant = name.isupper() and "_" in name  # 상수 패턴

        if self.current_class and name.startswith("self.") and self.current_function.name == "__init__":
            # 클래스 멤버 변수
            member_var = MemberVariable(
                name=name,
                type_hint=type_hint,
                is_private=is_private,
                default_value=value,
                is_public=is_public,
                is_readonly=is_readonly
            )
            self.current_class.member_variables.append(member_var)
        elif self.current_function is None:
            # 글로벌 변수
            global_var = GlobalVariable(
                name=name,
                type_hint=type_hint,
                value=value,
                is_constant=is_constant,
                is_cdef=is_cdef
            )
            self.global_variables.append(global_var)

    def _extract_function_args(self, node, is_cfunc) -> list[str]:
        """함수 매개변수 추출"""
        args = []
        node_args = node.declarator.args if is_cfunc else node.args

        for arg in node_args:
            arg_name = getattr(arg.declarator, "name", str(arg))

            # 타입 정보 추출
            arg_type = None
            if hasattr(arg, "type") and arg.type:
                arg_type = self._extract_type_from_node(arg.type)
            elif hasattr(arg, "base_type") and arg.base_type:
                arg_type = self._extract_type_from_node(arg.base_type)

            # 기본값 추출
            default_val = None
            if hasattr(arg, "default") and arg.default:
                default_val = self._extract_value_from_node(arg.default)

            # 매개변수 문자열 구성
            arg_str = arg_name
            if arg_type:
                arg_str += f": {arg_type}"
            if default_val:
                arg_str += f" = {default_val}"

            args.append(arg_str)

        return args

    def _extract_return_type(self, node, is_cfunc) -> str | None:
        """반환 타입 추출"""
        if is_cfunc:
            return self._extract_name_from_node(node.base_type)
        if hasattr(node, "return_type_annotation") and node.return_type_annotation:
            return self._extract_type_from_node(node.return_type_annotation.expr)
        return None

    def _analyze_decorators(self, node) -> dict[str, bool]:
        """데코레이터 분석"""
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
        """노드에서 이름 추출"""
        if node is None:
            return None
        if hasattr(node, "name"):
            return node.name
        if hasattr(node, "attribute") and hasattr(node, "obj"):
            obj_name = self._extract_name_from_node(node.obj)
            return f"{obj_name}.{node.attribute}" if obj_name else node.attribute
        return str(node) if node else None

    def _extract_type_from_node(self, type_node) -> str | None:
        """타입 노드에서 문자열 추출"""
        if type_node is None:
            return None
        if hasattr(type_node, "name"):
            return type_node.name
        if isinstance(type_node, PyrexTypes.BaseType):
            return str(type_node)
        if hasattr(type_node, "__str__"):
            return str(type_node)
        return None

    def _extract_value_from_node(self, node) -> str | None:
        """노드의 값을 문자열로 추출"""
        if node is None:
            return None
        if hasattr(node, "value"):
            return str(node.value)
        if hasattr(node, "compile_time_value"):
            return "expr"
        return str(node)


def analyze_cython_code(name:str, code_content: str) -> CythonCodeAnalyzer:
    """Cython 코드를 분석하여 정보를 추출합니다."""
    options = CompilationOptions(default_options)
    context = Context(include_directories="./", compiler_directives={}, options=options)
    try:
        # TreeFragment를 사용하여 Cython 코드를 AST로 파싱
        tree = parse_from_strings(name, code_content)

        if tree:
            analyzer = CythonCodeAnalyzer(context)
            # CythonTransform을 사용하여 트리 변환/분석
            analyzer.visit(tree)
            return analyzer
        else:
            raise ValueError("코드 파싱에 실패했습니다.")

    except Exception as e:
        print(f"분석 중 오류 발생: {e}")
        import traceback
        traceback.print_exc()
        # 기본 분석기 반환
        return CythonCodeAnalyzer()


def print_analysis_results(analyzer: CythonCodeAnalyzer):  # noqa: C901
    """분석 결과를 출력합니다."""
    print("=" * 60)
    print("CYTHON 코드 분석 결과 (CythonTransform 사용)")
    print("=" * 60)

    # 클래스 정보 출력
    if analyzer.classes:
        print("\n📦 클래스들:")
        for cls in analyzer.classes:
            class_type = "cdef class" if cls.is_cdef_class else "class"
            extension_info = " (확장 타입)" if cls.is_extension_type else ""
            print(f"\n{class_type}: {cls.name}{extension_info}")

            if cls.base_classes:
                print(f"  상속: {', '.join(cls.base_classes)}")

            # 멤버 변수
            if cls.member_variables:
                print("  멤버 변수:")
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

                    print(f"    - {var.name}{type_info}{default_info} ({visibility}){modifier_str}")

            # 메소드
            if cls.methods:
                print("  메소드:")
                for method in cls.methods:
                    visibility = "private" if method.is_private else "public"

                    # 함수 타입 확인
                    func_type = ""
                    if method.is_cdef:
                        func_type = "cdef "
                    elif method.is_cpdef:
                        func_type = "cpdef "
                    else:
                        func_type = "def "

                    # 데코레이터
                    decorators = []
                    if method.is_static:
                        decorators.append("@staticmethod")
                    if method.is_classmethod:
                        decorators.append("@classmethod")
                    if method.is_property:
                        decorators.append("@property")

                    decorator_str = " ".join(decorators) + " " if decorators else ""
                    args_str = ", ".join(method.args) if method.args else ""
                    return_str = f" -> {method.return_type}"

                    print(f"    - {decorator_str}{func_type}{method.name}({args_str}){return_str} ({visibility})")

    # 일반 함수 정보 출력
    if analyzer.functions:
        print("\n🔧 함수들:")
        for func in analyzer.functions:
            func_type = ""
            if func.is_cdef:
                func_type = "cdef "
            elif func.is_cpdef:
                func_type = "cpdef "
            else:
                func_type = "def "

            args_str = ", ".join(func.args) if func.args else ""
            return_str = f" -> {func.return_type}" if func.return_type else ""
            print(f"  - {func_type}{func.name}({args_str}){return_str}")

    # 글로벌 변수 정보 출력
    if analyzer.global_variables:
        print("\n🌍 글로벌 변수들:")
        for var in analyzer.global_variables:
            var_type = "cdef " if var.is_cdef else ""
            type_info = f": {var.type_hint}" if var.type_hint else ""
            value_info = f" = {var.value}" if var.value else ""
            classification = "상수" if var.is_constant else "변수"
            print(f"  - {var_type}{var.name}{type_info}{value_info} ({classification})")


# 사용 예제
if __name__ == "__main__":
    file_path = Path("/Users/sam/Documents/Development/woung717/nautilus_trader/nautilus_trader/accounting/accounts/cash.pyx")
    code = file_path.read_text(encoding="utf-8")

    # 코드 분석 실행
    analyzer = analyze_cython_code(name=str(file_path), code_content=code)

    # 결과 출력
    print_analysis_results(analyzer)
