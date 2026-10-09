/** Persistence for charges. */

export interface PaymentStore {
  save(payment: Payment): Payment;
}

export class BaseRepository implements PaymentStore {
  save(payment: Payment): Payment {
    return payment;
  }
}

export class PaymentRepository extends BaseRepository {
  save(payment: Payment): Payment {
    return payment;
  }
}
