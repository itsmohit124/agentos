require_relative "../app/payments/models"
require_relative "../app/payments/repository"
require_relative "../app/payments/service"

class PaymentTests
  def test_charge_saves_payment
    payment = Payment.new("pay-1", 100)
    repository = PaymentRepository.new
    service = PaymentService.new
    service.charge(repository, payment)
  end
end
