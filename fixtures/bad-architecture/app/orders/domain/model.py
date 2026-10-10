"""Order domain model. Infrastructure and the adapter are imported here."""

import sqlalchemy

from app.orders.adapter.db import MemoryStore


class Order:
    """An entity."""

    def __init__(self, order_id: str, amount: int) -> None:
        if amount <= 0:
            raise ValueError("amount > 0")
        self.order_id = order_id
        self.amount = amount

    def label(self) -> str:
        return self.order_id

    def total(self) -> int:
        return self.amount

    def stored(self) -> str:
        return MemoryStore().save(self.order_id)
