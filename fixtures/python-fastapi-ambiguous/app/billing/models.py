"""Billing names that have not been reconciled."""


class Payment:
    """A captured payment."""

    def __init__(self, payment_id: str, amount: int) -> None:
        self.payment_id = payment_id
        self.amount = amount


class Charge:
    """A card charge. Not yet decided to be the same thing as a payment."""

    def __init__(self, charge_id: str, amount: int) -> None:
        self.charge_id = charge_id
        self.amount = amount


class Transaction:
    """A ledger transaction. Finance still says this instead of payment."""

    def __init__(self, transaction_id: str, amount: int) -> None:
        self.transaction_id = transaction_id
        self.amount = amount
