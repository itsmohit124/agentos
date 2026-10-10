"""Adapter that the domain should not import."""


class MemoryStore:
    """Outer storage."""

    def save(self, order_id: str) -> str:
        return order_id

    def load(self, order_id: str) -> str:
        return order_id
