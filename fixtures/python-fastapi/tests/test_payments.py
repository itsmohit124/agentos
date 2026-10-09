"""Tests for payments."""

from app.payments.models import Payment
from app.payments.repository import PaymentRepository
from app.payments.service import PaymentService


def test_charge_saves_payment() -> None:
    """Charging a payment saves it through the repository."""
    payment = Payment("pay-1", 100)
    repository = PaymentRepository()
    service = PaymentService()
    saved = service.charge(repository, payment)
    assert saved == payment


def test_amount_must_be_positive() -> None:
    """Invariant: amount > 0."""
    try:
        Payment("pay-0", 0)
    except ValueError as error:
        assert str(error) == "amount > 0"
        return
    raise AssertionError("amount > 0")
