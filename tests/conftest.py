"""Create only the repository's ignored test workspace before pytest temp setup."""
from __future__ import annotations

import pytest


def pytest_sessionstart(session: pytest.Session) -> None:
    (session.config.rootpath / "work").mkdir(exist_ok=True)
