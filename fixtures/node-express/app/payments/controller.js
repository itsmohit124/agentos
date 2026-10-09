/** HTTP entrypoint for charges. */

export class PaymentController {
  /**
   * Create a payment via the service.
   * @param {PaymentService} service
   * @param {Payment} payment
   * @returns {Payment}
   */
  create(service, payment) {
    return service.charge(payment);
  }
}
