"""Untested domain entity."""


class UntestedCharge:
    """An entity."""

    def __init__(self, charge_id: str, amount: int) -> None:
        self.charge_id = charge_id
        self.amount = amount

    def label(self) -> str:
        return self.charge_id

    def total(self) -> int:
        return self.amount
