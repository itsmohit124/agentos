"""Persistence port for orders. The domain model does not use this module."""


class OrderRepository:
    """Stores orders."""

    def save(self, order: Order) -> Order:
        """Persist an order."""
        return order
