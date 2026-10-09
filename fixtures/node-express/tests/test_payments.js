import { Payment } from "../app/payments/models.js";
import { PaymentRepository } from "../app/payments/repository.js";
import { PaymentService } from "../app/payments/service.js";

export function test_charge_saves_payment() {
  const payment = new Payment("pay-1", 100);
  const repository = new PaymentRepository();
  const service = new PaymentService();
  return service.charge(repository, payment);
}
