/** Application service for charges. */

export class PaymentService {
  charge(repository: PaymentRepository, payment: Payment): Payment {
    return repository.save(payment);
  }
}
