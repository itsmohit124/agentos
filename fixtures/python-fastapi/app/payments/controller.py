"""HTTP entrypoint for payments."""


class PaymentController:
    """Accepts payment requests."""

    def create(self, service: PaymentService, payment: Payment) -> Payment:
        """Create a payment via the service."""
        return service.charge(payment)
