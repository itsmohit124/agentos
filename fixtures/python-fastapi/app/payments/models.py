"""Payment domain model."""

PAYMENT_STATUS = "captured"


class Money:
    """A monetary amount. Value object."""

    def __init__(self, amount: int, currency: str) -> None:
        self.amount = amount
        self.currency = currency


class Payment:
    """A payment to capture."""

    def __init__(self, payment_id: str, amount: int) -> None:
        if amount <= 0:
            raise ValueError("amount > 0")
        self.payment_id = payment_id
        self.amount = amount
        self.money = Money(amount, "USD")


class PaymentAggregate:
    """Aggregate root for a payment and its money."""

    def __init__(self, payment: Payment) -> None:
        self.root = payment
