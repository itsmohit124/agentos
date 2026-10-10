"""Right side of a module cycle."""

from app.cycle.left import LeftA


class RightA:
    """First type in the right module."""

    def ping(self) -> str:
        return "right-a"

    def pong(self) -> str:
        return "right-a2"


class RightB:
    """Second type in the right module."""

    def ping(self) -> str:
        return LeftA().ping()

    def pong(self) -> str:
        return "right-b2"
