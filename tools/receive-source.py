"""Trusted receiver entry point: python -I -S -B receive-source.py --help."""
from pathlib import Path
import sys

# Only this explicitly trusted helper directory is added in isolated mode.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from source_custody import main

if __name__ == '__main__':
    main()
