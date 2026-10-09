"""Payment application service."""


class PaymentService:
    """Charges payments through the repository."""

    def charge(self, repository: PaymentRepository, payment: Payment) -> Payment:
        """Charge a payment by saving it."""
        return repository.save(payment)
