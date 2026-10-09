/** Application service for charges. */

export class PaymentService {
  /**
   * Charge a payment by saving it.
   * @param {PaymentRepository} repository
   * @param {Payment} payment
   * @returns {Payment}
   */
  charge(repository, payment) {
    return repository.save(payment);
  }
}
