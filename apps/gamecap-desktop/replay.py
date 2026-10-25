#!/usr/bin/env python3
"""Save the instant-replay buffer (SIGUSR1 to GPU Screen Recorder)."""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from session import save_replay


def main() -> int:
    return 0 if save_replay() else 1


if __name__ == "__main__":
    sys.exit(main())
