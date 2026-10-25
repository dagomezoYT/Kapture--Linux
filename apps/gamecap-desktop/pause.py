#!/usr/bin/env python3
"""Pause/resume the active recording (SIGUSR2 / gsr-cli)."""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from session import toggle_pause


def main() -> int:
    result = toggle_pause()
    return 0 if result in {"pause", "resume"} else 1


if __name__ == "__main__":
    sys.exit(main())
