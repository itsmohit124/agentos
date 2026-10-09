package app.payments;

/** A captured charge. */
public class Payment {
    public Payment(String paymentId, int amount) {
        this.paymentId = paymentId;
        this.amount = amount;
    }

    public final String paymentId;
    public final int amount;
}
