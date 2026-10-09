package app.payments;

/** HTTP entrypoint for charges. */
public class PaymentController {
    public Payment create(PaymentService service, Payment payment) {
        return service.charge(payment);
    }
}
