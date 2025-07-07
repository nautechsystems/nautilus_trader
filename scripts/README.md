# Scripts directory

This directory contains assorted helper scripts used by NautilusTrader’s
developer tooling and CI pipeline. Only one of them (`curate-dataset.sh`)
needs a brief explanation because it is meant to be executed manually when
curating test-fixture datasets.

---

## `curate-dataset.sh` – package an external dataset for the test-data bucket

`curate-dataset.sh` automates the small but repetitive tasks required when we
bring a third-party file into the NautilusTrader *test-data* bucket:

- download the raw file from its original URL (with retries)
- create a versioned directory (`v1/<slug>/`)
- copy the file into that directory
- write a `LICENSE.txt` file holding the SPDX identifier or licence URL
- compute size and SHA-256 checksum and store them in `metadata.json`

The result is a self-contained directory ready to upload one-for-one to the
S3 bucket (or to commit into the repository if the data size is small).

### Usage

```bash
scripts/curate-dataset.sh <slug> <filename> <download-url> <licence>
```

- **`slug`** – sub-directory name (e.g. `fi2010_all`)
- **`filename`** – the basename you want inside the directory (e.g. `Fi2010.zip`)
- **`download-url`** – original public URL of the file
- **`licence`** – short ID or full URL (e.g. `CC-BY-SA-4.0`)

Example – curate the full FI-2010 limit-order-book dataset (all 10 trading
days) from a Dropbox mirror:

```bash
scripts/curate-dataset.sh fi2010_all Fi2010.zip \
  "https://www.dropbox.com/s/6ywf3td7zdrp1n5/Fi2010.zip?dl=1" \
  CC-BY-SA-4.0
```

After the script finishes you will have the following structure ready to
commit or upload:

```
v1/fi2010_all/
 ├── Fi2010.zip          # ≈230 MB, contains day_1 … day_10
 ├── LICENSE.txt         # CC-BY-SA-4.0
 └── metadata.json       # size, sha256, provenance
```

You can now reference `v1/fi2010_all/Fi2010.zip` from tests or example code,
and downstream tooling can verify the checksum.

### Notes

- The script uses `curl -L --fail --retry 3`, so transient network hiccups are
  handled automatically.
- Re-running the script with the same arguments simply overwrites the existing
  files – useful when the upstream file is updated and you want to bump the
  checksum.
- Only basic validation is performed; ensure that the licence you specify
  indeed permits redistribution.

---

## `create-pyo3-config.py` – create stable PyO3 configuration

`create-pyo3-config.py` creates a `.pyo3-config.txt` file that prevents PyO3
from rebuilding when switching between different build commands. This script
addresses a build performance issue where PyO3 would detect environment
changes and trigger full rebuilds (5-30 minutes) unnecessarily.

### Why this script is necessary

When using `uv build`, PyO3's build system detects changes in the Python environment
paths and triggers complete rebuilds of all PyO3 modules. This happens because:

1. **UV creates temporary environments**: Each `uv build` creates a new temporary
   Python environment with a unique path like `/.cache/uv/builds-v0/<hash>/`
2. **PyO3 embeds environment paths**: The pyo3-build-config crate detects these
   path changes and invalidates its cache

### What the script does

The script creates a stable PyO3 configuration file that:

* Uses the Python on the $PATH instead of temporary build environment Python
   (unless called from inside the temporary build environment)
* Provides consistent paths that don't change between builds
* Finally, this prevents PyO3 from auto-detecting environment changes

The file is not regenerated if it already exists and is identical to what would
be generated. It would cause a rebuild, if file timestamps were changed.

### Usage

The script is automatically called by the Makefile's `pyo3-config` target (which is
dependently run by all build targets, test targets and other targets, which might
cause the PyO3 build to be triggered), and by `build.py` when it is stalled or
nonexistent.

It can also be run manually:

```bash
scripts/create-pyo3-config.py
```

This creates `.pyo3-config.txt` in the project root with the current Python
environment configuration.

### Integration with build system

The build system uses this configuration through environment variables:

* `PYO3_CONFIG_FILE`: Points to the generated config file

In this case, it is not validated, as it is assumed to be created by the
Make or an user, who is responsible for ensuring its correctness.

If the environment variable isn't set, the setup script will look for
`.pyo3-config.txt` in the project root and validate it.

If the validation fails, it will be regenerated automatically with paths,
which will further cause the PyO3 rebuilds. This ensures that the build
won't fail due to missing or stalled PyO3 configuration.

---

For details on the other helper scripts, run them with `-h` or read the
inline comments; they are mostly invoked from CI and rarely need manual
execution.
