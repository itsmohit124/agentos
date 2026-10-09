/** HTTP entrypoint for charges. */

export class PaymentController {
  create(service: PaymentService, payment: Payment): Payment {
    return service.charge(payment);
  }
}
