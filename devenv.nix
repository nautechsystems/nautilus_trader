{ pkgs, ... }:

{
  languages.python = {
    enable = true;
    uv = {
      enable = true;
      sync.enable = true;
    };
    venv.enable = true;
  };

  languages.rust = {
    enable = true;
    channel = "stable";
    version = "1.92.0";
  };

  enterShell = ''
    echo "Nautilus Trader development environment"
    echo "Python $(python --version | cut -d' ' -f2) with uv package management"
    echo "Rust $(rustc --version | cut -d' ' -f2) for core components"
    echo ""
    echo "Available commands:"
    echo "  uv sync          - Sync Python dependencies"
    echo "  cargo build      - Build Rust components"
    echo "  pytest           - Run Python tests"
    echo "  cargo test       - Run Rust tests"
    echo "  pre-commit run   - Run pre-commit hooks"
  '';
}