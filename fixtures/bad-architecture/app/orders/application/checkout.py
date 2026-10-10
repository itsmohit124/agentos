"""Application layer depending outward on the adapter."""

from app.orders.adapter.db import MemoryStore


class CheckoutFlow:
    """Coordinates checkout from the wrong direction."""

    def run(self, store: MemoryStore) -> str:
        return store.save("o-1")

    def status(self) -> str:
        return "open"
