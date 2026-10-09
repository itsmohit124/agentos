"""Application service for billing."""


class BillingService:
    """Stores whichever of the three records the caller built."""

    def record(self, payment: Payment) -> str:
        """Record a payment id. Charge and transaction are separate types."""
        return payment.payment_id
