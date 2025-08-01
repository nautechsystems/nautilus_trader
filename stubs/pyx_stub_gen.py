from pathlib import Path

from cython_parser import CythonCodeAnalyzer
from cython_parser import analyze_cython_code


def generate_pyi_from_analyzer_result(analyzer: CythonCodeAnalyzer) -> str:
    """Generate a .pyi stub content from the analysis result."""
    stub_content = []

    # Global variables
    for var in analyzer.global_variables:
        type_info = f": {var.type_hint}" if var.type_hint else ""
        stub_content.append(f"{var.name}{type_info}")
    if analyzer.global_variables:
        stub_content.append("")

    # Functions
    for func in analyzer.functions:
        args_str = ", ".join(func.args) if func.args else ""
        return_str = f" -> {func.return_type}" if func.return_type else ""
        stub_content.append(f"def {func.name}({args_str}){return_str}:")
        if func.docstring:
            stub_content.append(f'    """{func.docstring}"""')
        stub_content.append("    ...")
        stub_content.append("")
    if analyzer.functions:
        stub_content.append("")

    # Classes
    for cls in analyzer.classes:
        base_classes_str = f"({', '.join(cls.base_classes)})" if cls.base_classes else ""
        stub_content.append(f"class {cls.name}{base_classes_str}:")
        if cls.docstring:
            stub_content.append(f'    """{cls.docstring}"""')

        # Member variables
        for var in cls.member_variables:
            type_info = f": {var.type_hint}" if var.type_hint else ""
            stub_content.append(f"    {var.name}{type_info}")

        # Methods
        for method in cls.methods:
            decorators = []
            if method.is_static:
                decorators.append("@staticmethod")
            if method.is_classmethod:
                decorators.append("@classmethod")
            if method.is_property:
                decorators.append("@property")

            for dec in decorators:
                stub_content.append(f"    {dec}")

            args_str = ", ".join(method.args) if method.args else ""
            return_str = f" -> {method.return_type}" if method.return_type else ""
            stub_content.append(f"    def {method.name}({args_str}){return_str}:")
            if method.docstring:
                stub_content.append(f'        """{method.docstring}"""')
            stub_content.append("        ...")
        stub_content.append("")

    return "\n".join(stub_content)


def generate_stub_file(pyx_file_path: Path, output_dir: Path):
    """Analyze a .pyx file and generates a .pyi stub file."""
    if not pyx_file_path.exists():
        print(f"Error: .pyx file not found at {pyx_file_path}")
        return

    code_content = pyx_file_path.read_text(encoding="utf-8")
    analyzer_result = analyze_cython_code(name=str(pyx_file_path), code_content=code_content)

    pyi_content = generate_pyi_from_analyzer_result(analyzer_result)

    output_dir.mkdir(parents=True, exist_ok=True)
    pyi_file_path = output_dir / f"{pyx_file_path.stem}.pyi"
    pyi_file_path.write_text(pyi_content, encoding="utf-8")
    print(f"Generated stub file: {pyi_file_path}")


if __name__ == "__main__":
    pyx_file = Path("/Users/sam/Documents/Development/woung717/nautilus_trader/nautilus_trader/persistence/wranglers.pyx")
    output_directory = Path("./stubs/generated")

    generate_stub_file(pyx_file, output_directory)
