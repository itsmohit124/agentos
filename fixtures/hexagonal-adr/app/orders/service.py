"""Order application service."""


class OrderService:
    """Loads an order aggregate through the repository."""

    def place(self, repository: OrderRepository, order: Order) -> Order:
        """Save a new order."""
        return repository.save(order)
