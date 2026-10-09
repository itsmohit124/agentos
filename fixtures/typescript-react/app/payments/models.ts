/** A captured charge. */

export const PAYMENT_STATUS = "captured";

export class Payment {
  constructor(paymentId: string, amount: number) {
    this.paymentId = paymentId;
    this.amount = amount;
  }

  paymentId: string;
  amount: number;
}
