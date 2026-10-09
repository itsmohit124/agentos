"""Order domain model. Persistence is pulled in directly."""

import sqlalchemy


class Order:
    """An order placed by a customer."""

    def __init__(self, order_id: str, amount: int) -> None:
        if amount <= 0:
            raise ValueError("amount > 0")
        self.order_id = order_id
        self.amount = amount


class OrderAggregate:
    """Aggregate root for Order."""

    def __init__(self, order: Order) -> None:
        self.order = order
