#!/usr/bin/env python3
"""Toggle recording from a global GNOME shortcut."""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from session import toggle  # noqa: E402


def main() -> int:
    toggle()
    return 0


if __name__ == "__main__":
    sys.exit(main())
