# Shell Scripts

This page defines when to create a shell script and how to name, write, invoke, and test it.
Bash is the default. Use POSIX `sh` only when a supported caller cannot rely on Bash being
installed.

The policy applies to shell files throughout the repository, including `scripts/`, `scripts/ci/`,
`.pre-commit-hooks/`, and component directories.

## When to write a script

Before adding a script, search for an existing script, Make target, or installed tool that already
provides the behavior. Add a script when it gives procedural logic one testable, linted source of
truth.

| Location       | Owns                                                                 | Delegates                                              |
| -------------- | -------------------------------------------------------------------- | ------------------------------------------------------ |
| GitHub Actions | Events, permissions, matrices, runners, secrets, and hosted actions  | Multi-step shell behavior                              |
| `Makefile`     | Discoverable tasks, dependencies, variables, and concurrency limits  | Non-trivial control flow                               |
| Shell script   | Validation, reusable command sequences, retries, and transformations | Workflow orchestration and build dependency management |

Prefer a script when:

- A command sequence is used by more than one workflow, Make target, or developer task.
- An inline GitHub Actions step contains branches, loops, retries, or failure handling worth
  testing outside the workflow.
- A Make recipe needs enough procedural logic that quoting, error propagation, or platform
  behavior becomes hard to review.
- A repeated maintenance or release task benefits from ShellCheck, `shfmt`, and focused tests.

Keep a command inline when it is short, used once, and clearer in its caller. Do not wrap one
stable command only to add another file.

When extracting GitHub Actions logic, keep expressions such as `${{ github.ref }}`, permissions,
secret selection, and runner selection in the workflow. Pass ordinary values to the script through
arguments or environment variables. The script's exit status must remain the step's exit status.

## Choose the shell and extension

The extension identifies the shell language, not whether the file is executable or sourceable.

| Extension | Interpreter | Use                                                                      |
| --------- | ----------- | ------------------------------------------------------------------------ |
| `.bash`   | Bash        | Default for new scripts and sourceable Bash files.                       |
| `.sh`     | POSIX `sh`  | Only when a supported caller has a real requirement to run without Bash. |

Use `#!/usr/bin/env bash` for `.bash` files and `#!/usr/bin/env sh` for `.sh` files. Keep the
shebang even when Make or GitHub Actions invokes the file through `bash` or `sh`, because tools and
direct callers use it to identify the interpreter.

Bash is preferred for normal development, Make, and CI scripts because the repository already
depends on it and its features make non-trivial shell code clearer. These features include
`pipefail`, `[[ ... ]]`, arrays, process substitution, and function-local variables.

Use POSIX `sh` for a small bootstrap or wrapper only when avoiding a Bash dependency is part of its
supported interface. Test the script under the target `/bin/sh`; simple syntax alone does not prove
POSIX compatibility.

Existing filenames predate this extension policy, so some `.sh` files contain Bash. Treat their
shebangs as the source of truth. Do not rename an existing script only to change its extension.
Apply the policy to new files and to scoped renames that already update every call site and document.

## Define the portability target

A script must support every platform on which its callers run. Unless its purpose states a narrower
target, write it for Linux, macOS, and Windows through Git Bash, MSYS2, or WSL.

Use Bash 3.2 as the default language floor because it is available on supported macOS systems.
Avoid Bash 4+ features unless every caller provisions a newer version. Common Bash 4+ features and
portable alternatives include:

| Feature                           | Bash version | Portable alternative               |
| --------------------------------- | ------------ | ---------------------------------- |
| Associative arrays (`declare -A`) | 4.0+         | Files, simple arrays, or functions |
| `readarray` / `mapfile`           | 4.0+         | `while read` loops                 |
| `${var,,}` / `${var^^}`           | 4.0+         | `tr` for case conversion           |

A CI-only script may use a newer Bash version or platform-specific tool when every workflow caller
guarantees that environment. Document the constraint near the code that depends on it. A path under
`scripts/ci/` does not by itself make a script Linux-only because CI also uses macOS and Windows
runners.

### System utilities

Prefer options supported by both GNU and BSD utilities. When no common form exists, detect the
capability or operating system and implement both forms.

| Operation         | GNU form       | BSD or macOS form | Portable approach                                         |
| ----------------- | -------------- | ----------------- | --------------------------------------------------------- |
| In-place `sed`    | `sed -i`       | `sed -i ''`       | Use a backup suffix such as `sed -i.bak`, then remove it. |
| File size         | `stat -c '%s'` | `stat -f '%z'`    | Try or select the supported form.                         |
| SHA-256           | `sha256sum`    | `shasum -a 256`   | Detect the command and keep output handling equal.        |
| Canonical path    | `readlink -f`  | No common form    | Avoid it or resolve from a known directory with `pwd`.    |
| Extended matching | `grep -P`      | No common form    | Use `grep -E` when it expresses the same pattern.         |
| Nanosecond time   | `date +%N`     | No common form    | Use an existing run ID or `$RANDOM` for cache busting.    |

Quote paths and expansions so spaces do not change argument boundaries. Do not assume filesystem
paths are case-sensitive. Use repository-relative paths only after resolving the repository root
from the script location, not from the caller's working directory.

Use only commands installed by the documented development or runner setup. If an optional command
is necessary, check for it with `command -v` and report how to install or replace it. The repository
stores text with LF endings through `.gitattributes`; do not add platform-specific line endings.

## Place and name scripts

- Put general development and maintenance commands under `scripts/`.
- Put workflow-specific build, test, publication, and verification commands under `scripts/ci/`.
- Put repository checks invoked by pre-commit under `.pre-commit-hooks/`.
- Keep component-specific scripts beside the component when moving them to `scripts/` would hide
  their ownership.

- Under `scripts/` and in component directories, use lowercase kebab-case. Name a regression script
  `test-<script-name>.bash` or `test-<script-name>.sh` to keep the tested pair together.
- Under `.pre-commit-hooks/`, use lowercase snake_case and match the existing `check_*` and
  `test_check_*` name families.

Keep each script focused on one task, and extend an existing script when new logic shares the same
responsibility.

## Structure scripts for reliable execution

Standalone Bash scripts should start with:

```bash
#!/usr/bin/env bash
set -euo pipefail
```

Use `set -eu` in a standalone POSIX `sh` script. POSIX does not define `pipefail`, so check pipeline
behavior explicitly when a failure must propagate. If a script cannot use these options, explain
the specific control flow that makes an option unsafe.

Follow these requirements:

- Validate required arguments and environment variables before changing state. Print concise usage
  text and exit nonzero for invalid input.
- Quote parameter expansions. Use Bash arrays for argument lists instead of building a command
  string, and do not use `eval`.
- Keep machine-readable output on standard output and diagnostics on standard error when callers
  capture the result.
- Do not end routine status output with a terminating period. Keep punctuation when the output is
  a complete explanatory or diagnostic sentence.
- Create temporary files with `mktemp`, register cleanup with `trap`, and constrain cleanup to the
  exact paths created by the script.
- Bound retries, report the final failure, and return a nonzero status when the requested operation
  does not complete.
- Do not print secrets or enable command tracing around credentials. Pass secrets through the
  environment or the tool's supported secret input.
- Use comments only for constraints or behavior that the commands do not make clear.

Give a standalone script executable permissions when users or tools call it as `./path`. A
sourceable file does not need executable permissions. Prefer executing a script in a child process;
source a file only when the caller must share its functions or shell state.

A sourceable Bash file must not call `exit` or change the caller's shell options. Return errors from
functions and let the caller choose its error policy. Use `${BASH_SOURCE[0]}` instead of `$0` to
resolve the source file's location.

When a script needs several functions, define `main` first, place called functions below their
callers, and invoke `main "$@"` after all definitions. This keeps the task visible at the top while
ensuring every function exists before execution starts.

## Integrate with Make and GitHub Actions

Make targets should provide the stable, discoverable command that developers run. Keep target
dependencies, build variables, and concurrency limits in the Makefile, then invoke the script with
an explicit interpreter. For example, the `check-generated-drift` target delegates its procedural
work to `scripts/ci/check-generated-drift.bash`.

GitHub Actions should provide workflow context through named environment variables and call the
same script used locally where practical. The build workflow follows this boundary: it owns the
Python matrix condition and `TARGET_DIR`, then invokes `scripts/ci/check-generated-drift.bash`.

Keep GitHub-specific output files such as `$GITHUB_OUTPUT` and `$GITHUB_ENV` at the workflow boundary
when the script is also a local command. A script dedicated to GitHub Actions may write them when
that integration is its stated purpose.

## Format and lint

The pre-commit configuration formats shell files with `shfmt` using two-space indentation, indented
case branches, consistent redirect spacing, and the Bash parser. ShellCheck then checks quoting,
expansion, control flow, portability, and common command errors. The pinned hooks in
`.pre-commit-config.yaml` are the source of truth for tool versions and options.

Run both hooks against the exact changed scripts:

```bash
prek run shfmt --files scripts/ci/test-wheel.bash
prek run shellcheck --files scripts/ci/test-wheel.bash
```

`shfmt` updates files in place. Review its changes before running ShellCheck. ShellCheck selects the
language from the shebang, so a `.sh` file with `#!/usr/bin/env sh` receives POSIX checks even though
the formatter uses the common Bash parser.

Keep ShellCheck suppressions on the narrowest applicable line. Add a nearby reason when the
constraint is not clear from the code, and do not disable a check for the whole repository to
silence one script.

The repository also rejects executable files without shebangs, mixed line endings, trailing
whitespace, and unresolved merge markers. These checks complement ShellCheck; they do not replace a
runtime test.

## Test behavior

Run the smallest test that exercises the changed branches and failure paths. For logic that can
regress independently of a workflow, add a companion shell test. This includes parsing,
multi-branch decisions, retries and cleanup, policy checks, and material external side effects. A
domain-level suite may cover cooperating scripts, and a thin wrapper does not need a one-to-one test
when that suite invokes it and proves its behavior.

Each companion test:

- Creates isolated state under `mktemp -d` and removes it on exit.
- Supplies fake external commands through a temporary `PATH` instead of changing production code.
- Uses distinct inputs and exact output, exit status, and side-effect assertions.
- Covers success, invalid input, dependency failure, and cleanup when those paths exist.
- Fails when a required test command is unavailable; a passing skip does not validate behavior.
- Runs from `make test-scripts`, which is the script test inventory used by CI.

When a script has callers on multiple operating systems, exercise platform-sensitive changes on
each caller's relevant CI matrix. A Linux test plus clean ShellCheck output does not prove macOS or
Windows behavior.

## Review checklist

- The behavior is not already available from a script, Make target, installed tool, or simple
  native workflow feature.
- GitHub Actions, Make, and the script own the right parts of the workflow.
- The extension, shebang, executable mode, and documented portability target agree.
- The script runs independently of the caller's working directory.
- Arguments, failures, temporary files, retries, output, and secrets have explicit handling.
- Focused behavior tests, `shfmt`, and ShellCheck pass for the final changed files.
- Every call site, workflow path filter, and document uses the final filename.
