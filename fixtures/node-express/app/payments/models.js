/** A captured charge. */

const PAYMENT_STATUS = "captured";

export class Payment {
  constructor(paymentId, amount) {
    this.paymentId = paymentId;
    this.amount = amount;
  }
}
