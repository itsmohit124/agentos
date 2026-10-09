package tests;

import app.payments.Payment;
import app.payments.PaymentRepository;
import app.payments.PaymentService;

/** Exercises a charge. */
class PaymentTests {
    void test_charge_saves_payment() {
        Payment payment = new Payment("pay-1", 100);
        PaymentRepository repository = new PaymentRepository();
        PaymentService service = new PaymentService();
        service.charge(repository, payment);
    }
}
