"""Payment domain model."""

PAYMENT_STATUS = "captured"


class Payment:
    """A payment to capture."""

    def __init__(self, payment_id: str, amount: int) -> None:
        self.payment_id = payment_id
        self.amount = amount
