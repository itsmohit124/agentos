"""Persistence for payments."""


class BaseRepository:
    """Storage port."""

    def save(self, payment: Payment) -> Payment:
        """Persist a payment."""
        return payment


class PaymentRepository(BaseRepository):
    """Stores payments."""

    def save(self, payment: Payment) -> Payment:
        """Persist a payment and return it."""
        return payment
