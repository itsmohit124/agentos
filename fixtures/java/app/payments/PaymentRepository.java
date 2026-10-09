package app.payments;

/** Persistence for charges. */
interface PaymentStore {
    Payment save(Payment payment);
}

class BaseRepository implements PaymentStore {
    public Payment save(Payment payment) {
        return payment;
    }
}

public class PaymentRepository extends BaseRepository {
    public Payment save(Payment payment) {
        return payment;
    }
}
