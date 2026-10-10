"""Left side of a module cycle."""

from app.cycle.right import RightA


class LeftA:
    """First type in the left module."""

    def ping(self) -> str:
        return "left-a"

    def pong(self) -> str:
        return "left-a2"


class LeftB:
    """Second type in the left module."""

    def ping(self) -> str:
        return "left-b"

    def pong(self) -> str:
        return "left-b2"
