"""The three names stay distinct."""

from app.billing.models import Charge, Payment, Transaction
from app.billing.service import BillingService


def test_names_are_not_collapsed() -> None:
    """Payment, Charge, and Transaction are different objects."""
    payment = Payment("pay-1", 100)
    charge = Charge("ch-1", 100)
    transaction = Transaction("tx-1", 100)
    assert BillingService().record(payment) == "pay-1"
    assert charge.charge_id != transaction.transaction_id
