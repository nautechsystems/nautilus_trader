import argparse
import glob
import os
import sys
from pathlib import Path

import litellm

from stubs.validate_pyx_stubs import PyxPyiValidator  # Import the validator class


def run_validation(pyx_file: Path, pyi_file: Path) -> tuple[bool, str]:
    """
    Run the validation using PyxPyiValidator class and captures its output.
    Returns (True, output) if validation passes, (False, output) otherwise.
    """
    try:
        validator = PyxPyiValidator(pyx_file, pyi_file, include_private=True)
        success = validator.validate()
        output = validator.results()

        return success, output
    except Exception as e:
        return False, f"An unexpected error occurred during validation: {e}"

def get_llm_response(prompt: str) -> str:
    """
    Get a response from the LLM using litellm.
    """
    try:
        response = litellm.completion(
            model="gemini/gemini-2.5-flash",
            messages=[{"role": "user", "content": prompt}],
            reasoning_effort="disable"
        )
        return response.choices[0].message.content
    except Exception as e:
        print(f"Error calling LLM: {e}", file=sys.stderr)
        return ""

def correct_pyi_with_llm(pyx_file: Path, pyi_file: Path, max_attempts: int = 3):
    """
    Attempt to correct a .pyi file using an LLM based on validation feedback.
    """
    print(f"Attempting to correct {pyi_file} for {pyx_file}")

    for attempt in range(1, max_attempts + 1):
        print(f"\n--- Attempt {attempt}/{max_attempts} ---")

        # 1. Run validation
        success, validation_output = run_validation(pyx_file, pyi_file)
        print(validation_output)

        if success:
            print(f"✅ Validation passed for {pyi_file}!")
            return True

        try:
            current_pyx_content = pyx_file.read_text(encoding="utf-8")
            current_pyi_content = pyi_file.read_text(encoding="utf-8")
        except FileNotFoundError as e:
            print(f"Error: File not found: {e}", file=sys.stderr)
            return False

        prompt = f"""
You are an expert Python developer specializing in Cython stub files (.pyi).
Your task is to correct the provided .pyi file content based on the validation errors and warnings.
The goal is to make the .pyi file accurately reflect the .pyx file's public interface,
including classes, methods, member variables, docstrings, and type annotations.

Here is the content of the original Cython file:
```python
{current_pyx_content}
```

Here is the current content of the stub file:
```python
{current_pyi_content}
```

Here is the validation feedback from the script:
```
{validation_output}
```

Please provide ONLY the corrected content for '{pyi_file.name}'.
Do NOT include any explanations, comments, or additional text outside of the file content.
Ensure the corrected content is a complete and valid .pyi file.
"""

        corrected_content = get_llm_response(prompt).replace("```python\n", "").replace("```", "")
        if not corrected_content:
            print("Error: LLM did not return any content.", file=sys.stderr)
            return False

        try:
            pyi_file.write_text(corrected_content, encoding="utf-8")
            print(f"Wrote corrected content to {pyi_file}")
        except Exception as e:
            print(f"Error writing to {pyi_file}: {e}", file=sys.stderr)
            return False

    print(f"\n❌ Failed to pass validation for {pyi_file} after {max_attempts} attempts.")
    return False

def main():
    parser = argparse.ArgumentParser(description="LLM agent to correct .pyi stub files based on validation feedback.")
    parser.add_argument("pyx_dir", type=Path, help="Base directory containing .pyx files")
    parser.add_argument("pyi_dir", type=Path, help="Base directory containing .pyi stub files")
    parser.add_argument(
        "-a", "--attempts",
        type=int,
        default=2,
        help="Maximum number of correction attempts (default: 3)"
    )
    args = parser.parse_args()

    if not args.pyx_dir.is_dir():
        print(f"Error: PYX directory not found: {args.pyx_dir}", file=sys.stderr)
        sys.exit(1)
    if not args.pyi_dir.is_dir():
        print(f"Error: PYI directory not found: {args.pyi_dir}", file=sys.stderr)
        sys.exit(1)

    all_success = True
    for pyx_path_str in glob.glob(str(args.pyx_dir / "**/*.pyx"), recursive=True):
        pyx_file = Path(pyx_path_str)
        relative_path = pyx_file.relative_to(args.pyx_dir)
        pyi_file = args.pyi_dir / relative_path.with_suffix(".pyi")

        if not pyi_file.exists():
            print(f"Warning: Corresponding .pyi file not found for {pyx_file}. Skipping.", file=sys.stderr)
            continue

        print(f"\n--- Processing {pyx_file} and {pyi_file} ---")
        success = correct_pyi_with_llm(pyx_file, pyi_file, args.attempts)
        if not success:
            all_success = False

    sys.exit(0 if all_success else 1)

if __name__ == "__main__":
    main()
