import os
from glob import glob
from pathlib import Path

import litellm


module_path = "nautilus_trader/"
stub_file_path = "stubs/"
symbol_file_path = "nautilus_trader/core/nautilus_pyo3.pyi"


def generate_stubs_instruction_prompt(symbol_code: str, cython_source_code: str) -> str :
    return f"""You are a Python programming assistant that generates .pyi stub file from the provided .pyx Cython source code.

Follow these rules strictly when extracting information and generating the .pyi stub:

# General Rules
- Only include Python-accessible symbols. Skip any cdef functions or variables that are not accessible from regular Python code.
- Do not import from cython or use cimport statements in the generated .pyi stub, skip them.
- Only standard Python imports which are exist in .pyx should be included.
- Include all Python-accessible functions, classes (with inheritances), global variables, and class members (variables, methods, and properties) with types.
- Class variables should be type hinted with 'var: ClassVar[type]' with 'from typing import ClassVar', whereas instance variables should be type hinted with 'var: type'
- Members that start with an underscore (_) are considered private and should not be included in stubs.
- Don't type hint with generic types such as Dict, Type, etc... from typing module. Just use original type.
- If the type is not explicitly specified in the pyx code or documentation, do not infer it; just keep it as it is.
- Preserve decorators like @property, @staticmethod, @classmethod, and @overload (only if present in the .pyx).
- Preserve all docstrings exactly as they appear in the .pyx file. Do not alter their content or formatting in any way.
- If a parameter is nullable or optional (i.e., has a default value of None), the stub type should be written as (param: type | None = None).
- For collections such as dict or list, always include explicit type hints for their elements .
- Skip private or internal definitions that are clearly not meant for public access.
- Function bodies in stub files just be a single ellipsis (...).

# Primitive Type Conversion (Cython → Python)
Convert Cython-specific types to appropriate Python types in the generated .pyi file:
------------
int, long -> int
uint64_t, int64_t -> int
uint32_t, int32_t -> int
size_t, ssize_t -> int
float, double -> float
str -> str
bint -> bool
type -> type
------------

# Symbol Reference
Below is a symbol definition file that you must reference in order to resolve dependencies when generating a stub file. 
The symbols defined in this file can be imported as follows: 'from nautilus_trader.core.nautilus_pyo3 import ...'
Symbols that can not found in .pxy should be imported from this file.
================
{symbol_code}
================

# Input
The Cython source code (.pyx) will be provided between the following markers:
This is given cython source code (.pyx)
================
{cython_source_code}
================

# Output
Respond only with the generated .pyi stub code. Do not include markdown formatting (such as codeblocks), explanations, comments, or filenames.
"""


def generate_stubs(symbol_code:str, cython_source_code: str) -> str:
    prompt = generate_stubs_instruction_prompt(
        symbol_code=symbol_code,
        cython_source_code=cython_source_code
    )

    response = litellm.completion(
        model="gemini/gemini-2.5-flash", # "openrouter/qwen/qwen3-32b", # "openrouter/deepseek/deepseek-r1-0528"
        messages=[{
            "role": "user",
            "content": prompt,
        }],
        reasoning_effort="disable"
    )

    return response.choices[0].message.content


def main():
    symbol_file = open(symbol_file_path)
    symbol_code = symbol_file.read()
    symbol_file.close()

    for cython_file_path in glob(module_path + "**/*.pyx", recursive=True):
        relative_path = os.path.relpath(cython_file_path, module_path)
        stub_filename = os.path.splitext(relative_path)[0] + ".pyi"
        target_path = os.path.join(stub_file_path, stub_filename)

        if Path(target_path).is_file():
            print(f"Stub for {cython_file_path} already exists at {target_path}")
            continue

        with open(cython_file_path) as f:
            stubs = generate_stubs(
                symbol_code=symbol_code,
                cython_source_code=f.read()
            )

            os.makedirs(os.path.dirname(target_path), exist_ok=True)
            with open(target_path, "w") as f_out:
                f_out.write(stubs)

            print(f"Generated stub for {cython_file_path} at {target_path}")


if __name__ == "__main__":
    main()
