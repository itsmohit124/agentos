import { Payment } from "../app/payments/models";
import { PaymentRepository } from "../app/payments/repository";
import { PaymentService } from "../app/payments/service";

export function test_charge_saves_payment(): boolean {
  const payment = new Payment("pay-1", 100);
  const repository = new PaymentRepository();
  const service = new PaymentService();
  return service.charge(repository, payment) === payment;
}
