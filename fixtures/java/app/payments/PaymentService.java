package app.payments;

/** Application service for charges. */
public class PaymentService {
    public Payment charge(PaymentRepository repository, Payment payment) {
        return repository.save(payment);
    }
}
