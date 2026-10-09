/** Persistence for charges. */

export class BaseRepository {
  save(payment) {
    return payment;
  }
}

export class PaymentRepository extends BaseRepository {
  save(payment) {
    return payment;
  }
}
